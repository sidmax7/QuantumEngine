//! Every Harman headset, microphone and earbud this crate can recognise, and
//! what is known about each one working with it.
//!
//! Only the Quantum 810 Wireless is verified. The rest are listed so they can
//! be named correctly and so what users report about them is tracked in one
//! place: the app shows it next to each control, and the README's hardware
//! table is generated from it (see [`markdown_table`]). A family or a list of
//! lighting zones describes the hardware; it never makes a feature work. Only
//! a report changes a feature's [`Status`].
//!
//! Where the facts come from: the USB IDs and model names are from JBL's
//! public Windows audio driver (`QuantumUSBAudio.inf`); the lighting zones are
//! the zones JBL's own app draws for each model. Both were typed in by hand.
//! No JBL code or data file is part of this crate.

use std::fmt;

/// What sort of device it is. Only headsets can be controlled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Headset,
    Earbuds,
    Microphone,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Headset => "headset",
            Kind::Earbuds => "earbuds",
            Kind::Microphone => "microphone",
        }
    }
}

/// Which line of JBL's range a model belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// The top wireless headsets: Quantum 800, 810, 850, 910, 950 and One.
    Flagship,
    /// Quantum 350 to 650.
    MidRange,
    /// USB-wired headsets: Quantum 50C, 300 and 400.
    Wired,
    Earbuds,
    Microphone,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::Flagship => "flagship",
            Family::MidRange => "mid-range",
            Family::Wired => "wired",
            Family::Earbuds => "earbuds",
            Family::Microphone => "microphone",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Family::Flagship => "Wireless flagship",
            Family::MidRange => "Mid-range",
            Family::Wired => "Wired",
            Family::Earbuds => "Earbuds",
            Family::Microphone => "Microphone",
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Family::Earbuds => Kind::Earbuds,
            Family::Microphone => Kind::Microphone,
            _ => Kind::Headset,
        }
    }
}

/// A part of the headset that lights up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightZone {
    Logo,
    Ring,
    Notch,
}

impl LightZone {
    pub fn as_str(self) -> &'static str {
        match self {
            LightZone::Logo => "logo",
            LightZone::Ring => "ring",
            LightZone::Notch => "notch",
        }
    }
}

/// How well one feature works on one model with this software.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    /// Tested by the project on real hardware.
    Verified,
    /// A user reported it working.
    Reported,
    /// Works, with a condition the note explains.
    Caveat,
    /// Nobody has tried it.
    Untested,
    /// Tried, and it doesn't work.
    Broken,
    /// The model doesn't have it, e.g. noise control on a wired Quantum 400.
    Absent,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Verified => "verified",
            Status::Reported => "reported",
            Status::Caveat => "caveat",
            Status::Untested => "untested",
            Status::Broken => "broken",
            Status::Absent => "absent",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Verified => "Verified",
            Status::Reported => "Reported working",
            Status::Caveat => "Works, with a catch",
            Status::Untested => "Untested",
            Status::Broken => "Doesn't work",
            Status::Absent => "Not on this model",
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Status::Verified => "✅",
            Status::Reported => "🟢",
            Status::Caveat => "🟡",
            Status::Untested => "⚪",
            Status::Broken => "❌",
            Status::Absent => "➖",
        }
    }
}

/// A feature's status, why, and where that came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Support {
    pub status: Status,
    /// One sentence for the user, or empty.
    pub note: &'static str,
    /// Where the status comes from, e.g. `issue #2`, or empty when untested.
    pub source: &'static str,
}

impl Support {
    pub const UNTESTED: Support = Support { status: Status::Untested, note: "", source: "" };

    const fn verified(note: &'static str) -> Support {
        Support { status: Status::Verified, note, source: "tested by the project" }
    }

    const fn from(status: Status, note: &'static str, source: &'static str) -> Support {
        Support { status, note, source }
    }
}

/// The features tracked for a headset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// Battery, connection, microphone, dial, serial and firmware.
    Reads,
    /// Off / ANC / TalkThru.
    NoiseControl,
    Sidetone,
    /// Lights on and off.
    Lights,
    /// Colours, brightness and effects.
    Lighting,
    /// The PC-side equalizer and JBL's headset tuning.
    Eq,
}

impl Feature {
    pub const ALL: [Feature; 6] =
        [Feature::Reads, Feature::NoiseControl, Feature::Sidetone, Feature::Lights, Feature::Lighting, Feature::Eq];

    pub fn as_str(self) -> &'static str {
        match self {
            Feature::Reads => "reads",
            Feature::NoiseControl => "anc",
            Feature::Sidetone => "sidetone",
            Feature::Lights => "lights",
            Feature::Lighting => "lighting",
            Feature::Eq => "eq",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Feature::Reads => "Battery and status",
            Feature::NoiseControl => "Noise control",
            Feature::Sidetone => "Sidetone",
            Feature::Lights => "Lights on/off",
            Feature::Lighting => "Lighting effects",
            Feature::Eq => "Equalizer",
        }
    }
}

/// What is known about each feature on one headset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Features {
    pub reads: Support,
    pub noise_control: Support,
    pub sidetone: Support,
    pub lights: Support,
    pub lighting: Support,
    pub eq: Support,
}

impl Features {
    pub const UNTESTED: Features = Features {
        reads: Support::UNTESTED,
        noise_control: Support::UNTESTED,
        sidetone: Support::UNTESTED,
        lights: Support::UNTESTED,
        lighting: Support::UNTESTED,
        eq: Support::UNTESTED,
    };

    pub fn get(&self, feature: Feature) -> Support {
        match feature {
            Feature::Reads => self.reads,
            Feature::NoiseControl => self.noise_control,
            Feature::Sidetone => self.sidetone,
            Feature::Lights => self.lights,
            Feature::Lighting => self.lighting,
            Feature::Eq => self.eq,
        }
    }
}

/// One product.
#[derive(Debug, PartialEq, Eq)]
pub struct Model {
    /// e.g. `Quantum 800`.
    pub name: &'static str,
    pub family: Family,
    /// Every USB product ID it shows up as (dongle, cable, console variants).
    pub ids: &'static [u16],
    /// What lights up, as JBL's own app draws it.
    pub zones: &'static [LightZone],
    /// `None` for anything that isn't a headset: there is nothing to control.
    pub features: Option<Features>,
}

impl Model {
    pub fn kind(&self) -> Kind {
        self.family.kind()
    }

    /// Whether every feature has been tested by the project.
    pub fn verified(&self) -> bool {
        self.features.is_some_and(|f| Feature::ALL.iter().all(|&x| f.get(x).status == Status::Verified))
    }

    /// The model name with JBL's prefix, e.g. `JBL Quantum 800`.
    pub fn full_name(&self) -> String {
        format!("JBL {}", self.name)
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

/// The vendor every model here shares (Harman International).
pub const VENDOR: u16 = 0x0ECB;

use Family::*;
use LightZone::*;

const NONE: &[LightZone] = &[];
const LOGO: &[LightZone] = &[Logo];
const LOGO_RING: &[LightZone] = &[Logo, Ring];
const LOGO_RING_NOTCH: &[LightZone] = &[Logo, Ring, Notch];

const HEADSET: Option<Features> = Some(Features::UNTESTED);

/// Verified, feature by feature, on the project's own headset.
const QUANTUM_810: Features = Features {
    reads: Support::verified(""),
    noise_control: Support::verified(""),
    sidetone: Support::verified(""),
    lights: Support::verified(""),
    lighting: Support::verified("Effects, speed and colours as in JBL's own app."),
    eq: Support::verified(""),
};

/// From InkyQuill's report on issue #2.
const QUANTUM_800: Features = Features {
    reads: Support::from(Status::Reported, "Every read answers; it reports no serial number.", "issue #2"),
    noise_control: Support::from(
        Status::Caveat,
        "Only with the mic boom raised; with it down the headset plays an error tone.",
        "issue #2",
    ),
    sidetone: Support::from(Status::Reported, "", "issue #2"),
    lights: Support::from(Status::Reported, "", "issue #2"),
    // Before 0.4.5 the ring followed only some effects (issue #2): effects
    // were sent that JBL doesn't define, too fast for the 800. Rebuilt from
    // JBL's own app since; waiting for a new report.
    lighting: Support {
        status: Status::Untested,
        note: "Rebuilt in 0.4.5 to work the way JBL's own app drives the 800; not tried yet.",
        source: "",
    },
    eq: Support::UNTESTED,
};

/// From realtoxicmaik's report on issue #4. Before 0.4.8 the app waited for
/// a wireless link the 400 doesn't have, so nothing could be changed on it.
const QUANTUM_400: Features = Features {
    reads: Support::from(
        Status::Reported,
        "Mic, dial, lights, serial and firmware answer; being wired, it has no battery or wireless link.",
        "issue #4",
    ),
    noise_control: Support::from(Status::Absent, "", "JBL's app has none for it"),
    sidetone: Support::UNTESTED,
    lights: Support::UNTESTED,
    lighting: Support::from(
        Status::Untested,
        "JBL's own app drives its logo the same way as the 810's lights.",
        "",
    ),
    eq: Support::from(
        Status::Caveat,
        "Choose QuantumEngine as the sound output yourself; it only attaches to the 810 on its own.",
        "issue #4",
    ),
};

/// Every model, flagships first.
pub static MODELS: &[Model] = &[
    Model { name: "Quantum 810 Wireless", family: Flagship, ids: &[0x2069], zones: LOGO_RING, features: Some(QUANTUM_810) },
    Model { name: "Quantum 800", family: Flagship, ids: &[0x1F47, 0x203E], zones: LOGO_RING, features: Some(QUANTUM_800) },
    Model { name: "Quantum 850 Wireless", family: Flagship, ids: &[0x219A, 0x219B, 0x219C], zones: NONE, features: HEADSET },
    Model { name: "Quantum 910 Wireless", family: Flagship, ids: &[0x2088, 0x209C], zones: LOGO_RING_NOTCH, features: HEADSET },
    Model {
        name: "Quantum 910P Console Wireless",
        family: Flagship,
        ids: &[0x20A2, 0x20A4],
        zones: LOGO_RING_NOTCH,
        features: HEADSET,
    },
    Model {
        name: "Quantum 910X Wireless for Xbox",
        family: Flagship,
        ids: &[0x209D, 0x209F, 0x2147, 0x214A],
        zones: LOGO_RING_NOTCH,
        features: HEADSET,
    },
    Model { name: "Quantum 950 Wireless", family: Flagship, ids: &[0x2127, 0x2128, 0x2129], zones: NONE, features: HEADSET },
    Model { name: "Quantum 950X Wireless", family: Flagship, ids: &[0x2141], zones: NONE, features: HEADSET },
    Model { name: "Quantum 950X Wireless for Xbox", family: Flagship, ids: &[0x213E, 0x2140], zones: NONE, features: HEADSET },
    Model { name: "Quantum One", family: Flagship, ids: &[0x1F48, 0x203A, 0x20C2], zones: LOGO_RING_NOTCH, features: HEADSET },
    Model { name: "Quantum 350 Wireless", family: MidRange, ids: &[0x2047, 0x206B, 0x20B2], zones: NONE, features: HEADSET },
    Model { name: "Quantum 360 Wireless", family: MidRange, ids: &[0x2115], zones: NONE, features: HEADSET },
    Model { name: "Quantum 360P Console Wireless", family: MidRange, ids: &[0x20AB], zones: NONE, features: HEADSET },
    Model { name: "Quantum 360X Wireless for Xbox", family: MidRange, ids: &[0x20A5, 0x2142], zones: NONE, features: HEADSET },
    Model { name: "Quantum 520 Wireless", family: MidRange, ids: &[0x218D, 0x218E], zones: NONE, features: HEADSET },
    Model { name: "Quantum 520X Wireless for Xbox", family: MidRange, ids: &[0x21A7, 0x21A9], zones: NONE, features: HEADSET },
    Model { name: "Quantum 600", family: MidRange, ids: &[0x1F46, 0x203C], zones: LOGO, features: HEADSET },
    Model { name: "Quantum 610 Wireless", family: MidRange, ids: &[0x205C], zones: LOGO, features: HEADSET },
    Model { name: "Quantum 650 Wireless", family: MidRange, ids: &[0x2125, 0x2126], zones: NONE, features: HEADSET },
    Model { name: "Quantum 650X Wireless for Xbox", family: MidRange, ids: &[0x213B, 0x213D], zones: NONE, features: HEADSET },
    Model { name: "Quantum 50C", family: Wired, ids: &[0x2112], zones: NONE, features: HEADSET },
    Model { name: "Quantum 300", family: Wired, ids: &[0x1F52], zones: NONE, features: HEADSET },
    Model { name: "Quantum 400", family: Wired, ids: &[0x1F4C, 0x2039, 0x20EE], zones: LOGO, features: Some(QUANTUM_400) },
    Model { name: "Quantum TWS", family: Earbuds, ids: &[0x208A, 0x209B], zones: NONE, features: None },
    Model { name: "Quantum TWS Air", family: Earbuds, ids: &[0x20BE], zones: NONE, features: None },
    Model { name: "Quantum Stream", family: Microphone, ids: &[0x205F], zones: NONE, features: None },
    Model { name: "Quantum Stream Studio", family: Microphone, ids: &[0x20B0], zones: NONE, features: None },
    Model { name: "Quantum Stream Talk", family: Microphone, ids: &[0x20AF], zones: NONE, features: None },
    Model { name: "Quantum Stream Wireless", family: Microphone, ids: &[0x20ED], zones: NONE, features: None },
];

/// The model a USB `vendor:product` belongs to, if it's one listed here.
pub fn lookup(vendor: u16, product: u16) -> Option<&'static Model> {
    if vendor != VENDOR {
        return None;
    }
    MODELS.iter().find(|m| m.ids.contains(&product))
}

/// The same, from `0ecb:203e`.
pub fn lookup_id(id: &str) -> Option<&'static Model> {
    let (vendor, product) = crate::access::parse_id(id)?;
    lookup(vendor, product)
}

/// The README's hardware table, as Markdown, generated from [`MODELS`].
pub fn markdown_table() -> String {
    let mut out = String::new();
    out.push_str("| Model | USB IDs | Family | Battery and status | Noise control | Sidetone | Lights | Lighting effects | EQ |\n");
    out.push_str("|---|---|---|---|---|---|---|---|---|\n");
    for m in MODELS {
        let ids = m.ids.iter().map(|p| format!("`{VENDOR:04x}:{p:04x}`")).collect::<Vec<_>>().join(" ");
        let cells = match &m.features {
            Some(f) => Feature::ALL.iter().map(|&x| cell(f.get(x))).collect::<Vec<_>>().join(" | "),
            None => format!("not a headset{}", " | —".repeat(Feature::ALL.len() - 1)),
        };
        out.push_str(&format!("| {} | {ids} | {} | {cells} |\n", m.name, m.family.label()));
    }
    out.push_str(
        "\n✅ verified by the project · 🟢 reported working · 🟡 works, with a catch · ⚪ untested · ❌ doesn't work · ➖ not on this model\n",
    );
    out
}

fn cell(s: Support) -> String {
    let source = if s.source.is_empty() || s.status == Status::Verified { String::new() } else { format!(" ({})", s.source) };
    if s.note.is_empty() {
        format!("{}{source}", s.status.symbol())
    } else {
        format!("{} {}{source}", s.status.symbol(), s.note)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn no_usb_id_belongs_to_two_models() {
        let mut seen = HashSet::new();
        for m in MODELS {
            for id in m.ids {
                assert!(seen.insert(*id), "{id:04x} is listed twice (again under {})", m.name);
            }
        }
        assert_eq!(seen.len(), 53, "the driver lists 53 IDs");
        assert_eq!(MODELS.len(), 29);
    }

    #[test]
    fn names_are_unique_and_every_model_has_an_id() {
        let mut names = HashSet::new();
        for m in MODELS {
            assert!(names.insert(m.name), "{} twice", m.name);
            assert!(!m.ids.is_empty(), "{} has no ID", m.name);
        }
    }

    #[test]
    fn only_the_810_is_verified() {
        let verified: Vec<_> = MODELS.iter().filter(|m| m.verified()).map(|m| m.name).collect();
        assert_eq!(verified, ["Quantum 810 Wireless"]);
        // And no other model has a single verified feature.
        for m in MODELS.iter().filter(|m| m.name != "Quantum 810 Wireless") {
            if let Some(f) = m.features {
                for x in Feature::ALL {
                    assert_ne!(f.get(x).status, Status::Verified, "{} {}", m.name, x.as_str());
                }
            }
        }
    }

    #[test]
    fn every_claim_says_where_it_came_from() {
        for m in MODELS {
            if let Some(f) = m.features {
                for x in Feature::ALL {
                    let s = f.get(x);
                    if s.status != Status::Untested {
                        assert!(!s.source.is_empty(), "{} {} has no source", m.name, x.as_str());
                    }
                    if s.status == Status::Caveat || s.status == Status::Broken {
                        assert!(!s.note.is_empty(), "{} {} needs a note saying what goes wrong", m.name, x.as_str());
                    }
                }
            }
        }
    }

    #[test]
    fn only_headsets_have_features() {
        for m in MODELS {
            assert_eq!(m.features.is_some(), m.kind() == Kind::Headset, "{}", m.name);
        }
    }

    #[test]
    fn the_verified_product_is_the_one_this_crate_opens() {
        let m = lookup(crate::VENDOR_ID, crate::PRODUCT_ID).unwrap();
        assert!(m.verified());
        assert_eq!(lookup_id("0ecb:203e").unwrap().name, "Quantum 800");
        assert_eq!(lookup_id("0ecb:1f47").unwrap().name, "Quantum 800");
        assert!(lookup(0x1234, 0x2069).is_none());
        assert!(lookup_id("0ecb:ffff").is_none());
    }

    /// The README's hardware table is this catalogue. Refresh it with
    /// `QUANTUMENGINE_UPDATE_README=1 cargo test -p jbl-quantum`.
    #[test]
    fn the_readme_table_matches_the_catalogue() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../README.md");
        let Ok(readme) = std::fs::read_to_string(&path) else {
            return; // Building from a source tree without the repository's README.
        };
        let start = readme.find("<!-- models:start").expect("README has a models:start marker");
        let start = start + readme[start..].find("-->").expect("the marker is closed") + 3;
        let end = readme.find("<!-- models:end -->").expect("README has a models:end marker");
        let fresh = format!("\n{}", markdown_table());
        if std::env::var_os("QUANTUMENGINE_UPDATE_README").is_some() {
            let updated = format!("{}{}{}", &readme[..start], fresh, &readme[end..]);
            std::fs::write(&path, updated).unwrap();
            return;
        }
        assert!(
            readme[start..end] == fresh,
            "README.md's hardware table is stale; run QUANTUMENGINE_UPDATE_README=1 cargo test -p jbl-quantum"
        );
    }

    #[test]
    fn the_table_has_a_row_per_model() {
        let table = markdown_table();
        assert_eq!(table.lines().filter(|l| l.starts_with("| Quantum")).count(), MODELS.len());
        assert!(table.contains("| Quantum 800 | `0ecb:1f47` `0ecb:203e` | Wireless flagship | 🟢"));
        assert!(table.contains("Quantum Stream | `0ecb:205f` | Microphone | not a headset"));
    }
}
