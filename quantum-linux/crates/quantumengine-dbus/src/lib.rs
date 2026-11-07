//! The D-Bus contract between `quantumengined` (the only process that
//! ever talks to the headset) and every other QuantumEngine client — the
//! window, `quantumenginectl`, and the Plasma widget.
//!
//! Defining the interface once, here, means the server and its clients
//! cannot drift apart: the `#[proxy]` macro below generates [`HeadsetProxy`]
//! (async) and [`HeadsetProxyBlocking`] straight from these method
//! signatures, so a client only ever needs `use quantumengine_dbus::*` — it
//! never hand-writes a matching call.
//!
//! # Contract
//!
//! Properties are cheap: the service answers them from an in-memory cache
//! kept fresh by its own worker thread, never by asking the headset while a
//! client waits. Read [`HeadsetProxyBlocking::link`] first — the other
//! properties hold stale or default values until it reads `"ready"`.
//!
//! Enum-shaped values are plain strings rather than a custom D-Bus type, so
//! they need no extra plumbing from QML or shell scripts:
//! * `Link`: `"no_dongle"` | `"no_access"` | `"headset_off"` | `"ready"`.
//! * `Anc`: `"off"` | `"on"` | `"talkthru"`.
//! * `Sidetone`: `"off"` | `"low"` | `"mid"` | `"high"`.
//!
//! The EQ, surround and update members don't involve the headset at all; they are on
//! the same object so a client needs only one proxy.
//!
//! The headset `Set*` methods block until the headset confirms the change (as
//! `jbl_quantum::Headset`'s own `set_*_confirmed` calls do) and fail with
//! `org.freedesktop.DBus.Error.Failed` if it doesn't. Only one command runs
//! at a time — the service serialises every caller through the same worker
//! thread that owns the headset, the same as a single in-process client
//! would.

use serde::{Deserialize, Serialize};
use zbus::proxy;
use zbus::zvariant::Type;

/// The well-known bus name `quantumengined` claims on the session bus.
pub const BUS_NAME: &str = "org.quantumengine.Headset1";
/// The single object every call and property lives on.
pub const OBJECT_PATH: &str = "/org/quantumengine/Headset1";

/// How many points [`HeadsetProxy::eq_response`] returns.
pub const EQ_RESPONSE_POINTS: usize = 64;

/// The frequency, in Hz, of point `i` of [`HeadsetProxy::eq_response`]: the
/// points are spaced evenly on a log scale from 20 Hz to 20 kHz, both
/// included.
pub fn eq_response_freq(i: usize) -> f64 {
    let t = i as f64 / (EQ_RESPONSE_POINTS - 1) as f64;
    20.0 * 1000f64.powf(t)
}

/// One EQ band, as `(type, frequency in Hz, gain in dB, Q)`. The type is
/// `"bell"`, `"lowshelf"`, `"highshelf"`, `"highpass"` or `"lowpass"`; the
/// pass filters ignore the gain.
pub type EqBand = (String, f64, f64, f64);

/// The most bands a custom EQ profile can have.
pub const EQ_MAX_BANDS: usize = 18;

/// One lighting zone, for [`HeadsetProxy::set_lighting`]. Mirrors
/// `jbl_quantum::ZoneLighting`, but this crate never depends on that crate —
/// the service converts each side.
#[derive(Debug, Clone, PartialEq, Eq, Type, Serialize, Deserialize)]
pub struct ZoneLightingArg {
    /// `"logo"` or `"ring"`.
    pub zone: String,
    /// 0-100; dims the colours (the headset has no brightness setting).
    pub brightness: u8,
    /// Animation speed, 0-100.
    pub speed: u8,
    /// 1-10 `(r, g, b)` triples; the device animates through them.
    pub colours: Vec<(u8, u8, u8)>,
    /// The effect byte, 0-6 — see `jbl_quantum::protocol::Effect`.
    pub effect: u8,
}

#[proxy(
    interface = "org.quantumengine.Headset1",
    default_service = "org.quantumengine.Headset1",
    default_path = "/org/quantumengine/Headset1",
    gen_blocking = true
)]
pub trait Headset {
    /// `"no_dongle"` | `"no_access"` | `"headset_off"` | `"ready"`. The other
    /// properties are only meaningful once this reads `"ready"`.
    #[zbus(property)]
    fn link(&self) -> zbus::Result<String>;

    /// The access error's message when [`link`](Self::link) is
    /// `"no_access"` (typically a missing udev rule); empty otherwise.
    #[zbus(property)]
    fn link_detail(&self) -> zbus::Result<String>;

    /// Battery percentage, 0-100.
    #[zbus(property)]
    fn battery(&self) -> zbus::Result<u8>;

    /// `"off"` | `"on"` | `"talkthru"`.
    #[zbus(property)]
    fn anc(&self) -> zbus::Result<String>;

    /// `"off"` | `"low"` | `"mid"` | `"high"`.
    #[zbus(property)]
    fn sidetone(&self) -> zbus::Result<String>;

    /// Whether the microphone is live: boom down and not muted.
    #[zbus(property)]
    fn mic_active(&self) -> zbus::Result<bool>;

    /// Game/chat dial position, 0 (full chat) to 16 (full game).
    #[zbus(property)]
    fn dial(&self) -> zbus::Result<u8>;

    #[zbus(property)]
    fn lights_on(&self) -> zbus::Result<bool>;

    /// The name the headset has for this host, e.g. `PC-myhost #1`.
    #[zbus(property)]
    fn device_name(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn serial(&self) -> zbus::Result<String>;

    /// Four `major.minor.patch` triplets.
    #[zbus(property)]
    fn firmware(&self) -> zbus::Result<Vec<String>>;

    /// Whether a `Set*` call is in progress. Other `Set*` calls still queue
    /// and wait rather than fail — this is only for showing a spinner.
    #[zbus(property)]
    fn busy(&self) -> zbus::Result<bool>;

    /// The last failed command's message, or empty. Cleared by the next
    /// command that succeeds.
    #[zbus(property)]
    fn last_error(&self) -> zbus::Result<String>;

    /// Sets noise control and waits for the headset to confirm it. `mode` is
    /// `"off"`, `"on"` or `"talkthru"`. Returns the confirmed mode.
    fn set_anc(&self, mode: &str) -> zbus::Result<String>;

    /// Sets sidetone and waits for it to read back. `level` is `"off"`,
    /// `"low"`, `"mid"` or `"high"`. Returns the confirmed level.
    fn set_sidetone(&self, level: &str) -> zbus::Result<String>;

    /// Switches the lights and waits for the headset to confirm it. Returns
    /// the confirmed state.
    fn set_lights(&self, on: bool) -> zbus::Result<bool>;

    /// Configures lighting for one or more zones and switches the lights on,
    /// as `jbl_quantum::Headset::set_lighting_zones` does — including the
    /// off/settle/on cycle that takes a few hundred milliseconds.
    fn set_lighting(&self, zones: Vec<ZoneLightingArg>) -> zbus::Result<()>;

    // A headset from the same vendor that this software has not been verified
    // against. It is listed and can be probed with reads only; control is
    // opt-in, one feature at a time. All of these are empty when no such
    // device is plugged in.

    /// `vendor:product` of the headset being served (e.g. `0ecb:2069`), or
    /// empty when none is. Look it up in `jbl_quantum::models`.
    #[zbus(property)]
    fn model_id(&self) -> zbus::Result<String>;

    /// `vendor:product` (e.g. `0ecb:203e`) of the unverified device, or empty.
    #[zbus(property)]
    fn unverified_device(&self) -> zbus::Result<String>;

    /// The name the device reports for itself.
    #[zbus(property)]
    fn unverified_name(&self) -> zbus::Result<String>;

    /// Why the device can't be used yet (no access to it, or a permission
    /// request in progress), for showing as-is, or empty.
    #[zbus(property)]
    fn unverified_access(&self) -> zbus::Result<String>;

    /// Whether the user chose to use the device. Off by default.
    #[zbus(property)]
    fn unverified_in_use(&self) -> zbus::Result<bool>;

    /// Which writes the user allowed, any of `"anc"`, `"sidetone"`, `"lights"`.
    /// None by default, which leaves the device read-only.
    #[zbus(property)]
    fn unverified_allowed(&self) -> zbus::Result<Vec<String>>;

    /// Reads (never writes) what the device answers to and returns a plain-text
    /// report, meant for pasting into an issue.
    fn probe_unverified(&self) -> zbus::Result<String>;

    /// Chooses whether to use the unverified device, and which writes to allow.
    fn set_unverified(&self, in_use: bool, allowed: Vec<String>) -> zbus::Result<()>;

    /// Asks for access to the device (a udev rule for this one ID, approved
    /// with the user's password) and returns at once; watch
    /// [`unverified_access`](Self::unverified_access).
    fn grant_unverified_access(&self) -> zbus::Result<()>;

    // Equalizer. Applied on the PC to the headset's game output by a PipeWire
    // filter; works whether or not the headset is connected right now.

    #[zbus(property)]
    fn eq_enabled(&self) -> zbus::Result<bool>;

    /// The sound profile's name, one of [`eq_profiles`](Self::eq_profiles).
    #[zbus(property)]
    fn eq_profile(&self) -> zbus::Result<String>;

    /// Every profile name in display order: the built-in ones (`"Flat"`
    /// first), then the user's own.
    #[zbus(property)]
    fn eq_profiles(&self) -> zbus::Result<Vec<String>>;

    /// The names of the user's own profiles, in the order they were made.
    #[zbus(property)]
    fn eq_custom_profiles(&self) -> zbus::Result<Vec<String>>;

    /// Whether unsaved bands (see [`preview_eq_bands`](Self::preview_eq_bands))
    /// are playing instead of the selected profile.
    #[zbus(property)]
    fn eq_previewing(&self) -> zbus::Result<bool>;

    /// JBL's headset correction under the profile: `"off"` | `"stereo"` |
    /// `"surround"`.
    #[zbus(property)]
    fn eq_correction(&self) -> zbus::Result<String>;

    /// Why the EQ isn't working, or empty when it is (or is off).
    #[zbus(property)]
    fn eq_status(&self) -> zbus::Result<String>;

    fn set_eq_enabled(&self, enabled: bool) -> zbus::Result<bool>;

    /// Selects a profile by name, ignoring case and punctuation. Returns the
    /// profile's display name.
    fn set_eq_profile(&self, name: &str) -> zbus::Result<String>;

    fn set_eq_correction(&self, correction: &str) -> zbus::Result<String>;

    /// The gain, in dB, that `profile` under `correction` applies at each of
    /// the [`EQ_RESPONSE_POINTS`] frequencies [`eq_response_freq`] gives, for
    /// drawing the curve. It is the curve's shape: the overall turn-down that
    /// keeps a boosting curve from clipping is left out.
    fn eq_response(&self, profile: &str, correction: &str) -> zbus::Result<Vec<f64>>;

    /// A profile's bands, built-in or custom, by name.
    fn eq_profile_bands(&self, name: &str) -> zbus::Result<Vec<EqBand>>;

    /// What [`eq_response`](Self::eq_response) gives for a profile, for
    /// bands that aren't saved yet (an editor's curve).
    fn eq_bands_response(&self, bands: Vec<EqBand>, correction: &str) -> zbus::Result<Vec<f64>>;

    /// Saves a custom profile and selects it. `original` is the custom
    /// profile being edited (which this may rename), or empty to add a new
    /// one. Returns the saved name. Fails if the name is taken, or a band is
    /// out of range (10 Hz to 20 kHz, ±30 dB, Q 0.05 to 20; at most
    /// [`EQ_MAX_BANDS`]).
    fn save_eq_profile(&self, original: &str, name: &str, bands: Vec<EqBand>) -> zbus::Result<String>;

    /// Deletes a custom profile; if it was selected, `"Flat"` is selected.
    fn delete_eq_profile(&self, name: &str) -> zbus::Result<()>;

    /// Plays `bands` in place of the selected profile, so an editor's
    /// changes can be heard before saving. Ends with
    /// [`end_eq_preview`](Self::end_eq_preview), a save, or selecting a
    /// profile.
    fn preview_eq_bands(&self, bands: Vec<EqBand>) -> zbus::Result<()>;

    fn end_eq_preview(&self) -> zbus::Result<()>;

    /// Adds a custom profile from Equalizer APO / AutoEQ `ParametricEQ.txt`
    /// text or an EasyEffects preset (JSON), named `name` (or `name 2`... if
    /// taken), and selects it. Returns the name it was saved under.
    fn import_eq_profile(&self, name: &str, text: &str) -> zbus::Result<String>;

    // Virtual 7.1 surround: a PipeWire sink that renders eight channels
    // binaurally for the headset. Turning it on makes it the default output.

    #[zbus(property)]
    fn surround_enabled(&self) -> zbus::Result<bool>;

    /// Why surround isn't working (e.g. the dongle is unplugged), or empty.
    #[zbus(property)]
    fn surround_status(&self) -> zbus::Result<String>;

    fn set_surround_enabled(&self, enabled: bool) -> zbus::Result<bool>;

    // The headset output's volume: the same one the desktop's slider moves,
    // up to 150%, where the part above 100% is a boost that can distort.

    /// The volume in percent, 0 to 150. Only meaningful while
    /// `volume_available`.
    #[zbus(property)]
    fn volume(&self) -> zbus::Result<u32>;

    /// Whether a headset output was found to take the volume of.
    #[zbus(property)]
    fn volume_available(&self) -> zbus::Result<bool>;

    /// Sets the volume (0 to 150) and returns what it reads back as.
    fn set_volume(&self, percent: u32) -> zbus::Result<u32>;

    // Updates from GitHub releases.

    /// The running service's version, e.g. `0.3.0`.
    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;

    /// A newer release's version, or empty.
    #[zbus(property)]
    fn update_version(&self) -> zbus::Result<String>;

    /// The newer release's web page, or empty.
    #[zbus(property)]
    fn update_url(&self) -> zbus::Result<String>;

    /// Whether [`install_update`](Self::install_update) can install it: this
    /// copy came from the pacman package and the release has one to install.
    #[zbus(property)]
    fn update_can_install(&self) -> zbus::Result<bool>;

    /// What the updater is doing or last failed at, for showing as-is.
    #[zbus(property)]
    fn update_status(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn update_busy(&self) -> zbus::Result<bool>;

    /// Whether the service checks for a new release once a day.
    #[zbus(property)]
    fn update_checks(&self) -> zbus::Result<bool>;

    /// Checks now. Returns the newer version, or empty when up to date.
    fn check_for_updates(&self) -> zbus::Result<String>;

    /// Starts installing the available update and returns at once; watch
    /// `UpdateStatus`. Shows a password prompt. On success the service
    /// restarts itself, so clients briefly lose it and reconnect.
    fn install_update(&self) -> zbus::Result<()>;

    fn set_update_checks(&self, enabled: bool) -> zbus::Result<bool>;
}
