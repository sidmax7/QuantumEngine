//! Report IDs and value encodings.
//!
//! Every constant here is traceable to `docs/protocol/spec.yaml` in the
//! research repo, which records how each was established.
//!
//! Deliberately absent: report IDs 0xF0, 0xF1, 0xF4, 0xF5, 0xF6 and 0x9A.
//! Those are the suspected firmware/DFU channel. They are not defined here, not
//! behind a feature flag and not commented out, so no bug in this crate can
//! reach them.

pub const VENDOR_ID: u16 = 0x0ECB; // Harman International
pub const PRODUCT_ID: u16 = 0x2069; // JBL Quantum 810 Wireless

/// Feature reports, readable with GET_REPORT and (some) writable with SET_REPORT.
pub mod feature {
    /// ANC state, read-back mirror of [`ANC`].
    pub const ANC_MIRROR: u8 = 0x45;
    /// ANC state. Writing here also updates [`ANC_MIRROR`].
    pub const ANC: u8 = 0x46;
    /// Battery level, direct percentage 0-100.
    pub const BATTERY: u8 = 0x49;
    /// Lights on/off, read-back mirror of [`LIGHTS`].
    pub const LIGHTS_MIRROR: u8 = 0x4A;
    /// Lights master on/off.
    pub const LIGHTS: u8 = 0x4B;
    /// Write: per-zone brightness and slot count. Read: a status block whose
    /// byte 0 is the lights state — NOT the brightness that was written.
    pub const LIGHT_CONFIG: u8 = 0x4C;
    /// Write: per-slot colour and effect. Read: status block, as [`LIGHT_CONFIG`].
    pub const LIGHT_COLOUR: u8 = 0x4D;
    /// Firmware versions: four 3-byte major.minor.patch triplets.
    pub const FIRMWARE: u8 = 0x51;
    /// Sidetone level, read-back mirror of [`SIDETONE`].
    pub const SIDETONE_MIRROR: u8 = 0x5C;
    /// Sidetone level.
    pub const SIDETONE: u8 = 0x5D;
    /// Name the headset has for the paired host: a length byte, one unknown
    /// byte, then ASCII.
    pub const DEVICE_NAME: u8 = 0x5B;
    /// Serial number, 16 ASCII characters.
    pub const SERIAL: u8 = 0x61;
    /// Game/chat dial position, 0-16.
    pub const DIAL: u8 = 0x62;
    /// Microphone active (boom down AND unmuted).
    pub const MIC_ACTIVE: u8 = 0x67;
    /// Headset connection state.
    pub const CONNECTION: u8 = 0x68;
    /// Connection state, mirror of [`CONNECTION`].
    pub const CONNECTION_MIRROR: u8 = 0x69;
}

/// Input report IDs, pushed by the device on the interrupt endpoint.
pub mod event {
    pub const ANC: u8 = 0x02;
    pub const BLUETOOTH: u8 = 0x03;
    pub const MIC: u8 = 0x06;
    pub const LIGHTS: u8 = 0x07;
    /// Battery percentage. NOTE: this fires on any host interaction, not only
    /// when the level changes.
    pub const BATTERY: u8 = 0x08;
    /// Seen only in the connect burst; meaning unknown.
    pub const UNKNOWN_09: u8 = 0x09;
    pub const DIAL: u8 = 0x10;
}

/// Active noise cancelling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anc {
    Off,
    On,
    TalkThru,
}

impl Anc {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Off),
            0x01 => Some(Self::On),
            0x02 => Some(Self::TalkThru),
            _ => None,
        }
    }

    pub fn as_byte(self) -> u8 {
        match self {
            Self::Off => 0x00,
            Self::On => 0x01,
            Self::TalkThru => 0x02,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::TalkThru => "talkthru",
        }
    }
}

/// Sidetone (microphone monitoring) level. A four-step enum, not a slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sidetone {
    Off,
    Low,
    Mid,
    High,
}

impl Sidetone {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Off),
            0x01 => Some(Self::Low),
            0x02 => Some(Self::Mid),
            0x03 => Some(Self::High),
            _ => None,
        }
    }

    pub fn as_byte(self) -> u8 {
        match self {
            Self::Off => 0x00,
            Self::Low => 0x01,
            Self::Mid => 0x02,
            Self::High => 0x03,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Mid => "mid",
            Self::High => "high",
        }
    }
}

/// A lighting element. These are NOT left and right earcups — the device
/// mirrors left and right unconditionally and offers no per-side control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Logo,
    Ring,
}

impl Zone {
    pub const ALL: [Zone; 2] = [Zone::Logo, Zone::Ring];

    pub fn as_byte(self) -> u8 {
        match self {
            Self::Logo => 0x00,
            Self::Ring => 0x01,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logo => "logo",
            Self::Ring => "ring",
        }
    }
}

/// Lighting animation.
///
/// A newtype rather than a closed enum: values above 0x0A were never tested, so
/// the range may extend further and this must not reject what it hasn't seen.
/// Several effects render differently on the ring than on the logo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect(pub u8);

impl Effect {
    /// Dims, changes colour, brightens again.
    pub const FADE: Effect = Effect(0x00);
    /// Instant colour change, no fade.
    pub const INSTANT: Effect = Effect(0x01);
    /// Spatial chase — travels around the ring, radial on the logo.
    /// This is the effect the vendor software's own captures use.
    pub const CHASE: Effect = Effect(0x02);
    /// Rapid blink.
    pub const STROBE: Effect = Effect(0x03);
    /// Ring chases while the logo strobes.
    pub const CHASE_STROBE_LOGO: Effect = Effect(0x04);
    /// Ring chases while the logo flashes in step.
    pub const CHASE_FLASH_LOGO: Effect = Effect(0x05);
    /// Both elements running fast.
    pub const FAST: Effect = Effect(0x07);
    /// Chase with the logo blinking about twice per cycle.
    pub const CHASE_BLINK_LOGO: Effect = Effect(0x08);
    /// As [`Effect::CHASE_BLINK_LOGO`] with a rainbow logo.
    pub const RAINBOW_LOGO: Effect = Effect(0x09);
    /// Generates colours beyond the configured palette.
    pub const SPECTRUM: Effect = Effect(0x0A);

    /// The effects with distinct observed behaviour, for listing in a UI.
    /// 0x06 is omitted: it was indistinguishable from 0x05 in testing.
    pub const KNOWN: [(Effect, &'static str); 10] = [
        (Self::FADE, "fade"),
        (Self::INSTANT, "instant"),
        (Self::CHASE, "chase"),
        (Self::STROBE, "strobe"),
        (Self::CHASE_STROBE_LOGO, "chase-strobe-logo"),
        (Self::CHASE_FLASH_LOGO, "chase-flash-logo"),
        (Self::FAST, "fast"),
        (Self::CHASE_BLINK_LOGO, "chase-blink-logo"),
        (Self::RAINBOW_LOGO, "rainbow-logo"),
        (Self::SPECTRUM, "spectrum"),
    ];

    pub fn name(self) -> Option<&'static str> {
        Self::KNOWN
            .iter()
            .find(|(e, _)| *e == self)
            .map(|(_, name)| *name)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::KNOWN
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(e, _)| *e)
    }
}

/// An 8-bit-per-channel colour. Byte order on the wire is plain RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Colour {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse `#rrggbb` or `rrggbb`.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self::new(
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
        ))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// Maximum colour slots the device animates through, per zone.
pub const MAX_SLOTS: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anc_round_trips() {
        for mode in [Anc::Off, Anc::On, Anc::TalkThru] {
            assert_eq!(Anc::from_byte(mode.as_byte()), Some(mode));
        }
        assert_eq!(Anc::from_byte(0x03), None);
    }

    #[test]
    fn sidetone_round_trips() {
        for level in [Sidetone::Off, Sidetone::Low, Sidetone::Mid, Sidetone::High] {
            assert_eq!(Sidetone::from_byte(level.as_byte()), Some(level));
        }
        assert_eq!(Sidetone::from_byte(0x04), None);
    }

    #[test]
    fn colour_hex_round_trips() {
        let c = Colour::new(0x99, 0xFF, 0x00);
        assert_eq!(c.to_hex(), "#99ff00");
        assert_eq!(Colour::parse_hex("#99ff00"), Some(c));
        assert_eq!(Colour::parse_hex("99ff00"), Some(c));
        assert_eq!(Colour::parse_hex("#99ff0"), None);
        assert_eq!(Colour::parse_hex("zzzzzz"), None);
    }

    #[test]
    fn effect_names_round_trip() {
        for (effect, name) in Effect::KNOWN {
            assert_eq!(Effect::from_name(name), Some(effect));
            assert_eq!(effect.name(), Some(name));
        }
        // An untested value stays representable but unnamed.
        assert_eq!(Effect(0x40).name(), None);
    }
}
