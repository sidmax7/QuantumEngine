//! The types that cross the IPC boundary to the UI. Flat, display-ready
//! views of what `quantumengined` reports over D-Bus. Field names reach
//! TypeScript in camelCase.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Whether the window can reach `quantumengined`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    /// Looking for it, or starting it.
    Connecting,
    Up,
    /// It could not be reached or started; `serviceError` says why.
    Down,
}

/// Everything the window shows, pushed whenever any of it changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StateDto {
    pub service: Service,
    pub service_error: String,
    /// The running service is older than this window (or too old to say its
    /// version), so it lacks things the window offers, such as the EQ.
    pub service_outdated: bool,
    pub headset: HeadsetDto,
    pub eq: EqDto,
    pub surround: SurroundDto,
    pub update: UpdateDto,
    pub unverified: UnverifiedDto,
    /// The model being served, from QuantumEngine's catalogue, if it's listed.
    pub model: Option<ModelDto>,
}

/// A headset from the same vendor that this software isn't verified on.
/// `id` is empty when there isn't one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UnverifiedDto {
    /// `vendor:product`, e.g. `0ecb:203e`.
    pub id: String,
    pub name: String,
    /// Why it can't be used yet (no access), or what's being done about it.
    pub access: String,
    pub in_use: bool,
    /// Any of `"anc"`, `"sidetone"`, `"lights"`: the writes the user allowed.
    pub allowed: Vec<String>,
    /// What the catalogue knows about it, if it's listed.
    pub model: Option<ModelDto>,
}

/// One model from QuantumEngine's catalogue (`jbl_quantum::models`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelDto {
    /// `vendor:product`.
    pub id: String,
    /// e.g. `JBL Quantum 800`.
    pub name: String,
    /// e.g. `Wireless flagship`.
    pub family: String,
    /// `"headset"` | `"earbuds"` | `"microphone"`.
    pub kind: String,
    /// Every feature tested by the project.
    pub verified: bool,
    /// What lights up: any of `"logo"`, `"ring"`, `"notch"`.
    pub zones: Vec<String>,
    /// Empty for anything that isn't a headset.
    pub features: Vec<FeatureDto>,
}

/// What is known about one feature on one model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FeatureDto {
    /// `"reads"` | `"anc"` | `"sidetone"` | `"lights"` | `"lighting"` | `"eq"`.
    pub id: String,
    pub label: String,
    /// `"verified"` | `"reported"` | `"caveat"` | `"untested"` | `"broken"` | `"absent"`.
    pub status: String,
    pub status_label: String,
    pub note: String,
    /// Where the status comes from, e.g. `issue #2`.
    pub source: String,
}

impl ModelDto {
    /// The catalogue entry for `id` (`0ecb:203e`), if there is one.
    pub fn lookup(id: &str) -> Option<Self> {
        use jbl_quantum::models::{lookup_id, Feature};
        let m = lookup_id(id)?;
        let features = m
            .features
            .map(|f| {
                Feature::ALL
                    .iter()
                    .map(|&x| {
                        let s = f.get(x);
                        FeatureDto {
                            id: x.as_str().into(),
                            label: x.label().into(),
                            status: s.status.as_str().into(),
                            status_label: s.status.label().into(),
                            note: s.note.into(),
                            source: s.source.into(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            id: id.to_ascii_lowercase(),
            name: m.full_name(),
            family: m.family.label().into(),
            kind: m.kind().as_str().into(),
            verified: m.verified(),
            zones: m.zones.iter().map(|z| z.as_str().into()).collect(),
            features,
        })
    }
}

/// Virtual 7.1 surround, rendered on this PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SurroundDto {
    pub enabled: bool,
    /// Why it isn't working, or empty.
    pub status: String,
}

/// The headset, as the service last saw it. Only meaningful once `link` is
/// `"ready"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HeadsetDto {
    /// `"no_dongle"` | `"no_access"` | `"unverified"` | `"headset_off"` | `"ready"`.
    pub link: String,
    /// Why the dongle can't be opened, when `link` is `"no_access"`.
    pub link_detail: String,
    pub battery: u8,
    /// `"off"` | `"on"` | `"talkthru"`.
    pub anc: String,
    /// `"off"` | `"low"` | `"mid"` | `"high"`.
    pub sidetone: String,
    /// Boom down and not muted.
    pub mic_active: bool,
    /// 0 (full chat) to 16 (full game).
    pub dial: u8,
    pub lights_on: bool,
    /// The name the headset has for this host.
    pub device_name: String,
    pub serial: String,
    pub firmware: Vec<String>,
    /// A headset change is in progress.
    pub busy: bool,
    /// The last failed command's message, or empty.
    pub last_error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EqDto {
    pub enabled: bool,
    pub profile: String,
    pub profiles: Vec<String>,
    /// `"off"` | `"stereo"` | `"surround"`.
    pub correction: String,
    /// Why the EQ isn't working, or empty.
    pub status: String,
    /// The user's own profiles (also in `profiles`, after the built-in ones).
    pub custom: Vec<String>,
    /// Unsaved bands are playing instead of `profile`.
    pub previewing: bool,
}

/// One EQ band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BandDto {
    /// `"bell"` | `"lowshelf"` | `"highshelf"` | `"highpass"` | `"lowpass"`.
    pub kind: String,
    pub freq: f64,
    /// dB; ignored by the pass filters.
    pub gain: f64,
    pub q: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDto {
    /// The running service's version.
    pub service_version: String,
    /// A newer release's version, or empty.
    pub available: String,
    pub can_install: bool,
    /// What the updater is doing or last failed at.
    pub status: String,
    pub busy: bool,
    /// Whether the service checks GitHub once a day.
    pub checks: bool,
}

impl Default for StateDto {
    fn default() -> Self {
        Self {
            service: Service::Connecting,
            service_error: String::new(),
            service_outdated: false,
            headset: HeadsetDto {
                link: "no_dongle".into(),
                link_detail: String::new(),
                battery: 0,
                anc: String::new(),
                sidetone: String::new(),
                mic_active: false,
                dial: 8,
                lights_on: false,
                device_name: String::new(),
                serial: String::new(),
                firmware: Vec::new(),
                busy: false,
                last_error: String::new(),
            },
            eq: EqDto {
                enabled: false,
                profile: String::new(),
                profiles: Vec::new(),
                correction: String::new(),
                status: String::new(),
                custom: Vec::new(),
                previewing: false,
            },
            surround: SurroundDto { enabled: false, status: String::new() },
            unverified: UnverifiedDto {
                id: String::new(),
                name: String::new(),
                access: String::new(),
                in_use: false,
                allowed: Vec::new(),
                model: None,
            },
            model: None,
            update: UpdateDto {
                service_version: String::new(),
                available: String::new(),
                can_install: false,
                status: String::new(),
                busy: false,
                checks: true,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct StateEvent(pub StateDto);

/// One lighting zone as the UI edits it, and as it is remembered between
/// runs (the headset can't report its lighting).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ZoneDto {
    /// 0-100; dims the colours.
    pub brightness: u8,
    /// Animation speed, 0-100.
    pub speed: u8,
    /// `#rrggbb`, one to [`InfoDto::max_colours`] of them.
    pub colours: Vec<String>,
    /// The raw effect byte, one of [`InfoDto::effects`].
    pub effect: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LightingDto {
    pub logo: ZoneDto,
    pub ring: ZoneDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EffectDto {
    pub id: u8,
    /// Human-readable, e.g. `Spin clockwise`.
    pub label: String,
    /// The zones it suits: `"logo"`, `"ring"`.
    pub zones: Vec<String>,
}

/// Facts that don't change while the window is open.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InfoDto {
    /// This window's version.
    pub version: String,
    pub effects: Vec<EffectDto>,
    pub max_colours: u32,
    /// The frequencies, in Hz, of each point `eq_response` returns.
    pub eq_frequencies: Vec<f64>,
    /// The most bands a custom EQ profile can have.
    pub eq_max_bands: u32,
}
