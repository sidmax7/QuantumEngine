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

use crate::settings;
use jbl_quantum::{Anc, Error, Event, Headset, HidrawNode, Sidetone, ZoneLighting, CONFIRM_TIMEOUT};
use std::cell::{Cell, RefCell};
use std::io;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often to check the connection and sidetone.
const POLL_EVERY: Duration = Duration::from_secs(2);
/// How often to retry opening a dongle that is missing or inaccessible.
const REOPEN_EVERY: Duration = Duration::from_secs(1);
/// Longest time a command waits before it is picked up.
const EVENT_WAIT: Duration = Duration::from_millis(50);

/// Where a command's outcome is delivered. The window only watches
/// [`Snapshot`] and passes [`no_reply`]; the D-Bus service (`dbus_service`)
/// blocks on one of these to answer its own callers.
pub type Reply<T> = Sender<Result<T, Error>>;

/// A reply channel nobody is listening to.
pub fn no_reply<T>() -> Reply<T> {
    mpsc::channel().0
}

pub enum Command {
    Anc(Anc, Reply<Anc>),
    Sidetone(Sidetone, Reply<Sidetone>),
    Lights(bool, Reply<bool>),
    Lighting(Vec<ZoneLighting>, Reply<()>),
    /// Reads what an unverified model answers to, as a report to paste into an
    /// issue. Reads only.
    Probe(Reply<String>),
    /// Whether to use the unverified model, and which writes to allow.
    SetUnverified(bool, Vec<String>, Reply<()>),
}

impl Command {
    /// Send `err` to whichever reply channel this command carries, ignoring a
    /// receiver that has already gone away.
    pub(crate) fn fail(self, err: Error) {
        match self {
            Command::Anc(_, r) => drop(r.send(Err(err))),
            Command::Sidetone(_, r) => drop(r.send(Err(err))),
            Command::Lights(_, r) => drop(r.send(Err(err))),
            Command::Lighting(_, r) => drop(r.send(Err(err))),
            Command::Probe(r) => drop(r.send(Err(err))),
            Command::SetUnverified(_, _, r) => drop(r.send(Err(err))),
        }
    }
}

/// What the dongle and headset are doing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Link {
    /// No dongle plugged in.
    #[default]
    NoDongle,
    /// A dongle is plugged in but cannot be opened.
    NoAccess(String),
    /// A dongle from the same vendor is plugged in, but it isn't a model this
    /// software is verified on, and the user hasn't chosen to use it.
    Unverified,
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

/// A device from the same vendor that isn't verified.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unverified {
    /// `vendor:product`.
    pub id: String,
    pub name: String,
    /// Why it can't be used yet, or empty.
    pub access: String,
    pub in_use: bool,
    /// Any of `anc`, `sidetone`, `lights`.
    pub allowed: Vec<String>,
    /// Not a headset (a microphone or earbuds): it is shown, never used.
    pub not_a_headset: bool,
}

/// Status of a request for access to the unverified device, shown until the
/// next one. Held apart from the snapshot, which is reset whenever the dongle
/// is lost.
static GRANT_STATUS: Mutex<String> = Mutex::new(String::new());

pub fn grant_status() -> String {
    GRANT_STATUS.lock().unwrap().clone()
}

pub fn set_grant_status(status: impl Into<String>) {
    *GRANT_STATUS.lock().unwrap() = status.into();
}

/// The writes an unverified device can be allowed.
pub const FEATURES: [&str; 3] = ["anc", "sidetone", "lights"];

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
    /// An unverified model that is plugged in, if any.
    pub unverified: Option<Unverified>,
    /// `vendor:product` of the device being served, e.g. `0ecb:2069`, or empty.
    pub model_id: String,
}

pub type Shared = Arc<Mutex<Snapshot>>;

/// Runs until the command channel closes (the window was closed).
pub fn run(shared: Shared, commands: Receiver<Command>, repaint: impl Fn()) {
    let worker = Worker {
        shared,
        commands,
        repaint,
        loose: Cell::new(false),
        reselect: Cell::new(false),
        nodes: RefCell::new(Vec::new()),
    };
    loop {
        match Headset::open_first() {
            Ok(headset) => {
                worker.loose.set(false);
                worker.update(|s| {
                    s.unverified = None;
                    s.model_id = format!("{:04x}:{:04x}", jbl_quantum::VENDOR_ID, jbl_quantum::PRODUCT_ID);
                });
                match worker.serve(&headset) {
                    Flow::Exit => return,
                    Flow::Reselect => continue,
                    Flow::Lost(e) => worker.update(|s| {
                        *s = Snapshot { error: Some(format!("lost the dongle: {e}")), ..Snapshot::default() }
                    }),
                }
            }
            Err(Error::NotFound) => match find_unverified() {
                Some(found) => match worker.unverified(found) {
                    Flow::Exit => return,
                    Flow::Reselect => continue,
                    Flow::Lost(e) => worker.update(|s| {
                        *s = Snapshot { error: Some(format!("lost the dongle: {e}")), ..Snapshot::default() }
                    }),
                },
                None => worker.update(|s| {
                    if s.link != Link::NoDongle || s.unverified.is_some() {
                        *s = Snapshot { link: Link::NoDongle, ..Snapshot::default() };
                    }
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
        worker.wait_for_retry();
    }
}

/// An unverified model that is plugged in: the interfaces it shows as.
struct Found {
    id: String,
    name: String,
    nodes: Vec<HidrawNode>,
    /// Known not to be a headset: listed so it can be named, never opened.
    not_a_headset: bool,
}

/// The first unverified model plugged in, with all of its interfaces. A
/// headset (or something not in the catalogue) is preferred over a
/// microphone or earbuds plugged in at the same time.
fn find_unverified() -> Option<Found> {
    let nodes = Headset::discover_unverified().ok()?;
    let headset_first = |n: &HidrawNode| match jbl_quantum::models::lookup(n.vendor_id, n.product_id) {
        Some(m) if m.kind() != jbl_quantum::models::Kind::Headset => 1,
        _ => 0,
    };
    let first = nodes.iter().min_by_key(|n| headset_first(n))?;
    let (vendor, product) = (first.vendor_id, first.product_id);
    let model = jbl_quantum::models::lookup(vendor, product);
    let nodes: Vec<_> = nodes.into_iter().filter(|n| n.product_id == product).collect();
    Some(Found {
        id: format!("{vendor:04x}:{product:04x}"),
        name: model.map_or_else(|| nodes[0].name.clone(), |m| m.full_name()),
        not_a_headset: model.is_some_and(|m| m.kind() != jbl_quantum::models::Kind::Headset),
        nodes,
    })
}

/// Opens the interface of an unverified model that answers the headset-link
/// read, which is the least that makes it this protocol's control interface.
/// Only reads are made. The error says what to tell the user.
fn open_best(nodes: &[HidrawNode]) -> Result<Headset, String> {
    let mut denied = None;
    for node in nodes {
        match Headset::open(node) {
            Ok(h) => {
                let answers = h.probe().iter().any(|r| r.report == jbl_quantum::protocol::feature::CONNECTION && r.plausible());
                if answers {
                    return Ok(h);
                }
            }
            Err(Error::Access(e)) => denied = Some(format!("cannot open {}: {e}", node.path.display())),
            Err(_) => {}
        }
    }
    Err(denied.unwrap_or_else(|| "none of the device's interfaces answered the way this software expects".into()))
}

/// The probe report for every interface of `nodes`, as one text.
fn probe_all(nodes: &[HidrawNode]) -> String {
    nodes
        .iter()
        .map(|n| Headset::probe_node(n).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

enum Flow {
    Exit,
    /// The choice of device changed: look again.
    Reselect,
    Lost(Error),
}

struct Worker<F: Fn()> {
    shared: Shared,
    commands: Receiver<Command>,
    repaint: F,
    /// Serving an unverified model: it may not answer every read, and writes
    /// need the user's permission, one feature at a time.
    loose: Cell<bool>,
    /// Set when the user changed whether to use the unverified model.
    reselect: Cell<bool>,
    /// The unverified model's interfaces, for probing.
    nodes: RefCell<Vec<HidrawNode>>,
}

impl<F: Fn()> Worker<F> {
    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut self.shared.lock().unwrap());
        (self.repaint)();
    }

    fn serve(&self, h: &Headset) -> Flow {
        match self.serve_inner(h) {
            Ok(flow) => flow,
            Err(e) => Flow::Lost(e),
        }
    }

    /// Waits a moment before looking for the dongle again, answering commands
    /// meanwhile.
    fn wait_for_retry(&self) {
        let until = Instant::now() + REOPEN_EVERY;
        while Instant::now() < until {
            match self.commands.recv_timeout(EVENT_WAIT) {
                Ok(cmd) => {
                    self.update(|s| s.error = Some("no headset dongle is available".into()));
                    cmd.fail(Error::NotFound);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// An unverified model is plugged in. Shows it; if the user chose to use
    /// it, opens it and serves it (reads freely, writes only as allowed),
    /// otherwise just waits and answers probes.
    fn unverified(&self, found: Found) -> Flow {
        let choice = settings::load().unverified.get(&found.id).cloned().unwrap_or_default();
        *self.nodes.borrow_mut() = found.nodes.clone();
        let opened = if choice.in_use && !found.not_a_headset { Some(open_best(&found.nodes)) } else { None };
        let access = match &opened {
            Some(Err(why)) => why.clone(),
            _ => String::new(),
        };
        let want = Unverified {
            id: found.id.clone(),
            name: found.name.clone(),
            access,
            in_use: choice.in_use && !found.not_a_headset,
            allowed: choice.allowed.clone(),
            not_a_headset: found.not_a_headset,
        };
        let serving = matches!(opened, Some(Ok(_)));
        let unchanged = {
            let s = self.shared.lock().unwrap();
            s.link == Link::Unverified && s.unverified.as_ref() == Some(&want)
        };
        if serving || !unchanged {
            self.update(|s| {
                if !serving {
                    *s = Snapshot { link: Link::Unverified, ..Snapshot::default() };
                } else {
                    s.model_id = found.id.clone();
                }
                s.unverified = Some(want);
            });
        }
        if let Some(Ok(headset)) = opened {
            self.loose.set(true);
            self.reselect.set(false);
            let flow = self.serve(&headset);
            self.loose.set(false);
            return flow;
        }

        // Not in use: answer probes and choices, for about a second.
        let until = Instant::now() + REOPEN_EVERY;
        while Instant::now() < until {
            match self.commands.recv_timeout(EVENT_WAIT) {
                Ok(cmd @ (Command::SetUnverified(..) | Command::Probe(_))) if found.not_a_headset => {
                    let why = format!("{} isn't a headset; QuantumEngine only controls headsets", found.name);
                    cmd.fail(Error::Invalid(why));
                }
                Ok(Command::SetUnverified(in_use, allowed, reply)) => {
                    let done = self.choose(in_use, allowed);
                    let flow_change = done.is_ok();
                    drop(reply.send(done));
                    if flow_change {
                        return Flow::Reselect;
                    }
                }
                Ok(Command::Probe(reply)) => drop(reply.send(Ok(probe_all(&found.nodes)))),
                Ok(cmd) => {
                    let why = "this model isn't verified; choose to use it, and allow this feature, under Device";
                    self.update(|s| s.error = Some(why.into()));
                    cmd.fail(Error::Invalid(why.into()));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Flow::Exit,
            }
        }
        Flow::Reselect
    }

    /// Saves what the user chose for the unverified model and shows it.
    fn choose(&self, in_use: bool, allowed: Vec<String>) -> Result<(), Error> {
        let id = self
            .shared
            .lock()
            .unwrap()
            .unverified
            .as_ref()
            .map(|u| u.id.clone())
            .ok_or_else(|| Error::Invalid("no unverified model is plugged in".into()))?;
        if let Some(bad) = allowed.iter().find(|a| !FEATURES.contains(&a.as_str())) {
            return Err(Error::Invalid(format!("unknown feature '{bad}' (want anc, sidetone or lights)")));
        }
        let mut allowed = allowed;
        allowed.sort();
        allowed.dedup();
        let choice = settings::UnverifiedChoice { in_use, allowed: allowed.clone() };
        settings::update(|s| {
            s.unverified.insert(id.clone(), choice);
        })
        .map_err(Error::Invalid)?;
        let changed = self.update_get(|s| {
            let was = s.unverified.as_ref().map(|u| u.in_use);
            if let Some(u) = s.unverified.as_mut() {
                u.in_use = in_use;
                u.allowed = allowed;
            }
            was != Some(in_use)
        });
        if changed {
            self.reselect.set(true);
        }
        Ok(())
    }

    /// Like [`Worker::update`], returning what `f` computed.
    fn update_get<T>(&self, f: impl FnOnce(&mut Snapshot) -> T) -> T {
        let out = f(&mut self.shared.lock().unwrap());
        (self.repaint)();
        out
    }

    /// `Some` when a write to `feature` is refused: an unverified model only
    /// gets the writes the user allowed.
    fn denied(&self, feature: &str) -> Option<Error> {
        if !self.loose.get() {
            return None;
        }
        let allowed = self
            .shared
            .lock()
            .unwrap()
            .unverified
            .as_ref()
            .is_some_and(|u| u.allowed.iter().any(|a| a == feature));
        (!allowed).then(|| Error::Invalid(format!("{feature} isn't allowed on this unverified model; allow it under Device")))
    }

    /// Returns `Flow::Exit` when the UI has gone away, `Flow::Reselect` when
    /// the choice of device changed and `Err` when the dongle has gone.
    fn serve_inner(&self, h: &Headset) -> Result<Flow, Error> {
        let mut connected = h.connected()?;
        self.refresh(h, connected)?;
        let mut next_poll = Instant::now() + POLL_EVERY;
        loop {
            loop {
                match self.commands.try_recv() {
                    Ok(cmd) => self.execute(h, cmd)?,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(Flow::Exit),
                }
            }
            if self.reselect.take() {
                return Ok(Flow::Reselect);
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
                    match h.sidetone() {
                        Ok(sidetone) => self.update(|s| s.sidetone = Some(sidetone)),
                        Err(e) if !self.loose.get() => return Err(e),
                        Err(_) => {}
                    }
                }
            }
        }
    }

    /// Read everything. With the headset off, the dongle still answers with
    /// its last known values, which would look live, so only the identity
    /// fields are kept then.
    fn refresh(&self, h: &Headset, connected: bool) -> Result<(), Error> {
        if connected && self.loose.get() {
            // An unverified model need not answer everything; keep what it does.
            // A wired one has no battery or noise control to ask about.
            let wireless = !h.is_wired();
            let battery = wireless.then(|| h.battery().ok()).flatten();
            let anc = wireless.then(|| h.anc().ok()).flatten();
            let sidetone = h.sidetone().ok();
            let (mic_active, dial, lights_on) = (h.mic_active().ok(), h.dial().ok(), h.lights_on().ok());
            let (serial, firmware, device_name) = (h.serial().ok(), h.firmware().ok(), h.device_name().ok());
            let silent: Vec<&str> = [
                ("battery", wireless && battery.is_none()),
                ("noise control", wireless && anc.is_none()),
                ("sidetone", sidetone.is_none()),
                ("microphone", mic_active.is_none()),
                ("dial", dial.is_none()),
                ("lights", lights_on.is_none()),
                ("serial", serial.is_none()),
                ("firmware", firmware.is_none()),
            ]
            .into_iter()
            .filter_map(|(name, missing)| missing.then_some(name))
            .collect();
            self.update(|s| {
                s.link = Link::Ready;
                s.battery = battery;
                s.anc = anc;
                s.sidetone = sidetone;
                s.mic_active = mic_active;
                s.dial = dial;
                s.lights_on = lights_on;
                s.device_name = device_name.unwrap_or_default();
                s.serial = serial.unwrap_or_default();
                s.firmware = firmware.unwrap_or_default();
                s.error = (!silent.is_empty()).then(|| format!("This model didn't answer: {}.", silent.join(", ")));
            });
        } else if connected {
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
                    unverified: s.unverified.take(),
                    model_id: std::mem::take(&mut s.model_id),
                    ..Snapshot::default()
                }
            });
        }
        Ok(())
    }

    /// Run one command and deliver its outcome to the caller's reply channel.
    /// Device-level failures other than a lost dongle are shown to the user
    /// and do not end the session.
    fn execute(&self, h: &Headset, cmd: Command) -> Result<(), Error> {
        let feature = match &cmd {
            Command::Anc(..) => Some("anc"),
            Command::Sidetone(..) => Some("sidetone"),
            Command::Lights(..) | Command::Lighting(..) => Some("lights"),
            Command::Probe(_) | Command::SetUnverified(..) => None,
        };
        if let Some(why) = feature.and_then(|f| self.denied(f)) {
            self.update(|s| s.error = Some(why.to_string()));
            cmd.fail(why);
            return Ok(());
        }
        match cmd {
            Command::Probe(reply) => {
                let text = if self.loose.get() {
                    Ok(probe_all(&self.nodes.borrow()))
                } else {
                    Err(Error::Invalid("this model is verified; there is nothing to probe".into()))
                };
                drop(reply.send(text));
                Ok(())
            }
            Command::SetUnverified(in_use, allowed, reply) => {
                let done = if self.loose.get() {
                    self.choose(in_use, allowed)
                } else {
                    Err(Error::Invalid("this model is verified; there is nothing to choose".into()))
                };
                drop(reply.send(done));
                Ok(())
            }
            Command::Anc(m, reply) => {
                self.update(|s| s.pending = Some(Pending::Anc(m)));
                self.finish(reply, h.set_anc_confirmed(m, CONFIRM_TIMEOUT), |s, m| s.anc = Some(m))
            }
            Command::Sidetone(l, reply) => {
                self.update(|s| s.pending = Some(Pending::Sidetone(l)));
                self.finish(reply, h.set_sidetone_confirmed(l, CONFIRM_TIMEOUT), |s, l| s.sidetone = Some(l))
            }
            Command::Lights(on, reply) => {
                self.update(|s| s.pending = Some(Pending::Lights(on)));
                self.finish(reply, h.set_lights_confirmed(on, CONFIRM_TIMEOUT), |s, on| s.lights_on = Some(on))
            }
            Command::Lighting(zones, reply) => {
                self.update(|s| s.pending = Some(Pending::Lighting));
                self.finish(reply, h.set_lighting_zones(&zones), |s, ()| s.lights_on = Some(true))
            }
        }
    }

    /// Apply one command's result to the snapshot, answer its reply channel,
    /// and return `Err` only for a lost-dongle failure — that one ends
    /// `serve_inner` so `run` can go back to reconnecting.
    fn finish<T: Copy>(
        &self,
        reply: Reply<T>,
        result: Result<T, Error>,
        apply: impl FnOnce(&mut Snapshot, T),
    ) -> Result<(), Error> {
        match result {
            Ok(value) => {
                self.update(|s| {
                    apply(s, value);
                    s.pending = None;
                    s.error = None;
                });
                drop(reply.send(Ok(value)));
                Ok(())
            }
            Err(Error::Io(e)) => {
                self.update(|s| s.pending = None);
                // A distinct `io::Error` carrying the same kind and message,
                // since `Error` cannot be cloned to hand one copy to the
                // reply channel and another to `serve_inner`.
                drop(reply.send(Err(Error::Io(io::Error::new(e.kind(), e.to_string())))));
                Err(Error::Io(e))
            }
            Err(e) => {
                self.update(|s| {
                    s.pending = None;
                    s.error = Some(e.to_string());
                });
                drop(reply.send(Err(e)));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn worker(commands: Receiver<Command>, shared: Shared, loose: bool) -> Worker<impl Fn()> {
        Worker {
            shared,
            commands,
            repaint: || {},
            loose: Cell::new(loose),
            reselect: Cell::new(false),
            nodes: RefCell::new(Vec::new()),
        }
    }

    fn allowing(allowed: &[&str]) -> Shared {
        let shared = Shared::default();
        shared.lock().unwrap().unverified = Some(Unverified {
            id: "0ecb:203e".into(),
            name: "test".into(),
            access: String::new(),
            in_use: true,
            allowed: allowed.iter().map(|a| a.to_string()).collect(),
            not_a_headset: false,
        });
        shared
    }

    #[test]
    fn a_verified_model_is_never_gated() {
        let (_tx, rx) = channel();
        let w = worker(rx, Shared::default(), false);
        assert!(w.denied("anc").is_none());
        assert!(w.denied("lights").is_none());
    }

    #[test]
    fn an_unverified_model_only_gets_the_writes_that_were_allowed() {
        let (_tx, rx) = channel();
        let w = worker(rx, allowing(&["anc"]), true);
        assert!(w.denied("anc").is_none());
        assert!(w.denied("sidetone").is_some());
        assert!(w.denied("lights").is_some());
    }

    #[test]
    fn an_unverified_model_with_nothing_allowed_is_read_only() {
        let (_tx, rx) = channel();
        let w = worker(rx, allowing(&[]), true);
        for f in FEATURES {
            assert!(w.denied(f).is_some(), "{f}");
        }
    }

    /// End to end on the simulated headset: a refused write never reaches it,
    /// and the same write goes through once it is allowed.
    #[test]
    fn a_refused_write_is_not_sent_and_an_allowed_one_is() {
        let shared = allowing(&[]);
        let (tx, rx) = channel();
        let shared_for_worker = shared.clone();
        let thread = std::thread::spawn(move || {
            let w = worker(rx, shared_for_worker, true);
            let h = Headset::open_simulated();
            matches!(w.serve(&h), Flow::Exit)
        });

        let (reply, answer) = mpsc::channel();
        tx.send(Command::Anc(Anc::On, reply)).unwrap();
        let refused = answer.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(refused, Err(Error::Invalid(_))));
        assert_ne!(shared.lock().unwrap().anc, Some(Anc::On));

        shared.lock().unwrap().unverified.as_mut().unwrap().allowed = vec!["anc".into()];
        let (reply, answer) = mpsc::channel();
        tx.send(Command::Anc(Anc::On, reply)).unwrap();
        assert!(matches!(answer.recv_timeout(Duration::from_secs(5)).unwrap(), Ok(Anc::On)));
        assert_eq!(shared.lock().unwrap().anc, Some(Anc::On));

        drop(tx);
        assert!(thread.join().unwrap());
    }

    #[test]
    fn probing_a_verified_model_says_there_is_nothing_to_do() {
        let (tx, rx) = channel();
        let shared = Shared::default();
        let thread = std::thread::spawn(move || {
            let w = worker(rx, shared, false);
            let h = Headset::open_simulated();
            matches!(w.serve(&h), Flow::Exit)
        });
        let (reply, answer) = mpsc::channel();
        tx.send(Command::Probe(reply)).unwrap();
        assert!(matches!(answer.recv_timeout(Duration::from_secs(5)).unwrap(), Err(Error::Invalid(_))));
        drop(tx);
        assert!(thread.join().unwrap());
    }

    /// An unverified model that the user hasn't chosen to use: it is shown,
    /// probing works (and says why when the device can't be opened), and
    /// everything else is refused without touching the device.
    #[test]
    fn an_unverified_model_not_in_use_only_answers_probes() {
        let shared = Shared::default();
        let (tx, rx) = channel();
        let shared_for_worker = shared.clone();
        let thread = std::thread::spawn(move || {
            let w = worker(rx, shared_for_worker, false);
            let found = Found {
                // An ID no real settings file mentions, and a node that can't be opened.
                id: "0ecb:ffff".into(),
                name: "Test model".into(),
                not_a_headset: false,
                nodes: vec![HidrawNode {
                    path: "/dev/quantumengine-test-missing".into(),
                    vendor_id: 0x0ecb,
                    product_id: 0xffff,
                    name: "Test model".into(),
                }],
            };
            loop {
                let again = Found { id: found.id.clone(), name: found.name.clone(), nodes: found.nodes.clone(), not_a_headset: found.not_a_headset };
                if matches!(w.unverified(again), Flow::Exit) {
                    return;
                }
            }
        });

        let (reply, answer) = mpsc::channel();
        tx.send(Command::Probe(reply)).unwrap();
        let text = answer.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        assert!(text.contains("0ecb:ffff"), "{text}");
        assert!(text.contains("Could not open"), "{text}");
        assert!(text.contains("Verified: no"), "{text}");

        let (reply, answer) = mpsc::channel();
        tx.send(Command::Anc(Anc::On, reply)).unwrap();
        assert!(matches!(answer.recv_timeout(Duration::from_secs(5)).unwrap(), Err(Error::Invalid(m)) if m.contains("isn't verified")));

        let (reply, answer) = mpsc::channel();
        tx.send(Command::SetUnverified(false, vec!["bogus".into()], reply)).unwrap();
        assert!(matches!(answer.recv_timeout(Duration::from_secs(5)).unwrap(), Err(Error::Invalid(m)) if m.contains("unknown feature")));

        {
            let s = shared.lock().unwrap();
            assert_eq!(s.link, Link::Unverified);
            let u = s.unverified.as_ref().unwrap();
            assert_eq!(u.id, "0ecb:ffff");
            assert!(!u.in_use);
            assert!(u.allowed.is_empty());
        }

        drop(tx);
        thread.join().unwrap();
    }

    /// A microphone or earbuds is named, but never probed, used or allowed anything.
    #[test]
    fn a_device_that_is_not_a_headset_is_never_used() {
        let shared = Shared::default();
        let (tx, rx) = channel();
        let shared_for_worker = shared.clone();
        let thread = std::thread::spawn(move || {
            let w = worker(rx, shared_for_worker, false);
            let node = HidrawNode {
                path: "/dev/quantumengine-test-missing".into(),
                vendor_id: 0x0ecb,
                product_id: 0x205f,
                name: "Quantum Stream".into(),
            };
            loop {
                let found = Found {
                    id: "0ecb:205f".into(),
                    name: "JBL Quantum Stream".into(),
                    nodes: vec![node.clone()],
                    not_a_headset: true,
                };
                if matches!(w.unverified(found), Flow::Exit) {
                    return;
                }
            }
        });

        let (reply, answer) = mpsc::channel();
        tx.send(Command::Probe(reply)).unwrap();
        assert!(matches!(answer.recv_timeout(Duration::from_secs(5)).unwrap(), Err(Error::Invalid(m)) if m.contains("isn't a headset")));
        let (reply, answer) = mpsc::channel();
        tx.send(Command::SetUnverified(true, vec!["anc".into()], reply)).unwrap();
        assert!(matches!(answer.recv_timeout(Duration::from_secs(5)).unwrap(), Err(Error::Invalid(m)) if m.contains("isn't a headset")));
        {
            let s = shared.lock().unwrap();
            let u = s.unverified.as_ref().unwrap();
            assert!(u.not_a_headset);
            assert!(!u.in_use);
            assert_eq!(u.name, "JBL Quantum Stream");
            assert!(s.model_id.is_empty());
        }
        drop(tx);
        thread.join().unwrap();
    }
}
