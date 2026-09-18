//! The D-Bus contract between `quantumengine --tray` (the only process that
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
//! The `Set*` methods block until the headset confirms the change (as
//! `jbl_quantum::Headset`'s own `set_*_confirmed` calls do) and fail with
//! `org.freedesktop.DBus.Error.Failed` if it doesn't. Only one command runs
//! at a time — the service serialises every caller through the same worker
//! thread that owns the headset, the same as a single in-process client
//! would.

use serde::{Deserialize, Serialize};
use zbus::proxy;
use zbus::zvariant::Type;

/// The well-known bus name `quantumengine --tray` claims on the session bus.
pub const BUS_NAME: &str = "org.quantumengine.Headset1";
/// The single object every call and property lives on.
pub const OBJECT_PATH: &str = "/org/quantumengine/Headset1";

/// One lighting zone, for [`HeadsetProxy::set_lighting`]. Mirrors
/// `jbl_quantum::ZoneLighting`, but this crate never depends on that crate —
/// the service converts each side.
#[derive(Debug, Clone, PartialEq, Eq, Type, Serialize, Deserialize)]
pub struct ZoneLightingArg {
    /// `"logo"` or `"ring"`.
    pub zone: String,
    /// 0-100.
    pub brightness: u8,
    /// 1-5 `(r, g, b)` triples; the device animates through them.
    pub colours: Vec<(u8, u8, u8)>,
    /// The raw effect byte — see `jbl_quantum::protocol::Effect`'s named
    /// constants for the ones with known behaviour.
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
}
