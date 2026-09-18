//! A simulated headset, for development and tests without the real device
//! plugged in. Enabled by setting the `QUANTUMENGINE_SIMULATE` environment
//! variable before calling [`crate::Headset::open_first`], or directly with
//! [`crate::Headset::open_simulated`] / [`crate::Headset::open_simulated_disconnected`].
//!
//! It reproduces the behaviour documented at the top of this crate closely
//! enough to exercise client code honestly — in particular, that an
//! immediate read-back is not always trustworthy, and that some settings
//! never produce an event — but it is a simplified model, not a byte-exact
//! replica of the real device's timing. Lighting configuration (`0x4C`/`0x4D`)
//! is accepted but not modelled, since nothing in this crate ever reads it
//! back on real hardware either.

use crate::hidraw::Transport;
use crate::protocol::{event, feature};
use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// An ANC change in flight. `anc_mirror` below derives the current mirror
/// value from this and the elapsed time, so nothing needs to wake up and
/// mutate state when the transition finishes — it self-heals on the next
/// read or write.
struct AncTransition {
    target: u8,
    started: Instant,
    settle: Duration,
    /// On <-> TalkThru passes through Off; the mirror reads Off for the
    /// whole transition instead of the target.
    passthrough_off: bool,
}

struct State {
    anc: u8,
    anc_transition: Option<AncTransition>,
    sidetone: u8,
    mic_active: bool,
    dial: u8,
    lights_on: bool,
    battery: u8,
    connected: bool,
}

impl State {
    fn new(connected: bool) -> Self {
        Self {
            anc: 0x00, // Off
            anc_transition: None,
            sidetone: 0x00, // Off
            mic_active: false,
            dial: 8, // centre
            lights_on: false,
            battery: 80,
            connected,
        }
    }
}

pub(crate) struct SimTransport {
    state: Mutex<State>,
    events_rx: Mutex<Receiver<Vec<u8>>>,
    events_tx: Sender<Vec<u8>>,
}

impl SimTransport {
    pub fn new() -> Self {
        Self::with_state(State::new(true))
    }

    pub fn new_disconnected() -> Self {
        Self::with_state(State::new(false))
    }

    fn with_state(state: State) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self { state: Mutex::new(state), events_rx: Mutex::new(events_rx), events_tx }
    }

    /// The ANC mirror's current value, accounting for any transition in
    /// flight. Settles itself: a transition whose time has passed is treated
    /// as over even before anything has cleared `anc_transition`.
    fn anc_mirror(&self, state: &State) -> u8 {
        match &state.anc_transition {
            None => state.anc,
            Some(t) if t.started.elapsed() >= t.settle => t.target,
            Some(t) if t.passthrough_off => 0x00,
            Some(t) => t.target, // Off<->On/TalkThru: correct at once
        }
    }

    /// Send an event once `delay` has passed. Only borrows the (cheap to
    /// clone, 'static) sending half of the channel, so this never needs to
    /// reach back into `self` or its state lock from the spawned thread.
    fn emit_after(&self, delay: Duration, data: Vec<u8>) {
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let _ = tx.send(data);
        });
    }

    fn emit_now(&self, data: Vec<u8>) {
        let _ = self.events_tx.send(data);
    }
}

impl Transport for SimTransport {
    fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        let (n, buf) = self.get_feature_debug(report_id, len)?;
        let n = n as usize;
        if n < 1 || buf[0] != report_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "report 0x{report_id:02X} did not answer (simulated write-only report)"
                ),
            ));
        }
        Ok(if n > 1 { buf[1..n].to_vec() } else { Vec::new() })
    }

    fn get_feature_debug(&self, report_id: u8, len: usize) -> io::Result<(i32, Vec<u8>)> {
        let state = self.state.lock().unwrap();
        // Real write-only reports don't answer at all: the kernel hands back
        // whatever report was last actually read, with a mismatched ID. `None`
        // here reproduces that mismatch generically, via the same buf[0] !=
        // report_id check `get_feature` above uses on real hardware.
        let value: Option<Vec<u8>> = match report_id {
            feature::ANC_MIRROR => Some(vec![self.anc_mirror(&state)]),
            feature::SIDETONE_MIRROR => Some(vec![state.sidetone]),
            feature::LIGHTS_MIRROR => {
                // Reading this report can itself trigger an event on real
                // hardware; simulate the worst case (every read does) so
                // client code is forced to handle it rather than assume it
                // won't happen.
                let on = state.lights_on;
                self.emit_now(vec![event::LIGHTS, u8::from(on)]);
                Some(vec![u8::from(on)])
            }
            feature::BATTERY => {
                // Every real read of the battery produces an event, not only
                // reads that changed it.
                let pct = state.battery;
                self.emit_now(vec![event::BATTERY, pct]);
                Some(vec![pct])
            }
            feature::CONNECTION => Some(vec![u8::from(state.connected)]),
            feature::MIC_ACTIVE => Some(vec![u8::from(state.mic_active)]),
            feature::DIAL => Some(vec![state.dial]),
            feature::DEVICE_NAME => {
                let name = b"PC-simulated #1";
                let mut v = vec![name.len() as u8, 0];
                v.extend_from_slice(name);
                Some(v)
            }
            feature::SERIAL => Some(b"SIM0000-0000000".to_vec()),
            feature::FIRMWARE => Some(vec![0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0]),
            _ => None,
        };
        match value {
            Some(mut v) => {
                v.resize(len, 0);
                let mut buf = vec![report_id];
                buf.append(&mut v);
                Ok((buf.len() as i32, buf))
            }
            None => Ok((0, vec![0u8; len + 1])),
        }
    }

    fn set_feature(&self, payload: &[u8]) -> io::Result<()> {
        let Some(&report) = payload.first() else { return Ok(()) };
        let mut state = self.state.lock().unwrap();
        match report {
            feature::ANC => {
                // Settle a finished transition before deciding what a new
                // write means, so a write to the already-settled target is
                // correctly treated as a no-op.
                if let Some(t) = &state.anc_transition {
                    if t.started.elapsed() >= t.settle {
                        state.anc = t.target;
                        state.anc_transition = None;
                    }
                }
                let target = payload.get(1).copied().unwrap_or(0);
                let current = self.anc_mirror(&state);
                if current == target && state.anc_transition.is_none() {
                    return Ok(()); // ANC to its current value: no-op, no event
                }
                let passthrough = matches!((current, target), (0x01, 0x02) | (0x02, 0x01));
                let settle = if passthrough {
                    Duration::from_millis(400)
                } else if target == 0x00 {
                    Duration::from_millis(470)
                } else {
                    Duration::from_millis(120)
                };
                state.anc_transition =
                    Some(AncTransition { target, started: Instant::now(), settle, passthrough_off: passthrough });
                self.emit_after(settle, vec![event::ANC, target]);
            }
            feature::SIDETONE => {
                state.sidetone = payload.get(1).copied().unwrap_or(0);
                // No event, ever — matches the real device.
            }
            feature::LIGHTS => {
                let on = payload.get(1).copied().unwrap_or(0) != 0;
                state.lights_on = on;
                self.emit_after(Duration::from_millis(50), vec![event::LIGHTS, u8::from(on)]);
            }
            feature::LIGHT_CONFIG | feature::LIGHT_COLOUR => {
                // Accepted, not modelled — see the module doc comment.
            }
            _ => {}
        }
        Ok(())
    }

    fn read_report(&self) -> io::Result<Vec<u8>> {
        self.events_rx
            .lock()
            .unwrap()
            .recv()
            .map_err(|_| io::Error::new(io::ErrorKind::NotConnected, "simulated dongle closed"))
    }

    fn read_report_timeout(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        match self.events_rx.lock().unwrap().recv_timeout(timeout) {
            Ok(data) => Ok(Some(data)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                Err(io::Error::new(io::ErrorKind::NotConnected, "simulated dongle closed"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Anc, Headset, Sidetone};

    #[test]
    fn reports_a_plausible_default_state() {
        let h = Headset::open_simulated();
        assert!(h.is_simulated());
        assert!(h.connected().unwrap());
        assert_eq!(h.anc().unwrap(), Anc::Off);
        assert_eq!(h.sidetone().unwrap(), Sidetone::Off);
        assert!(!h.lights_on().unwrap());
        assert_eq!(h.device_name().unwrap(), "PC-simulated #1");
        assert_eq!(h.serial().unwrap(), "SIM0000-0000000");
        assert_eq!(h.firmware().unwrap().len(), 4);
    }

    #[test]
    fn open_simulated_disconnected_reads_as_not_connected() {
        let h = Headset::open_simulated_disconnected();
        assert!(!h.connected().unwrap());
        assert!(matches!(
            h.set_anc_confirmed(Anc::On, Duration::from_millis(500)),
            Err(crate::Error::NotConnected)
        ));
    }

    #[test]
    fn simple_anc_transition_confirms() {
        let h = Headset::open_simulated();
        let mode = h.set_anc_confirmed(Anc::On, Duration::from_secs(1)).unwrap();
        assert_eq!(mode, Anc::On);
    }

    #[test]
    fn talkthru_passthrough_transition_confirms() {
        let h = Headset::open_simulated();
        h.set_anc_confirmed(Anc::On, Duration::from_secs(1)).unwrap();
        let mode = h.set_anc_confirmed(Anc::TalkThru, Duration::from_secs(1)).unwrap();
        assert_eq!(mode, Anc::TalkThru);
    }

    #[test]
    fn writing_the_current_anc_mode_is_immediate() {
        let h = Headset::open_simulated();
        // Off is the default; asking for Off again must not wait for an
        // event that will never come.
        let mode = h.set_anc_confirmed(Anc::Off, Duration::from_millis(1)).unwrap();
        assert_eq!(mode, Anc::Off);
    }

    #[test]
    fn sidetone_confirms_with_no_event() {
        let h = Headset::open_simulated();
        let level = h.set_sidetone_confirmed(Sidetone::High, Duration::from_millis(500)).unwrap();
        assert_eq!(level, Sidetone::High);
    }

    #[test]
    fn lights_confirm_via_their_event() {
        let h = Headset::open_simulated();
        let on = h.set_lights_confirmed(true, Duration::from_secs(1)).unwrap();
        assert!(on);
    }

    #[test]
    fn write_only_reports_do_not_answer() {
        let h = Headset::open_simulated();
        assert!(h.raw_feature(feature::ANC, 1).is_err());
        assert!(h.raw_feature(feature::LIGHTS, 1).is_err());
    }
}
