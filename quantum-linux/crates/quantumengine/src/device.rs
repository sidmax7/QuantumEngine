//! The worker thread that owns the headset.
//!
//! The UI never touches the device. It sends [`Command`]s and draws the
//! [`Snapshot`] this thread keeps up to date. The confirmed setters block for up
//! to a couple of seconds, which must not happen on the UI thread.
//!
//! State comes from three places:
//! * a full read when the headset (re)connects;
//! * device events, which cover the physical controls and our own writes;
//! * a slow poll of the connection and sidetone, which have no events.
//!
//! Per the library docs, nothing is ever re-read in response to an event:
//! reading the battery or the lights state makes the device send another one.

use jbl_quantum::{Anc, Error, Event, Headset, Sidetone, ZoneLighting, CONFIRM_TIMEOUT};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often to check the connection and sidetone.
const POLL_EVERY: Duration = Duration::from_secs(2);
/// How often to retry opening a dongle that is missing or inaccessible.
const REOPEN_EVERY: Duration = Duration::from_secs(1);
/// Longest time a command waits before it is picked up.
const EVENT_WAIT: Duration = Duration::from_millis(50);

pub enum Command {
    Anc(Anc),
    Sidetone(Sidetone),
    Lights(bool),
    Lighting(Vec<ZoneLighting>),
}

/// What the dongle and headset are doing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Link {
    /// No dongle plugged in.
    #[default]
    NoDongle,
    /// A dongle is plugged in but cannot be opened.
    NoAccess(String),
    /// The dongle is open; the headset is off or out of range.
    HeadsetOff,
    Ready,
}

/// A write in progress, so the UI can show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    Anc(Anc),
    Sidetone(Sidetone),
    Lights(bool),
    Lighting,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub link: Link,
    pub battery: Option<u8>,
    pub anc: Option<Anc>,
    pub sidetone: Option<Sidetone>,
    pub mic_active: Option<bool>,
    pub dial: Option<u8>,
    pub lights_on: Option<bool>,
    pub device_name: String,
    pub serial: String,
    pub firmware: Vec<String>,
    pub pending: Option<Pending>,
    /// The last failure, shown until the next command succeeds.
    pub error: Option<String>,
}

pub type Shared = Arc<Mutex<Snapshot>>;

/// Runs until the command channel closes (the window was closed).
pub fn run(shared: Shared, commands: Receiver<Command>, repaint: impl Fn()) {
    let worker = Worker { shared, commands, repaint };
    loop {
        match Headset::open_first() {
            Ok(headset) => match worker.serve(&headset) {
                Flow::Exit => return,
                Flow::Lost(e) => worker.update(|s| {
                    *s = Snapshot { error: Some(format!("lost the dongle: {e}")), ..Snapshot::default() }
                }),
            },
            Err(e) => {
                let link = match e {
                    Error::Access(_) => Link::NoAccess(e.to_string()),
                    _ => Link::NoDongle,
                };
                worker.update(|s| {
                    if s.link != link {
                        *s = Snapshot { link: link.clone(), ..Snapshot::default() };
                    }
                });
            }
        }
        // Wait before retrying, but keep answering commands so the UI is not
        // left with a write that never completes.
        let until = Instant::now() + REOPEN_EVERY;
        while Instant::now() < until {
            match worker.commands.recv_timeout(EVENT_WAIT) {
                Ok(_) => worker.update(|s| s.error = Some("no headset dongle is available".into())),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

enum Flow {
    Exit,
    Lost(Error),
}

struct Worker<F: Fn()> {
    shared: Shared,
    commands: Receiver<Command>,
    repaint: F,
}

impl<F: Fn()> Worker<F> {
    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut self.shared.lock().unwrap());
        (self.repaint)();
    }

    fn serve(&self, h: &Headset) -> Flow {
        match self.serve_inner(h) {
            Ok(()) => Flow::Exit,
            Err(e) => Flow::Lost(e),
        }
    }

    /// Returns `Ok` when the UI has gone away and `Err` when the dongle has.
    fn serve_inner(&self, h: &Headset) -> Result<(), Error> {
        let mut connected = h.connected()?;
        self.refresh(h, connected)?;
        let mut next_poll = Instant::now() + POLL_EVERY;
        loop {
            loop {
                match self.commands.try_recv() {
                    Ok(cmd) => self.execute(h, cmd)?,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }

            if let Some(event) = h.next_event_timeout(EVENT_WAIT)? {
                self.update(|s| apply_event(s, event));
            }

            if Instant::now() >= next_poll {
                next_poll = Instant::now() + POLL_EVERY;
                let now = h.connected()?;
                if now != connected {
                    connected = now;
                    self.refresh(h, connected)?;
                } else if connected {
                    // Sidetone has no event. Reading it has no side effects.
                    let sidetone = h.sidetone()?;
                    self.update(|s| s.sidetone = Some(sidetone));
                }
            }
        }
    }

    /// Read everything. With the headset off, the dongle still answers with
    /// its last known values, which would look live, so only the identity
    /// fields are kept then.
    fn refresh(&self, h: &Headset, connected: bool) -> Result<(), Error> {
        if connected {
            let st = h.status()?;
            self.update(|s| {
                s.link = Link::Ready;
                s.battery = Some(st.battery_percent);
                s.anc = Some(st.anc);
                s.sidetone = Some(st.sidetone);
                s.mic_active = Some(st.mic_active);
                s.dial = Some(st.dial);
                s.lights_on = Some(st.lights_on);
                s.device_name = st.device_name;
                s.serial = st.serial;
                s.firmware = st.firmware;
            });
        } else {
            let (serial, firmware) = (h.serial()?, h.firmware()?);
            self.update(|s| {
                *s = Snapshot {
                    link: Link::HeadsetOff,
                    serial,
                    firmware,
                    error: s.error.take(),
                    ..Snapshot::default()
                }
            });
        }
        Ok(())
    }

    /// Run one command. Device-level failures other than a lost dongle are
    /// shown to the user and do not end the session.
    fn execute(&self, h: &Headset, cmd: Command) -> Result<(), Error> {
        let pending = match &cmd {
            Command::Anc(m) => Pending::Anc(*m),
            Command::Sidetone(l) => Pending::Sidetone(*l),
            Command::Lights(on) => Pending::Lights(*on),
            Command::Lighting(_) => Pending::Lighting,
        };
        self.update(|s| s.pending = Some(pending));

        let result = match cmd {
            Command::Anc(m) => h
                .set_anc_confirmed(m, CONFIRM_TIMEOUT)
                .map(|m| self.update(|s| s.anc = Some(m))),
            Command::Sidetone(l) => h
                .set_sidetone_confirmed(l, CONFIRM_TIMEOUT)
                .map(|l| self.update(|s| s.sidetone = Some(l))),
            Command::Lights(on) => h
                .set_lights_confirmed(on, CONFIRM_TIMEOUT)
                .map(|on| self.update(|s| s.lights_on = Some(on))),
            Command::Lighting(zones) => h
                .set_lighting_zones(&zones)
                .map(|()| self.update(|s| s.lights_on = Some(true))),
        };

        match result {
            Ok(()) => {
                self.update(|s| {
                    s.pending = None;
                    s.error = None;
                });
                Ok(())
            }
            Err(Error::Io(e)) => {
                self.update(|s| s.pending = None);
                Err(Error::Io(e))
            }
            Err(e) => {
                self.update(|s| {
                    s.pending = None;
                    s.error = Some(e.to_string());
                });
                Ok(())
            }
        }
    }
}

fn apply_event(s: &mut Snapshot, event: Event) {
    match event {
        Event::Anc(m) => s.anc = Some(m),
        Event::MicActive(on) => s.mic_active = Some(on),
        Event::Lights(on) => s.lights_on = Some(on),
        Event::Battery(pct) => s.battery = Some(pct),
        Event::Dial(pos) => s.dial = Some(pos),
        Event::Bluetooth(_) | Event::Unknown { .. } => {}
    }
}
