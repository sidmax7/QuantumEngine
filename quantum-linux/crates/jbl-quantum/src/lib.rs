//! Control a JBL Quantum 810 Wireless headset over its vendor HID interface.
//!
//! ```no_run
//! # fn main() -> Result<(), jbl_quantum::Error> {
//! use jbl_quantum::{Anc, Headset, CONFIRM_TIMEOUT};
//!
//! let headset = Headset::open_first()?;
//! println!("{}%", headset.battery()?);
//! let mode = headset.set_anc_confirmed(Anc::TalkThru, CONFIRM_TIMEOUT)?;
//! println!("ANC is now {}", mode.as_str());
//! # Ok(())
//! # }
//! ```
//!
//! Tested against a Quantum 810 Wireless (`0ecb:2069`) on firmware 0.7.2 /
//! 0.8.2. Other Quantum models are not supported: they may or may not share
//! this protocol, and guessing is how headsets get bricked.
//!
//! # Writing settings
//!
//! Each setting has two setters:
//!
//! * `set_*` writes and returns immediately. Use it from an event loop that
//!   will see the outcome as an [`Event`].
//! * `set_*_confirmed` writes and waits until the device reports the new
//!   state, returning it. Use it from one-shot tools.
//!
//! Settings do not all settle the same way (all measured on the device):
//!
//! | setting  | readable mirror after a write | event after a write |
//! |----------|-------------------------------|---------------------|
//! | ANC to Off | correct at once | ~470 ms later |
//! | ANC Off → On/TalkThru | correct at once | ~50–170 ms later |
//! | ANC On ↔ TalkThru | shows the target, then **Off**, then the target; settled after ~250–530 ms | once, at the end |
//! | ANC to its current value | unchanged | **none** |
//! | sidetone | correct at once | never — sidetone has no event |
//! | lights | correct at once | ~50 ms later, on **every** write, even a no-op |
//!
//! On↔TalkThru passes through Off because TalkThru is a separate audio path
//! (speakers off, ambient sound passed through), not a third ANC level: the
//! headset disengages one before engaging the other. So an immediate read-back
//! of ANC is not a reliable confirmation; the event is.
//!
//! # Events
//!
//! Physical controls (ANC button, dial, boom, mute button, lights hold) push
//! events on their own, and every open descriptor receives them. Some *reads*
//! also cause events:
//!
//! * reading the battery ([`Headset::battery`], [`Headset::status`]) always
//!   produces a [`Event::Battery`];
//! * reading the lights state ([`Headset::lights_on`]) can produce
//!   [`Event::Lights`], and does so almost every time when read rapidly.
//!
//! So never re-read a setting in response to its own event — for the battery
//! that is an endless loop. Use the event's value. A lights event produced by a
//! read carries the state at the time of that read and can arrive just after a
//! later write, so avoid reading the lights state around lighting writes.
//!
//! # Developing without the headset
//!
//! [`Headset::open_simulated`] returns a simulated headset that approximates
//! the settling and event behaviour above, so code built on this crate can be
//! developed and tested without the real dongle plugged in. Setting the
//! [`SIMULATE_ENV`] environment variable switches [`Headset::open_first`] to
//! it as well, which is convenient for manually trying out a client:
//! `QUANTUMENGINE_SIMULATE=1 cargo run`.

mod hidraw;
pub mod protocol;
mod sim;

pub use hidraw::HidrawNode;
pub use protocol::{Anc, Colour, Effect, Sidetone, Zone, MAX_SLOTS, PRODUCT_ID, VENDOR_ID};

use hidraw::Transport;
use protocol::feature;
use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A sensible timeout for the `set_*_confirmed` calls. The slowest transition
/// measured (ANC On ↔ TalkThru) completed within ~600 ms.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(2);

/// A lights write that gets no event within this long is sent again. Normal
/// latency measured 45–75 ms. A resend this long after a write is already
/// clear of the window in which the device ignores lights writes.
const LIGHTS_RESEND_AFTER: Duration = Duration::from_millis(250);

/// Pause after the lights-off event before writing the lighting
/// configuration, to clear the window in which the device ignores writes.
const LIGHTS_SETTLE: Duration = Duration::from_millis(150);

#[derive(Debug)]
pub enum Error {
    /// No matching headset dongle is plugged in.
    NotFound,
    /// The dongle is present but could not be opened. Usually the udev rule is
    /// missing — see `udev/70-jbl-quantum.rules`.
    Access(io::Error),
    /// The dongle is present but the headset is switched off or out of range.
    NotConnected,
    /// The device did not report the requested state in time.
    Timeout(String),
    Io(io::Error),
    /// The device returned a value this crate does not recognise.
    Protocol(String),
    /// A caller passed something the device cannot represent.
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "no JBL Quantum 810 dongle found"),
            Self::Access(e) => write!(
                f,
                "cannot access the dongle: {e}\n\
                 install the udev rule and replug it (see udev/70-jbl-quantum.rules)"
            ),
            Self::NotConnected => write!(
                f,
                "the dongle is connected but the headset is not (switched off or out of range)"
            ),
            Self::Timeout(m) => write!(f, "timed out: {m}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Protocol(m) => write!(f, "unexpected value from device: {m}"),
            Self::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::PermissionDenied => Self::Access(e),
            _ => Self::Io(e),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Everything readable in one shot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub connected: bool,
    pub battery_percent: u8,
    pub anc: Anc,
    pub sidetone: Sidetone,
    pub mic_active: bool,
    /// Game/chat dial, 0 = full chat, 8 = centre, 16 = full game.
    pub dial: u8,
    pub lights_on: bool,
    pub serial: String,
    /// Four `major.minor.patch` triplets. Which component each belongs to is
    /// not established.
    pub firmware: Vec<String>,
    /// The name the headset has for the paired host.
    pub device_name: String,
}

/// Lighting for one zone, for [`Headset::set_lighting_zones`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneLighting {
    pub zone: Zone,
    /// 0-100.
    pub brightness: u8,
    /// 1..=[`MAX_SLOTS`] colours.
    pub palette: Vec<Colour>,
    pub effect: Effect,
}

/// An open connection to the headset dongle, real or simulated.
pub struct Headset {
    hid: Box<dyn Transport>,
    /// Events that arrived while a `set_*_confirmed` call was waiting for
    /// something else. Handed out by `next_event*` before new ones.
    pending: Mutex<VecDeque<Event>>,
    simulated: bool,
}

/// The environment variable that switches [`Headset::open_first`] to a
/// simulated headset instead of discovering real hardware. Any value works,
/// except `off`, which simulates the dongle being plugged in but the headset
/// itself being switched off — see [`Headset::open_simulated_disconnected`].
pub const SIMULATE_ENV: &str = "QUANTUMENGINE_SIMULATE";

impl Headset {
    /// List every matching dongle.
    pub fn discover() -> Result<Vec<HidrawNode>> {
        Ok(hidraw::discover(VENDOR_ID, PRODUCT_ID)?)
    }

    /// Open the first dongle found, or a simulated headset if
    /// [`SIMULATE_ENV`] is set — handy for development and manual testing
    /// without the real hardware plugged in.
    pub fn open_first() -> Result<Self> {
        if let Ok(mode) = std::env::var(SIMULATE_ENV) {
            return Ok(if mode.eq_ignore_ascii_case("off") {
                Self::open_simulated_disconnected()
            } else {
                Self::open_simulated()
            });
        }
        let node = Self::discover()?.into_iter().next().ok_or(Error::NotFound)?;
        Self::open(&node)
    }

    pub fn open(node: &HidrawNode) -> Result<Self> {
        Ok(Self {
            hid: Box::new(hidraw::Hidraw::open(&node.path)?),
            pending: Mutex::new(VecDeque::new()),
            simulated: false,
        })
    }

    /// A simulated headset that behaves as though connected, for tests and
    /// development without hardware. See the `sim` module docs for how
    /// closely it matches the real device.
    pub fn open_simulated() -> Self {
        Self {
            hid: Box::new(sim::SimTransport::new()),
            pending: Mutex::new(VecDeque::new()),
            simulated: true,
        }
    }

    /// Like [`Headset::open_simulated`], but the simulated headset reads as
    /// switched off — for testing the "dongle present, headset off" path.
    pub fn open_simulated_disconnected() -> Self {
        Self {
            hid: Box::new(sim::SimTransport::new_disconnected()),
            pending: Mutex::new(VecDeque::new()),
            simulated: true,
        }
    }

    /// Whether this is a simulated headset rather than real hardware.
    pub fn is_simulated(&self) -> bool {
        self.simulated
    }

    /// Raw `GET_REPORT(Feature)`, for diagnostics and for reports this crate
    /// does not model. Returns the payload without the report-ID byte, and
    /// fails if the report does not answer.
    pub fn raw_feature(&self, report: u8, len: usize) -> Result<Vec<u8>> {
        Ok(self.hid.get_feature(report, len)?)
    }

    /// Unvalidated read, for probing which reports answer. The first byte of
    /// the returned buffer is whatever report the kernel handed back.
    #[doc(hidden)]
    pub fn raw_feature_debug(&self, report: u8, len: usize) -> Result<(i32, Vec<u8>)> {
        Ok(self.hid.get_feature_debug(report, len)?)
    }

    fn read_u8(&self, report: u8) -> Result<u8> {
        let data = self.hid.get_feature(report, 1)?;
        data.first().copied().ok_or_else(|| {
            Error::Protocol(format!("report 0x{report:02X} returned no data"))
        })
    }

    fn require_connected(&self) -> Result<()> {
        if self.connected()? {
            Ok(())
        } else {
            Err(Error::NotConnected)
        }
    }

    /// Battery level as a percentage.
    ///
    /// Each call makes the device emit an [`Event::Battery`].
    pub fn battery(&self) -> Result<u8> {
        self.read_u8(feature::BATTERY)
    }

    /// Whether the headset itself is switched on and linked to the dongle.
    /// The dongle answers regardless, so this is the headset's state, not the
    /// dongle's.
    pub fn connected(&self) -> Result<bool> {
        Ok(self.read_u8(feature::CONNECTION)? != 0)
    }

    /// Current ANC mode. Mid-way through an On ↔ TalkThru change this reads
    /// Off for a few hundred milliseconds; see the crate docs.
    pub fn anc(&self) -> Result<Anc> {
        // The setter 0x46 does not answer reads; its mirror does.
        let raw = self.read_u8(feature::ANC_MIRROR)?;
        Anc::from_byte(raw).ok_or_else(|| Error::Protocol(format!("ANC value 0x{raw:02X}")))
    }

    /// Write the ANC mode and return immediately.
    pub fn set_anc(&self, mode: Anc) -> Result<()> {
        Ok(self.hid.set_feature(&[feature::ANC, mode.as_byte()])?)
    }

    /// Write the ANC mode and wait until the headset reports it.
    pub fn set_anc_confirmed(&self, mode: Anc, timeout: Duration) -> Result<Anc> {
        self.require_connected()?;
        let before = self.anc()?;
        self.set_anc(mode)?;
        // Writing the current mode produces no event, so there is nothing to
        // wait for. The write still goes out: if an On <-> TalkThru change is
        // in flight the mirror can momentarily read Off, and skipping the write
        // would then leave the headset in the wrong mode.
        if before == mode {
            return Ok(mode);
        }
        if self.wait_for(|e| *e == Event::Anc(mode), timeout)? {
            return Ok(mode);
        }
        match self.anc()? {
            now if now == mode => Ok(mode),
            now => Err(Error::Timeout(format!(
                "ANC still reads {} after asking for {}",
                now.as_str(),
                mode.as_str()
            ))),
        }
    }

    pub fn sidetone(&self) -> Result<Sidetone> {
        // The setter 0x5D does not answer reads; its mirror does.
        let raw = self.read_u8(feature::SIDETONE_MIRROR)?;
        Sidetone::from_byte(raw)
            .ok_or_else(|| Error::Protocol(format!("sidetone value 0x{raw:02X}")))
    }

    /// Write the sidetone level and return immediately.
    ///
    /// The device emits no event for sidetone, so an event-driven client will
    /// not see the change — read it back instead.
    pub fn set_sidetone(&self, level: Sidetone) -> Result<()> {
        Ok(self.hid.set_feature(&[feature::SIDETONE, level.as_byte()])?)
    }

    /// Write the sidetone level and wait until it reads back.
    pub fn set_sidetone_confirmed(&self, level: Sidetone, timeout: Duration) -> Result<Sidetone> {
        self.require_connected()?;
        self.set_sidetone(level)?;
        // Measured as correct on the first read; poll briefly in case not.
        let deadline = Instant::now() + timeout;
        loop {
            let now = self.sidetone()?;
            if now == level {
                return Ok(level);
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout(format!(
                    "sidetone still reads {} after asking for {}",
                    now.as_str(),
                    level.as_str()
                )));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Whether the microphone is live: boom down *and* not muted.
    pub fn mic_active(&self) -> Result<bool> {
        Ok(self.read_u8(feature::MIC_ACTIVE)? != 0)
    }

    /// Game/chat dial position, 0-16.
    pub fn dial(&self) -> Result<u8> {
        self.read_u8(feature::DIAL)
    }

    /// Whether the lights are on.
    ///
    /// May make the device emit an [`Event::Lights`]; see the crate docs.
    pub fn lights_on(&self) -> Result<bool> {
        // The setter 0x4B does not answer reads; its mirror does.
        Ok(self.read_u8(feature::LIGHTS_MIRROR)? != 0)
    }

    /// Switch the lights on or off and return immediately.
    pub fn set_lights(&self, on: bool) -> Result<()> {
        Ok(self.hid.set_feature(&[feature::LIGHTS, u8::from(on)])?)
    }

    /// Switch the lights on or off and wait until the headset reports it.
    ///
    /// The device ignores a lights write that arrives roughly 50–100 ms after
    /// the previous lights write was acknowledged (measured: 17 of 120 writes
    /// in a randomized test, all inside that window). Such a write is
    /// acknowledged with no event, so this re-sends when the event is late.
    /// Re-sending is harmless: every lights write is acknowledged, changed or
    /// not.
    pub fn set_lights_confirmed(&self, on: bool, timeout: Duration) -> Result<bool> {
        self.require_connected()?;
        let deadline = Instant::now() + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            self.set_lights(on)?;
            if self.wait_for(|e| *e == Event::Lights(on), left.min(LIGHTS_RESEND_AFTER))? {
                return Ok(on);
            }
        }
        match self.lights_on()? {
            now if now == on => Ok(on),
            _ => Err(Error::Timeout(format!(
                "lights did not switch {}",
                if on { "on" } else { "off" }
            ))),
        }
    }

    /// The name the headset has for the paired host, e.g. `PC-myhost #1`.
    pub fn device_name(&self) -> Result<String> {
        let data = self.hid.get_feature(feature::DEVICE_NAME, 63)?;
        Ok(parse_device_name(&data))
    }

    pub fn serial(&self) -> Result<String> {
        let data = self.hid.get_feature(feature::SERIAL, 16)?;
        Ok(String::from_utf8_lossy(&data).trim_end_matches('\0').trim().to_string())
    }

    /// Firmware versions, as four `major.minor.patch` strings.
    pub fn firmware(&self) -> Result<Vec<String>> {
        let data = self.hid.get_feature(feature::FIRMWARE, 12)?;
        Ok(data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| format!("{}.{}.{}", c[0], c[1], c[2]))
            .collect())
    }

    /// Read everything in one pass.
    ///
    /// Reads the battery and the lights state, so it makes the device emit a
    /// battery event and possibly a lights event.
    pub fn status(&self) -> Result<Status> {
        Ok(Status {
            connected: self.connected()?,
            battery_percent: self.battery()?,
            anc: self.anc()?,
            sidetone: self.sidetone()?,
            mic_active: self.mic_active()?,
            dial: self.dial()?,
            lights_on: self.lights_on()?,
            serial: self.serial()?,
            firmware: self.firmware()?,
            device_name: self.device_name()?,
        })
    }

    /// Configure one lighting zone and switch the lights on.
    ///
    /// The configuration latches when the lights are switched on — writing it
    /// while they are already on has no visible effect — so this switches off,
    /// writes, then switches on. Both zones are affected by the off/on. The
    /// off and on switches are confirmed, and the configuration is written
    /// only once the device is past the window in which it ignores writes, so
    /// this takes a few hundred milliseconds.
    ///
    /// `palette` must hold 1..=[`MAX_SLOTS`] colours; the device animates
    /// through them. A single colour gives a solid (still animated) zone —
    /// there is no separate static mode. The configuration cannot be read back.
    pub fn set_lighting(
        &self,
        zone: Zone,
        brightness: u8,
        palette: &[Colour],
        effect: Effect,
    ) -> Result<()> {
        self.set_lighting_zones(&[ZoneLighting {
            zone,
            brightness,
            palette: palette.to_vec(),
            effect,
        }])
    }

    /// Like [`Headset::set_lighting`] for several zones at once, with a single
    /// off/on cycle instead of one per zone.
    pub fn set_lighting_zones(&self, zones: &[ZoneLighting]) -> Result<()> {
        for z in zones {
            validate_lighting(z.brightness, &z.palette)?;
        }
        self.set_lights_confirmed(false, CONFIRM_TIMEOUT)?;
        std::thread::sleep(LIGHTS_SETTLE);
        for z in zones {
            // The slot count must match what is written, or the device
            // animates through uninitialised slots.
            self.hid.set_feature(&[
                feature::LIGHT_CONFIG,
                z.zone.as_byte(),
                z.brightness,
                z.palette.len() as u8,
            ])?;
            for (slot, colour) in z.palette.iter().enumerate() {
                self.hid.set_feature(&[
                    feature::LIGHT_COLOUR,
                    z.zone.as_byte(),
                    slot as u8,
                    colour.r,
                    colour.g,
                    colour.b,
                    z.effect.0,
                    (slot as u8) * 2,
                ])?;
            }
        }
        self.set_lights_confirmed(true, CONFIRM_TIMEOUT).map(|_| ())
    }

    /// Block until the device pushes an event.
    ///
    /// Writes are echoed back as events, so a client that reacts to events
    /// must not treat its own writes as external changes.
    pub fn next_event(&self) -> Result<Event> {
        if let Some(e) = self.pending.lock().unwrap().pop_front() {
            return Ok(e);
        }
        Ok(Event::decode(&self.hid.read_report()?))
    }

    /// Like [`Headset::next_event`], but gives up after `timeout`.
    pub fn next_event_timeout(&self, timeout: Duration) -> Result<Option<Event>> {
        if let Some(e) = self.pending.lock().unwrap().pop_front() {
            return Ok(Some(e));
        }
        Ok(self
            .hid
            .read_report_timeout(timeout)?
            .map(|data| Event::decode(&data)))
    }

    /// Wait for a fresh event matching `want`, keeping every other event for
    /// later. Events already pending predate the caller's write, so they are
    /// not considered.
    fn wait_for(&self, want: impl Fn(&Event) -> bool, timeout: Duration) -> Result<bool> {
        let mut stash = Vec::new();
        let found = await_matching(
            timeout,
            |left| {
                Ok(self
                    .hid
                    .read_report_timeout(left)?
                    .map(|data| Event::decode(&data)))
            },
            want,
            &mut stash,
        );
        self.pending.lock().unwrap().extend(stash);
        found
    }
}

/// Pull events from `fetch` until one satisfies `want` or `timeout` passes.
/// Non-matching events are appended to `stash` in arrival order.
fn await_matching(
    timeout: Duration,
    mut fetch: impl FnMut(Duration) -> Result<Option<Event>>,
    want: impl Fn(&Event) -> bool,
    stash: &mut Vec<Event>,
) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match fetch(left)? {
            Some(e) if want(&e) => return Ok(true),
            Some(e) => stash.push(e),
            None => return Ok(false),
        }
    }
    Ok(false)
}

fn validate_lighting(brightness: u8, palette: &[Colour]) -> Result<()> {
    if palette.is_empty() || palette.len() > MAX_SLOTS {
        return Err(Error::Invalid(format!(
            "palette must have 1 to {MAX_SLOTS} colours, got {}",
            palette.len()
        )));
    }
    if brightness > 100 {
        return Err(Error::Invalid(format!(
            "brightness must be 0-100, got {brightness}"
        )));
    }
    Ok(())
}

/// Layout: length byte, one unknown byte, then `length` bytes of ASCII.
/// Anything past the declared length is stale buffer content, not name.
fn parse_device_name(data: &[u8]) -> String {
    let len = data.first().copied().unwrap_or(0) as usize;
    let text = data.get(2..2 + len).unwrap_or(&[]);
    String::from_utf8_lossy(text).trim().to_string()
}

/// An event pushed by the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Anc(Anc),
    /// Bluetooth link became active or inactive.
    Bluetooth(bool),
    MicActive(bool),
    /// Lights state. Also produced by writes that change nothing, and by reads
    /// of the lights state.
    Lights(bool),
    /// Battery percentage. Produced by every read of the battery, not only
    /// when the level changes — never re-read the battery in response.
    Battery(u8),
    /// Game/chat dial position, 0-16.
    Dial(u8),
    /// A report this crate does not decode, kept verbatim.
    Unknown { report_id: u8, data: Vec<u8> },
}

impl Event {
    fn decode(data: &[u8]) -> Self {
        use protocol::event as ev;
        let (Some(&id), Some(&value)) = (data.first(), data.get(1)) else {
            return Self::Unknown {
                report_id: data.first().copied().unwrap_or(0),
                data: data.to_vec(),
            };
        };
        match id {
            ev::ANC => Anc::from_byte(value).map_or_else(
                || Self::Unknown { report_id: id, data: data.to_vec() },
                Self::Anc,
            ),
            ev::BLUETOOTH => Self::Bluetooth(value != 0),
            ev::MIC => Self::MicActive(value != 0),
            ev::LIGHTS => Self::Lights(value != 0),
            ev::BATTERY => Self::Battery(value),
            ev::DIAL => Self::Dial(value),
            _ => Self::Unknown { report_id: id, data: data.to_vec() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_known_events() {
        assert_eq!(Event::decode(&[0x02, 0x02]), Event::Anc(Anc::TalkThru));
        assert_eq!(Event::decode(&[0x03, 0x01]), Event::Bluetooth(true));
        assert_eq!(Event::decode(&[0x06, 0x00]), Event::MicActive(false));
        assert_eq!(Event::decode(&[0x07, 0x01]), Event::Lights(true));
        assert_eq!(Event::decode(&[0x08, 0x28]), Event::Battery(40));
        assert_eq!(Event::decode(&[0x10, 0x10]), Event::Dial(16));
    }

    #[test]
    fn keeps_undecodable_events_verbatim() {
        // 0x09 appears in the connect burst but its meaning is unknown; it must
        // survive rather than be silently dropped.
        assert_eq!(
            Event::decode(&[0x09, 0x01]),
            Event::Unknown { report_id: 0x09, data: vec![0x09, 0x01] }
        );
        // An out-of-range ANC value is not forced into the enum.
        assert!(matches!(
            Event::decode(&[0x02, 0x7F]),
            Event::Unknown { report_id: 0x02, .. }
        ));
        // A truncated report does not panic.
        assert!(matches!(Event::decode(&[0x02]), Event::Unknown { .. }));
        assert!(matches!(Event::decode(&[]), Event::Unknown { .. }));
    }

    /// A scripted event source standing in for the device.
    fn script(events: Vec<Event>) -> impl FnMut(Duration) -> Result<Option<Event>> {
        let mut queue = VecDeque::from(events);
        move |_| Ok(queue.pop_front())
    }

    #[test]
    fn waiting_finds_the_matching_event_and_keeps_the_rest() {
        // The On <-> TalkThru path: a battery event and a dial move arrive
        // before the ANC confirmation.
        let mut stash = Vec::new();
        let found = await_matching(
            Duration::from_secs(1),
            script(vec![Event::Battery(50), Event::Dial(8), Event::Anc(Anc::TalkThru)]),
            |e| *e == Event::Anc(Anc::TalkThru),
            &mut stash,
        )
        .unwrap();
        assert!(found);
        assert_eq!(stash, vec![Event::Battery(50), Event::Dial(8)]);
    }

    #[test]
    fn waiting_does_not_accept_a_different_value() {
        // A stale ANC event for another mode must not count as confirmation.
        let mut stash = Vec::new();
        let found = await_matching(
            Duration::from_secs(1),
            script(vec![Event::Anc(Anc::Off)]),
            |e| *e == Event::Anc(Anc::On),
            &mut stash,
        )
        .unwrap();
        assert!(!found);
        assert_eq!(stash, vec![Event::Anc(Anc::Off)]);
    }

    #[test]
    fn waiting_gives_up_when_the_source_times_out() {
        let mut stash = Vec::new();
        let found = await_matching(
            Duration::from_secs(1),
            script(vec![]),
            |_| true,
            &mut stash,
        )
        .unwrap();
        assert!(!found);
        assert!(stash.is_empty());
    }

    #[test]
    fn waiting_respects_an_already_expired_deadline() {
        let mut calls = 0;
        let found = await_matching(
            Duration::ZERO,
            |_| {
                calls += 1;
                Ok(Some(Event::Battery(1)))
            },
            |_| true,
            &mut Vec::new(),
        )
        .unwrap();
        assert!(!found);
        assert_eq!(calls, 0);
    }

    #[test]
    fn device_name_honours_the_length_prefix() {
        // Shaped like a real capture: the name, then the tail of the serial number
        // left over in the device's buffer.
        let mut data = vec![0x0c, 0x00];
        data.extend_from_slice(b"PC-myhost #1123");
        data.resize(63, 0);
        assert_eq!(parse_device_name(&data), "PC-myhost #1");
    }

    #[test]
    fn device_name_survives_a_bad_length() {
        let data = vec![0x40, 0x00, b'a', b'b'];
        assert_eq!(parse_device_name(&data), "");
        assert_eq!(parse_device_name(&[]), "");
    }

    #[test]
    fn lighting_validation() {
        let red = Colour::new(0xff, 0, 0);
        assert!(validate_lighting(100, &[red]).is_ok());
        assert!(validate_lighting(0, &[red; MAX_SLOTS]).is_ok());
        assert!(validate_lighting(101, &[red]).is_err());
        assert!(validate_lighting(50, &[]).is_err());
        assert!(validate_lighting(50, &[red; MAX_SLOTS + 1]).is_err());
    }

    #[test]
    fn headset_can_move_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Headset>();
    }
}
