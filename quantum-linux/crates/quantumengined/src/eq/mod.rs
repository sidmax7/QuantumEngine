//! The equalizer: JBL's headset correction plus a sound profile, applied on
//! the PC by a PipeWire filter in front of the headset's game output. It never
//! talks to the headset itself.
//!
//! A profile is either built in (see [`profiles`]) or one the user made or
//! imported (see [`custom`]); both are found by name. While a custom profile
//! is being edited, its unsaved bands can be previewed live.
//!
//! One thread owns the filter. Everything else reads [`State`] and sends
//! [`Command`]s, the same arrangement as the headset worker in `device`.

pub mod biquad;
pub mod custom;
pub mod pipewire;
pub mod profiles;

use crate::notifier::Notifier;
use crate::output;
use crate::settings;
use biquad::{peak_db, response_db, Band, Coefficients};
use custom::CustomProfile;
use jbl_quantum::models::Model;
use pipewire::{Chain, Filter, Target, RATE, SLOTS};
use profiles::{Correction, PROFILES};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often to check that the filter is still running while the EQ is on.
const WATCH_EVERY: Duration = Duration::from_secs(5);
/// How long to wait before restarting a filter that failed.
const RETRY_AFTER: Duration = Duration::from_secs(30);
/// A preview nobody has updated for this long ends by itself, so a window
/// closed mid-edit can't leave unsaved bands playing.
const PREVIEW_LEASE: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug)]
pub struct State {
    pub enabled: bool,
    /// The selected profile's display name, built-in or custom.
    pub profile: String,
    pub correction: Correction,
    /// The user's own profiles, in the order they were made.
    pub custom: Vec<CustomProfile>,
    /// Whether unsaved bands are playing instead of the selected profile.
    pub previewing: bool,
    /// Why the EQ isn't working, or empty when it is (or is off).
    pub status: String,
}

impl State {
    /// Every profile name, built-in first.
    pub fn profile_names(&self) -> Vec<String> {
        PROFILES.iter().map(|p| p.name.to_string()).chain(self.custom.iter().map(|p| p.name.clone())).collect()
    }

    /// A profile's display name and bands, by name (ignoring case and
    /// punctuation).
    pub fn find(&self, name: &str) -> Option<(String, Vec<Band>)> {
        find(name, &self.custom)
    }
}

/// The output the EQ attaches to, and the model it belongs to: the
/// plugged-in headset's, or else the Quantum 810's output with no model known,
/// so the EQ is ready for its dongle as it always was.
fn resolve() -> (Target, Option<&'static Model>) {
    match output::find() {
        Ok(Some(o)) => (Target::of(&o), Some(o.model)),
        _ => (Target::quantum_810(), None),
    }
}

/// A built-in or custom profile's display name and bands.
pub fn find(name: &str, custom: &[CustomProfile]) -> Option<(String, Vec<Band>)> {
    if let Some(p) = profiles::find(name) {
        return Some((p.name.to_string(), p.bands()));
    }
    let key = profiles::normalise(name);
    custom.iter().find(|p| profiles::normalise(&p.name) == key).map(|p| (p.name.clone(), p.bands.clone()))
}

pub type Reply<T> = Sender<Result<T, String>>;

pub enum Command {
    Enable(bool, Reply<bool>),
    Profile(String, Reply<String>),
    Correction(Correction, Reply<Correction>),
    /// Saves a custom profile and selects it. `original` is the profile being
    /// edited (it may be renamed), or `None` for a new one.
    Save { original: Option<String>, name: String, bands: Vec<Band>, reply: Reply<String> },
    Delete(String, Reply<()>),
    /// Plays these bands in place of the selected profile until `None` (or
    /// a save or profile change) ends the preview.
    Preview(Option<Vec<Band>>, Reply<()>),
    /// Adds a profile from Equalizer APO / AutoEQ text or an EasyEffects
    /// preset, named after `name` (made unique), and selects it.
    Import { name: String, text: String, reply: Reply<String> },
}

#[derive(Clone)]
pub struct Handle {
    state: Arc<Mutex<State>>,
    commands: Sender<Command>,
}

impl Handle {
    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    pub fn send(&self, cmd: Command) {
        // Only fails if the EQ thread has died, which it never does on purpose.
        let _ = self.commands.send(cmd);
    }

    /// Sends `make(reply)` and waits for the answer.
    pub fn call<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
        let (reply, rx) = mpsc::channel();
        self.commands.send(make(reply)).map_err(|_| "the EQ thread has stopped".to_string())?;
        rx.recv().map_err(|_| "no reply from the EQ thread".to_string())?
    }
}

pub fn no_reply<T>() -> Reply<T> {
    mpsc::channel().0
}

/// Starts the EQ thread, applying the saved settings straight away.
pub fn spawn(notifier: Notifier) -> Handle {
    let saved = settings::load().eq;
    let profile = find(&saved.profile, &saved.custom).map_or_else(|| PROFILES[0].name.to_string(), |(name, _)| name);
    let state = Arc::new(Mutex::new(State {
        enabled: saved.enabled,
        profile,
        correction: saved.correction,
        custom: saved.custom,
        previewing: false,
        status: String::new(),
    }));
    let (commands, rx) = mpsc::channel();
    let worker = Worker {
        state: state.clone(),
        notifier,
        filter: None,
        filter_target: None,
        preview: None,
        preview_at: Instant::now(),
        retry_at: None,
    };
    std::thread::Builder::new()
        .name("eq".into())
        .spawn(move || worker.run(rx))
        .expect("failed to start the EQ thread");
    Handle { state, commands }
}

/// Every band that does something: a 0 dB bell or shelf is a pass-through,
/// so it's left out (a pass filter has no gain and always counts).
fn active(correction: Correction, bands: &[Band]) -> Vec<Band> {
    correction.bands().into_iter().chain(bands.iter().copied()).filter(|b| b.gain_db != 0.0 || !b.kind.has_gain()).collect()
}

/// The coefficients for `correction` under `bands`, turned down by however
/// much the combined curve boosts at its highest point so it can never clip.
pub fn chain(correction: Correction, bands: &[Band]) -> Chain {
    let bands = active(correction, bands);
    // The last slot is kept for the volume reduction.
    assert!(bands.len() < SLOTS, "{} bands do not fit the filter", bands.len());
    let mut chain = [Coefficients::IDENTITY; SLOTS];
    for (slot, band) in chain.iter_mut().zip(&bands) {
        *slot = Coefficients::design(band, RATE);
    }
    let headroom = peak_db(&chain, RATE);
    // Anything under a hundredth of a dB is rounding, not a boost.
    if headroom > 0.01 {
        chain[SLOTS - 1] = Coefficients::IDENTITY.scaled(10f64.powf(-headroom / 20.0));
    }
    chain
}

/// The shape of `correction` under `bands`, in dB, at each of `freqs`;
/// [`chain`] without the turn-down that keeps it from clipping.
pub fn response(correction: Correction, bands: &[Band], freqs: impl Iterator<Item = f64>) -> Vec<f64> {
    let filters: Vec<_> = active(correction, bands).iter().map(|b| Coefficients::design(b, RATE)).collect();
    freqs.map(|f| response_db(&filters, f, RATE)).collect()
}

struct Worker {
    state: Arc<Mutex<State>>,
    notifier: Notifier,
    filter: Option<Filter>,
    /// The output the running filter is attached to.
    filter_target: Option<Target>,
    /// Unsaved bands being auditioned, if any, and when they last changed.
    preview: Option<Vec<Band>>,
    preview_at: Instant,
    retry_at: Option<Instant>,
}

impl Worker {
    fn run(mut self, commands: Receiver<Command>) {
        self.apply();
        loop {
            match commands.recv_timeout(WATCH_EVERY) {
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => self.watch(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Enable(on, reply) => {
                let result = self.save(|s| s.eq.enabled = on).map(|()| {
                    self.state.lock().unwrap().enabled = on;
                    on
                });
                drop(reply.send(result));
            }
            Command::Profile(name, reply) => {
                let found = self.state.lock().unwrap().find(&name);
                let result = match found {
                    None => Err(format!("unknown EQ profile '{name}'")),
                    Some((name, _)) => self.select(name),
                };
                drop(reply.send(result));
            }
            Command::Correction(c, reply) => {
                let result = self.save(|s| s.eq.correction = c).map(|()| {
                    self.state.lock().unwrap().correction = c;
                    c
                });
                drop(reply.send(result));
            }
            Command::Save { original, name, bands, reply } => {
                drop(reply.send(self.save_custom(original, &name, bands)));
            }
            Command::Delete(name, reply) => drop(reply.send(self.delete_custom(&name))),
            Command::Preview(bands, reply) => {
                let result = match &bands {
                    Some(b) => custom::check_bands(b),
                    None => Ok(()),
                };
                if result.is_ok() {
                    self.preview = bands;
                    self.preview_at = Instant::now();
                }
                drop(reply.send(result));
            }
            Command::Import { name, text, reply } => {
                let result = custom::parse_import(&text).and_then(|bands| {
                    let name = custom::unique_name(&name, &self.state.lock().unwrap().custom);
                    self.save_custom(None, &name, bands)
                });
                drop(reply.send(result));
            }
        }
        self.state.lock().unwrap().previewing = self.preview.is_some();
        self.retry_at = None;
        self.apply();
    }

    fn save(&self, change: impl FnOnce(&mut settings::Settings)) -> Result<(), String> {
        settings::update(change).map(drop)
    }

    /// Selects a profile by its display name, ending any preview.
    fn select(&mut self, name: String) -> Result<String, String> {
        self.save(|s| s.eq.profile = name.clone())?;
        self.preview = None;
        self.state.lock().unwrap().profile = name.clone();
        Ok(name)
    }

    fn save_custom(&mut self, original: Option<String>, name: &str, bands: Vec<Band>) -> Result<String, String> {
        custom::check_bands(&bands)?;
        let mut list = self.state.lock().unwrap().custom.clone();
        let name = custom::check_name(name, &list, original.as_deref())?;
        let profile = CustomProfile { name: name.clone(), bands };
        match &original {
            Some(old) => {
                let key = profiles::normalise(old);
                let slot = list
                    .iter_mut()
                    .find(|p| profiles::normalise(&p.name) == key)
                    .ok_or_else(|| format!("there is no custom profile called '{old}'"))?;
                *slot = profile;
            }
            None => list.push(profile),
        }
        self.save(|s| {
            s.eq.custom = list.clone();
            s.eq.profile = name.clone();
        })?;
        let mut state = self.state.lock().unwrap();
        state.custom = list;
        state.profile = name.clone();
        drop(state);
        self.preview = None;
        Ok(name)
    }

    fn delete_custom(&mut self, name: &str) -> Result<(), String> {
        if let Some(p) = profiles::find(name) {
            return Err(format!("“{}” is built in; only your own profiles can be deleted.", p.name));
        }
        let key = profiles::normalise(name);
        let (mut list, selected) = {
            let state = self.state.lock().unwrap();
            (state.custom.clone(), profiles::normalise(&state.profile) == key)
        };
        let before = list.len();
        list.retain(|p| profiles::normalise(&p.name) != key);
        if list.len() == before {
            return Err(format!("there is no custom profile called '{name}'"));
        }
        let flat = PROFILES[0].name.to_string();
        self.save(|s| {
            s.eq.custom = list.clone();
            if selected {
                s.eq.profile = flat.clone();
            }
        })?;
        let mut state = self.state.lock().unwrap();
        state.custom = list;
        if selected {
            state.profile = flat;
            self.preview = None;
        }
        Ok(())
    }

    /// Restarts a filter that has died, e.g. because PipeWire restarted, and
    /// ends an abandoned preview.
    fn watch(&mut self) {
        if self.preview.is_some() && self.preview_at.elapsed() >= PREVIEW_LEASE {
            self.preview = None;
            self.state.lock().unwrap().previewing = false;
            self.apply();
            return;
        }
        if !self.state.lock().unwrap().enabled {
            return;
        }
        if self.filter.as_mut().is_some_and(Filter::is_running) {
            // Still running: only a different headset being plugged in
            // changes what it should be attached to.
            if Some(&resolve().0) == self.filter_target.as_ref() {
                return;
            }
            self.apply();
            return;
        }
        self.filter = None;
        if self.retry_at.is_none_or(|t| Instant::now() >= t) {
            self.apply();
        }
    }

    /// Makes the filter match the current state.
    fn apply(&mut self) {
        let state = self.state.lock().unwrap().clone();
        let status = if state.enabled {
            let bands = match &self.preview {
                Some(b) => b.clone(),
                None => state.find(&state.profile).map(|(_, b)| b).unwrap_or_default(),
            };
            let (target, model) = resolve();
            let chain = chain(state.correction.in_use(model), &bands);
            if self.filter.as_mut().is_some_and(|f| !f.is_running()) || self.filter_target.as_ref() != Some(&target) {
                self.filter = None;
            }
            let result = match &mut self.filter {
                Some(f) => f.set(&chain),
                None => Filter::start(&target, &chain).map(|f| {
                    self.filter = Some(f);
                    self.filter_target = Some(target);
                }),
            };
            match result {
                Ok(()) => String::new(),
                Err(e) => {
                    self.filter = None;
                    self.retry_at = Some(Instant::now() + RETRY_AFTER);
                    eprintln!("quantumengined: EQ: {e}");
                    e
                }
            }
        } else {
            self.filter = None;
            String::new()
        };
        self.state.lock().unwrap().status = status;
        self.notifier.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_combination_fits_and_never_boosts_above_zero() {
        for c in Correction::ALL {
            for p in PROFILES {
                let chain = chain(c, &p.bands());
                let peak = peak_db(&chain, RATE);
                assert!(peak <= 0.01, "{} + {}: peak {peak} dB", c.as_str(), p.name);
            }
        }
    }

    #[test]
    fn flat_without_correction_is_a_pass_through() {
        let chain = chain(Correction::Off, &PROFILES[0].bands());
        assert!(chain.iter().all(|c| *c == Coefficients::IDENTITY));
    }

    #[test]
    fn the_drawn_curve_is_the_chain_without_the_turn_down() {
        let profile = profiles::find("Bass Boost").unwrap().bands();
        let freqs = [50.0, 1_000.0, 12_000.0];
        let filters = chain(Correction::Stereo, &profile);
        let headroom = -filters[SLOTS - 1].magnitude_db(1_000.0, RATE);
        for (f, db) in freqs.into_iter().zip(response(Correction::Stereo, &profile, freqs.into_iter())) {
            assert!((db - (response_db(&filters, f, RATE) + headroom)).abs() < 1e-9, "{f} Hz");
        }
        assert!(response(Correction::Off, &PROFILES[0].bands(), freqs.into_iter()).iter().all(|&db| db == 0.0));
    }

    #[test]
    fn the_largest_custom_profile_fits_under_the_largest_tuning() {
        let bands = vec![Band { kind: biquad::FilterType::HighPass, freq: 20.0, gain_db: 0.0, q: 0.7 }; custom::MAX_BANDS];
        let chain = chain(Correction::Surround, &bands);
        assert_eq!(chain.iter().filter(|c| **c != Coefficients::IDENTITY).count(), custom::MAX_BANDS + 5 + 1);
    }

    #[test]
    fn custom_profiles_are_found_by_name_after_built_ins() {
        let mine = vec![CustomProfile { name: "Night Mode".into(), bands: vec![Band::bell(100.0, -3.0, 1.0)] }];
        assert_eq!(find("night-mode", &mine).unwrap().0, "Night Mode");
        assert_eq!(find("cs2", &mine).unwrap().0, "CS2");
        assert!(find("nothing", &mine).is_none());
    }

    #[test]
    fn the_volume_reduction_matches_the_curves_peak() {
        // JBL's surround curve tops out about 6 dB up.
        let chain = chain(Correction::Surround, &PROFILES[0].bands());
        let reduction = chain[SLOTS - 1].magnitude_db(1_000.0, RATE);
        assert!((reduction + 6.0).abs() < 0.05, "turned down {reduction} dB");
    }
}
