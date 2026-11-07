//! The headset's output volume.
//!
//! This is the real volume of the headset's PipeWire output, the one the
//! desktop's own slider and volume keys move, so the window, the widget and
//! the desktop always agree and nothing is stored here. The range goes past
//! 100% as a boost: `wpctl` stops at 100% unless it is given a higher limit,
//! and above 100% is software gain, which can distort loud passages.

use crate::notifier::Notifier;
use crate::output;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The most the volume can be set to.
pub const MAX_PERCENT: u32 = 150;

/// How often to read the volume, so a change made elsewhere shows up here.
const WATCH_EVERY: Duration = Duration::from_secs(2);
/// With no headset output known, looking for one means dumping the whole
/// PipeWire graph, so that is done on one read in this many.
const LOOK_EVERY: u32 = 5;

#[derive(Default)]
struct State {
    /// The PipeWire node of the headset's output, once found.
    node: Option<u64>,
    /// Its volume, or `None` when there is no headset output.
    percent: Option<u32>,
}

#[derive(Clone)]
pub struct Handle {
    state: Arc<Mutex<State>>,
    notifier: Notifier,
}

/// Starts watching the headset's volume.
pub fn spawn(notifier: Notifier) -> Handle {
    let handle = Handle { state: Arc::default(), notifier };
    let worker = handle.clone();
    std::thread::Builder::new()
        .name("volume".into())
        .spawn(move || {
            let mut idle = 0;
            loop {
                worker.refresh(&mut idle);
                std::thread::sleep(WATCH_EVERY);
            }
        })
        .expect("failed to start the volume thread");
    handle
}

impl Handle {
    /// The headset output's volume in percent, or `None` when there is none.
    pub fn percent(&self) -> Option<u32> {
        self.state.lock().unwrap().percent
    }

    /// Sets the volume and returns what it reads back as.
    pub fn set(&self, percent: u32) -> Result<u32, String> {
        if percent > MAX_PERCENT {
            return Err(format!("the volume goes up to {MAX_PERCENT}%"));
        }
        let node = output::find()?.ok_or("There is no headset output to set the volume of.")?.node_id;
        let limit = format!("{}", f64::from(MAX_PERCENT) / 100.0);
        let target = format!("{:.2}", f64::from(percent) / 100.0);
        wpctl(&["set-volume", "-l", &limit, &node.to_string(), &target])?;
        let now = read(node)?;
        {
            let mut state = self.state.lock().unwrap();
            state.node = Some(node);
            state.percent = Some(now);
        }
        self.notifier.notify();
        Ok(now)
    }

    fn refresh(&self, idle: &mut u32) {
        let known = self.state.lock().unwrap().node;
        let node = known.or_else(|| {
            *idle += 1;
            if *idle % LOOK_EVERY == 1 { output::find().ok().flatten().map(|o| o.node_id) } else { None }
        });
        let percent = node.and_then(|n| read(n).ok());
        // A node that no longer answers has gone, say with the dongle.
        let node = percent.and(node);
        if node.is_some() {
            *idle = 0;
        }
        let changed = {
            let mut state = self.state.lock().unwrap();
            let changed = state.percent != percent;
            state.node = node;
            state.percent = percent;
            changed
        };
        if changed {
            self.notifier.notify();
        }
    }
}

fn read(node: u64) -> Result<u32, String> {
    let text = wpctl(&["get-volume", &node.to_string()])?;
    parse(&text).ok_or_else(|| format!("cannot read the volume from '{}'", text.trim()))
}

/// `Volume: 0.85` (or `Volume: 0.85 [MUTED]`) as a percentage.
fn parse(text: &str) -> Option<u32> {
    let value: f64 = text.split("Volume:").nth(1)?.split_whitespace().next()?.parse().ok()?;
    (value >= 0.0).then(|| (value * 100.0).round() as u32)
}

fn wpctl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("wpctl")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run wpctl: {e}"))?;
    if !out.status.success() {
        return Err(format!("wpctl {} failed: {}", args[0], String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpctl_volumes_read_as_percent() {
        assert_eq!(parse("Volume: 0.41\n"), Some(41));
        assert_eq!(parse("Volume: 1.50"), Some(150));
        assert_eq!(parse("Volume: 0.85 [MUTED]\n"), Some(85));
        assert_eq!(parse("Volume: 0.005"), Some(1));
        assert_eq!(parse("nothing here"), None);
        assert_eq!(parse("Volume: loud"), None);
    }

    #[test]
    fn a_volume_past_the_limit_is_refused_before_anything_runs() {
        let handle = Handle { state: Arc::default(), notifier: Notifier::default() };
        assert!(handle.set(MAX_PERCENT + 1).unwrap_err().contains("150%"));
    }
}
