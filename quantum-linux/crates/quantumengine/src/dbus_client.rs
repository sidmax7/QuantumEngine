//! Feeds [`device::Shared`] from the `quantumengine --tray` service over
//! D-Bus, instead of from the headset directly — this is what the window
//! uses. `--tray` remains the only process that ever calls
//! `jbl_quantum::Headset`; everything here only ever talks to it over D-Bus,
//! the same as the tray icon and `quantumenginectl` do.
//!
//! The proxy's property cache is disabled (see [`Worker::connect`]):
//! [`Worker::refresh`] already re-reads every property on a short timer
//! regardless, so a cache kept current via its own `PropertiesChanged`
//! subscription would just be a second, redundant way of doing the same job.

use crate::device::{self, Link, Reply, Shared, Snapshot};
use quantumengine_dbus::HeadsetProxyBlocking;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

/// How often the window checks the (cached) proxy for a fresh snapshot.
const POLL_EVERY: Duration = Duration::from_millis(150);
/// How long to wait for a `quantumengine --tray` this module started itself
/// to claim the bus name.
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait before trying to (re)connect again.
const RECONNECT_EVERY: Duration = Duration::from_secs(1);

/// Runs until the command channel closes (the window was closed).
pub fn run(shared: Shared, commands: Receiver<device::Command>, repaint: impl Fn()) {
    let worker = Worker { shared, commands, repaint };
    loop {
        match worker.connect() {
            Ok(proxy) => worker.serve(&proxy),
            Err(e) => worker.update(|s| {
                *s = Snapshot { error: Some(e), ..Snapshot::default() }
            }),
        }
        if !worker.idle(RECONNECT_EVERY) {
            return; // the window closed
        }
    }
}

struct Worker<F: Fn()> {
    shared: Shared,
    commands: Receiver<device::Command>,
    repaint: F,
}

impl<F: Fn()> Worker<F> {
    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut self.shared.lock().unwrap());
        (self.repaint)();
    }

    /// Waits up to `timeout`, answering any command that arrives (there is
    /// no service to run it) so a caller isn't left hanging. Returns `false`
    /// once the command channel disconnects.
    fn idle(&self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        while Instant::now() < until {
            match self.commands.recv_timeout(Duration::from_millis(50).min(until - Instant::now())) {
                Ok(cmd) => cmd.fail(jbl_quantum::Error::NotFound),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
        true
    }

    /// Connects to the running service, or starts one and waits for it.
    fn connect(&self) -> Result<HeadsetProxyBlocking<'static>, String> {
        let conn = zbus::blocking::Connection::session()
            .map_err(|e| format!("cannot reach the session bus: {e}"))?;
        let bus = zbus::blocking::fdo::DBusProxy::new(&conn)
            .map_err(|e| format!("cannot reach the session bus: {e}"))?;
        let name = || quantumengine_dbus::BUS_NAME.try_into().expect("a valid bus name");
        let running = bus.name_has_owner(name()).unwrap_or(false);
        if !running {
            start_service()?;
            let deadline = Instant::now() + SERVICE_START_TIMEOUT;
            while !bus.name_has_owner(name()).unwrap_or(false) {
                if Instant::now() >= deadline {
                    return Err("quantumengine --tray did not start in time".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        // No property cache: `refresh` below already re-reads everything
        // every `POLL_EVERY` regardless, so a background cache kept current
        // via its own `PropertiesChanged` subscription would just be a
        // second, redundant way of doing the same job.
        HeadsetProxyBlocking::builder(&conn)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .map_err(|e| format!("cannot reach the headset service: {e}"))
    }

    /// Returns once a property read fails (the service went away — a
    /// rejected `Set*` call, by contrast, is just that command's error, so
    /// only `refresh`'s own result decides whether to reconnect) or the
    /// window closed.
    fn serve(&self, proxy: &HeadsetProxyBlocking<'_>) {
        loop {
            loop {
                match self.commands.try_recv() {
                    Ok(cmd) => self.execute(proxy, cmd),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if self.refresh(proxy).is_err() {
                return;
            }
            // A plain sleep, not `idle`: this is just pacing between polls,
            // with a service known to be up, so a command arriving during it
            // must wait for next iteration's `try_recv`, not be discarded as
            // unfulfillable the way `idle` (correctly) does while reconnecting.
            std::thread::sleep(POLL_EVERY);
        }
    }

    /// Reads every property in one go and updates the snapshot.
    fn refresh(&self, proxy: &HeadsetProxyBlocking<'_>) -> zbus::Result<()> {
        let link = proxy.link()?;
        let snap = Snapshot {
            link: parse_link(&link, proxy.link_detail().unwrap_or_default()),
            battery: (link == "ready").then(|| proxy.battery().unwrap_or_default()),
            anc: parse_anc(&proxy.anc().unwrap_or_default()),
            sidetone: parse_sidetone(&proxy.sidetone().unwrap_or_default()),
            mic_active: Some(proxy.mic_active().unwrap_or_default()),
            dial: Some(proxy.dial().unwrap_or_default()),
            lights_on: Some(proxy.lights_on().unwrap_or_default()),
            device_name: proxy.device_name().unwrap_or_default(),
            serial: proxy.serial().unwrap_or_default(),
            firmware: proxy.firmware().unwrap_or_default(),
            // Always None here: `execute` runs commands synchronously and
            // clears `pending` itself before `refresh` ever runs, so there is
            // no in-flight command for this tick to preserve.
            pending: None,
            error: {
                let msg = proxy.last_error().unwrap_or_default();
                (!msg.is_empty()).then_some(msg)
            },
        };
        self.update(|s| *s = snap);
        Ok(())
    }

    /// Runs one command and delivers its outcome.
    fn execute(&self, proxy: &HeadsetProxyBlocking<'_>, cmd: device::Command) {
        match cmd {
            device::Command::Anc(m, reply) => {
                self.update(|s| s.pending = Some(device::Pending::Anc(m)));
                let result = proxy.set_anc(m.as_str()).map(|s| parse_anc(&s).unwrap_or(m));
                self.finish(reply, result, |s, m| s.anc = Some(m));
            }
            device::Command::Sidetone(l, reply) => {
                self.update(|s| s.pending = Some(device::Pending::Sidetone(l)));
                let result = proxy.set_sidetone(l.as_str()).map(|s| parse_sidetone(&s).unwrap_or(l));
                self.finish(reply, result, |s, l| s.sidetone = Some(l));
            }
            device::Command::Lights(on, reply) => {
                self.update(|s| s.pending = Some(device::Pending::Lights(on)));
                self.finish(reply, proxy.set_lights(on), |s, on| s.lights_on = Some(on));
            }
            device::Command::Lighting(zones, reply) => {
                self.update(|s| s.pending = Some(device::Pending::Lighting));
                let args = zones.iter().map(to_arg).collect();
                self.finish(reply, proxy.set_lighting(args), |s, ()| s.lights_on = Some(true));
            }
        }
    }

    /// Applies one command's D-Bus result to the snapshot and answers the
    /// caller's reply channel. Mirrors `device::Worker::finish`, but a
    /// rejected command here is just that command's error — unlike a lost
    /// dongle in the direct-hardware backend, it says nothing about whether
    /// the service itself is still reachable, so this never needs to signal
    /// that back to `serve`.
    fn finish<T: Copy>(&self, reply: Reply<T>, result: zbus::Result<T>, apply: impl FnOnce(&mut Snapshot, T)) {
        match result {
            Ok(value) => {
                self.update(|s| {
                    apply(s, value);
                    s.pending = None;
                    s.error = None;
                });
                drop(reply.send(Ok(value)));
            }
            Err(e) => {
                let msg = e.to_string();
                self.update(|s| {
                    s.pending = None;
                    s.error = Some(msg.clone());
                });
                drop(reply.send(Err(jbl_quantum::Error::Protocol(msg))));
            }
        }
    }
}

fn parse_link(link: &str, detail: String) -> Link {
    match link {
        "ready" => Link::Ready,
        "headset_off" => Link::HeadsetOff,
        "no_access" => Link::NoAccess(detail),
        _ => Link::NoDongle,
    }
}

fn parse_anc(s: &str) -> Option<jbl_quantum::Anc> {
    use jbl_quantum::Anc;
    match s {
        "on" => Some(Anc::On),
        "talkthru" => Some(Anc::TalkThru),
        "off" => Some(Anc::Off),
        _ => None,
    }
}

fn parse_sidetone(s: &str) -> Option<jbl_quantum::Sidetone> {
    use jbl_quantum::Sidetone;
    match s {
        "low" => Some(Sidetone::Low),
        "mid" => Some(Sidetone::Mid),
        "high" => Some(Sidetone::High),
        "off" => Some(Sidetone::Off),
        _ => None,
    }
}

fn to_arg(z: &jbl_quantum::ZoneLighting) -> quantumengine_dbus::ZoneLightingArg {
    quantumengine_dbus::ZoneLightingArg {
        zone: z.zone.as_str().to_string(),
        brightness: z.brightness,
        colours: z.palette.iter().map(|c| (c.r, c.g, c.b)).collect(),
        effect: z.effect.0,
    }
}

/// Starts `quantumengine --tray` as an independent, detached process.
///
/// Its stdio is detached (not inherited) rather than left to default: the
/// service outlives this window, so if it inherited, say, a pipe this
/// process's own output was captured into, that pipe would never see EOF
/// and whatever is reading it would hang forever waiting for a process that
/// has no reason to exit.
fn start_service() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find my own executable: {e}"))?;
    std::process::Command::new(exe)
        .arg("--tray")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start quantumengine --tray: {e}"))
}
