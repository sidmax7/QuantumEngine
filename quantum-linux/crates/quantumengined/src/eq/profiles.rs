//! The built-in EQ curves.
//!
//! The values come from JBL QuantumENGINE's own configuration files, as
//! extracted and converted by alessandromrc in
//! <https://github.com/alessandromrc/jbl-quantum-4-linux> (the `Q810` headset
//! curves and the 10-band game and genre presets). Thanks to alessandromrc for
//! the extraction, and to DefSteam for testing them on real hardware.

use super::biquad::{Band, FilterType};
use std::f64::consts::FRAC_1_SQRT_2;
use serde::{Deserialize, Serialize};

/// Every sound profile is ten bells at these frequencies, all with this Q.
pub const PROFILE_FREQS: [f64; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0];
const PROFILE_Q: f64 = 1.0;

pub struct Profile {
    pub name: &'static str,
    pub gains: [f64; 10],
}

impl Profile {
    pub fn bands(&self) -> Vec<Band> {
        PROFILE_FREQS.iter().zip(self.gains).map(|(&f, g)| Band::bell(f, g, PROFILE_Q)).collect()
    }
}

/// `Flat` comes first and is the default.
pub const PROFILES: &[Profile] = &[
    Profile { name: "Flat", gains: [0.0; 10] },
    Profile { name: "Bass Boost", gains: [6.0, 6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.0] },
    Profile { name: "Cinematic", gains: [4.0, 3.0, 2.0, 0.0, -1.0, 1.0, 2.0, -1.0, -2.0, -2.0] },
    Profile { name: "FPS", gains: [-5.0, -3.0, -1.0, 0.0, -1.0, 1.0, 4.0, 2.0, -1.0, 0.0] },
    Profile { name: "MOBA", gains: [3.0, 4.0, 2.0, 1.0, -1.0, -1.0, 1.0, 2.0, -3.0, -3.0] },
    Profile { name: "RPG", gains: [3.0, 3.0, 3.0, 0.0, -2.0, -2.0, -2.0, 1.0, 2.0, -2.0] },
    Profile { name: "Apex Legends", gains: [-6.0, 3.0, 2.0, -3.0, 2.0, 1.0, 2.0, 4.0, 1.0, -4.0] },
    Profile { name: "CS2", gains: [-6.0, -4.0, -3.0, -2.0, 2.0, 3.0, 4.0, 4.0, 3.0, 0.0] },
    Profile { name: "Dota 2", gains: [4.0, 3.0, 1.0, -4.0, -1.0, 3.0, 4.0, 1.0, 3.0, -4.0] },
    Profile { name: "Escape from Tarkov", gains: [-4.0, 3.0, -3.0, 6.0, 3.0, 1.0, 3.0, 6.0, 2.0, 1.0] },
    Profile { name: "Fortnite", gains: [-4.0, 3.0, 2.0, -3.0, -4.0, 3.0, 3.0, 4.0, 2.0, -4.0] },
    Profile { name: "GTA 5", gains: [-1.0, 1.0, 2.0, -3.0, 3.0, 4.0, 3.0, 3.0, 2.0, -2.0] },
    Profile { name: "League of Legends", gains: [2.0, 3.0, 1.0, -3.0, -4.0, 1.0, 2.0, 4.0, 1.0, -3.0] },
    Profile { name: "LN3 Immersion", gains: [4.0, 5.0, 4.0, 1.0, -3.0, -2.0, 1.0, 3.0, 1.0, -2.0] },
    Profile { name: "LN3 Thrill", gains: [1.0, 0.0, -1.0, -5.0, 2.0, 1.0, 3.0, 4.0, 2.0, 1.0] },
    Profile { name: "Overwatch 2", gains: [2.0, 2.0, -4.0, 2.0, -6.0, 4.0, 6.0, 5.0, 2.0, -4.0] },
    Profile { name: "PUBG", gains: [-1.0, -2.0, -1.0, -1.0, 2.0, 3.0, 0.0, 5.0, 4.0, -2.0] },
    Profile { name: "Valorant", gains: [-3.0, 4.0, 3.0, -2.0, -1.0, 1.0, 3.0, 4.0, 1.0, -4.0] },
    Profile { name: "WoW", gains: [3.0, 2.0, 2.0, -1.0, -2.0, -1.0, 1.0, 3.0, -2.0, -2.0] },
];

/// Looks a profile up by name, ignoring case, spaces and punctuation, so
/// `bass-boost` and `BASS BOOST` both find "Bass Boost".
pub fn find(name: &str) -> Option<&'static Profile> {
    let key = normalise(name);
    PROFILES.iter().find(|p| normalise(p.name) == key)
}

/// The key names are compared by: lowercase letters and digits only.
pub fn normalise(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// JBL's per-model correction for the Quantum 810, applied underneath the
/// sound profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Correction {
    Off,
    /// The curve QuantumENGINE uses with surround off: a gentle bass cut.
    #[default]
    Stereo,
    /// The curve QuantumENGINE puts under its spatial audio.
    Surround,
}

impl Correction {
    pub const ALL: [Correction; 3] = [Correction::Off, Correction::Stereo, Correction::Surround];

    /// What to apply for `model`: this one if the model has JBL's tuning, or
    /// if none is known yet (the 810's applies, as it always did), and nothing
    /// otherwise, so a headset is never shaped for another one.
    pub fn in_use(self, model: Option<&jbl_quantum::models::Model>) -> Self {
        if model.is_none_or(|m| m.has_eq_tuning()) { self } else { Correction::Off }
    }


    pub fn as_str(self) -> &'static str {
        match self {
            Correction::Off => "off",
            Correction::Stereo => "stereo",
            Correction::Surround => "surround",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }

    /// QuantumENGINE's stereo preset also turns the output down 6 dB; that
    /// is left out here because headroom is worked out from the whole curve
    /// instead (see `eq::chain`).
    pub fn bands(self) -> Vec<Band> {
        match self {
            Correction::Off => Vec::new(),
            Correction::Stereo => vec![Band { kind: FilterType::LowShelf, freq: 300.0, gain_db: -2.0, q: 1.0 }],
            Correction::Surround => vec![
                Band::bell(13_000.0, 10.0, 0.6),
                Band { kind: FilterType::HighShelf, freq: 4_000.0, gain_db: -2.0, q: FRAC_1_SQRT_2 },
                Band::bell(11_000.0, -12.0, 3.5),
                Band::bell(6_000.0, -10.0, 3.0),
                Band::bell(3_000.0, 2.0, 2.5),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_flat_comes_first() {
        assert_eq!(PROFILES[0].name, "Flat");
        assert!(PROFILES[0].gains.iter().all(|&g| g == 0.0));
        for (i, a) in PROFILES.iter().enumerate() {
            for b in &PROFILES[i + 1..] {
                assert_ne!(normalise(a.name), normalise(b.name));
            }
        }
    }

    #[test]
    fn lookup_ignores_case_and_punctuation() {
        assert_eq!(find("bass-boost").unwrap().name, "Bass Boost");
        assert_eq!(find("cs2").unwrap().name, "CS2");
        assert_eq!(find("Escape From Tarkov").unwrap().name, "Escape from Tarkov");
        assert!(find("nope").is_none());
    }

    #[test]
    fn only_a_model_with_tuning_gets_the_chosen_correction() {
        use jbl_quantum::models::lookup;
        let (q810, q400) = (lookup(0x0ecb, 0x2069), lookup(0x0ecb, 0x20ee));
        for c in Correction::ALL {
            assert_eq!(c.in_use(q810), c);
            assert_eq!(c.in_use(None), c, "no model known yet keeps the choice");
            assert_eq!(c.in_use(q400), Correction::Off);
        }
    }

    #[test]
    fn corrections_round_trip_through_their_names() {
        for c in Correction::ALL {
            assert_eq!(Correction::parse(c.as_str()), Some(c));
        }
    }
}
