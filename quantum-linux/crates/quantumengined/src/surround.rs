//! Virtual 7.1 surround: a PipeWire sink with eight channels, each played
//! through a binaural room impulse response (see [`crate::brir`]: the MIT
//! KEMAR head libmysofa ships, corrected, in a small simulated room), mixed
//! down to stereo for the headset's game output.
//!
//! This is the Linux side of what QuantumENGINE does on Windows, where the
//! headphones are set to 7.1 in the sound settings and the PC renders the
//! channels binaurally. Here the sink is the 7.1 device: games that offer
//! 7.1 send it eight channels, and stereo plays from two virtual speakers in
//! front. Switching it on makes it the default output; switching it off puts
//! the headset back. Its output goes to the headset's game output like any
//! app, so the EQ still applies on top.
//!
//! The graph follows the structure of Harman's HX3D virtualiser, as found by
//! static analysis of QuantumENGINE 2.8.6 (`reverse engineer/surroundsound/`):
//! the same 7.1 speaker angles, a 0.9 trim on multichannel input, bass below
//! 120 Hz kept out of the HRTF and sent to both ears, LFE added to both ears
//! at half level, and JBL's surround tuning after the binaural stage (the EQ
//! switches to it while surround is on). Only those numbers and that
//! structure are reused; the HRTF data is the open MIT KEMAR set.
//!
//! ```text
//! FL FR FC RL RR SL SR ─┬─ BRIR per ear ──── sum L/R ─ high-pass 120 Hz (LR4) ─┐
//!                       └─ sum ×0.45 ──────┐                                      ├─ L / R
//! LFE ─ HP 15 Hz ─ shelf ─ ×0.45 ──────────┴─ low-pass 120 Hz (LR4) ─ both ears ─┘
//! ```
//!
//! One thread owns the graph; everything else reads [`State`] and sends
//! [`Command`]s, as with the EQ.

use crate::brir;
use crate::eq::{self, profiles::Correction};
use crate::output;
use crate::notifier::Notifier;
use crate::pwgraph::{self, quoted, Graph};
use crate::settings;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NODE_NAME: &str = "quantumengine_surround";
const DESCRIPTION: &str = "QuantumEngine Surround 7.1";
/// libmysofa's own copy of the MIT KEMAR measurements (BSD-3-Clause);
/// `pipewire-audio` depends on libmysofa, so it's there wherever PipeWire's
/// audio stack is.
const HRTF: &str = "/usr/share/libmysofa/MIT_KEMAR_normal_pinna.sofa";
const RATE: u32 = 48_000;
const WATCH_EVERY: Duration = Duration::from_secs(5);
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// The seven directional 7.1 channels in PipeWire's order, with where each
/// speaker sits: degrees anticlockwise from straight ahead, as SOFA files
/// measure them (so 30 is front left). These are the angles of
/// QuantumENGINE's "Dolby 7.1" layout, which are also ITU-R BS.775's. LFE
/// has no direction and takes the bass path instead.
const SPEAKERS: [(&str, f64); 7] = [
    ("FL", 30.0),
    ("FR", 330.0),
    ("FC", 0.0),
    ("RL", 150.0),
    ("RR", 210.0),
    ("SL", 90.0),
    ("SR", 270.0),
];
/// Every channel PipeWire sends, in order: the speakers with LFE fourth.
const POSITIONS: &str = "FL FR FC LFE RL RR SL SR";
/// HX3D trims 5.1 and 7.1 input to 0.9 before rendering.
const TRIM: f64 = 0.9;
/// Below this, sound skips the HRTF and goes to both ears (HX3D's
/// `XOverFc`): KEMAR's low end adds nothing but colouring, and bass has no
/// direction you could hear anyway.
const CROSSOVER_HZ: f64 = 120.0;
/// Each channel's share of the summed bass, and LFE's, both as HX3D adds
/// LFE: half level to each ear (then trimmed).
const BASS_GAIN: f64 = 0.5 * TRIM;
/// Butterworth Q; two in a row make a Linkwitz-Riley crossover, whose low
/// and high halves add back up flat.
const BUTTERWORTH_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// Convolution partition sizes, set rather than left to defaults: a short
/// first block keeps per-cycle work small (this is what stopped the
/// crackling), and the 12288-tap responses take three tail blocks.
const BLOCKSIZE: u32 = 256;
const TAILSIZE: u32 = 4096;
/// The buffer the surround sink asks for. Bigger than PipeWire's minimum, so
/// every cycle has room for 7 convolutions; 21 ms of latency.
const LATENCY: &str = "1024/48000";

#[derive(Clone, Debug)]
pub struct State {
    pub enabled: bool,
    /// Why surround isn't working, or empty when it is (or is off).
    pub status: String,
}

pub type Reply<T> = Sender<Result<T, String>>;

pub enum Command {
    Enable(bool, Reply<bool>),
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
        let _ = self.commands.send(cmd);
    }

    pub fn call<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
        let (reply, rx) = mpsc::channel();
        self.commands.send(make(reply)).map_err(|_| "the surround thread has stopped".to_string())?;
        rx.recv().map_err(|_| "no reply from the surround thread".to_string())?
    }
}

pub fn no_reply<T>() -> Reply<T> {
    mpsc::channel().0
}

/// Starts the surround thread, applying the saved setting straight away.
/// `eq` is told which JBL tuning goes with surround on or off.
pub fn spawn(notifier: Notifier, eq: eq::Handle) -> Handle {
    let state = Arc::new(Mutex::new(State { enabled: settings::load().surround.enabled, status: String::new() }));
    let (commands, rx) = mpsc::channel();
    let worker = Worker { state: state.clone(), notifier, eq, graph: None, target: None, brir: None, retry_at: None };
    std::thread::Builder::new()
        .name("surround".into())
        .spawn(move || worker.run(rx))
        .expect("failed to start the surround thread");
    Handle { state, commands }
}

struct Worker {
    state: Arc<Mutex<State>>,
    notifier: Notifier,
    eq: eq::Handle,
    graph: Option<Graph>,
    /// The headset sink the running graph plays into.
    target: Option<String>,
    /// The impulse-response file, built once per run of the service.
    brir: Option<PathBuf>,
    retry_at: Option<Instant>,
}

impl Worker {
    fn run(mut self, commands: Receiver<Command>) {
        self.apply(false);
        loop {
            match commands.recv_timeout(WATCH_EVERY) {
                Ok(Command::Enable(on, reply)) => {
                    let result = settings::update(|s| s.surround.enabled = on).map(|_| {
                        self.state.lock().unwrap().enabled = on;
                        on
                    });
                    let changed = result.is_ok();
                    drop(reply.send(result));
                    if changed {
                        self.retry_at = None;
                        self.apply(true);
                        self.match_tuning(on);
                    }
                }
                Err(RecvTimeoutError::Timeout) => self.watch(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Swaps JBL's stereo tuning for its surround tuning and back, as HX3D
    /// does: the surround curve is the driver EQ it puts after the binaural
    /// stage, the stereo one what it uses with 3D off. A tuning the user set
    /// to off stays off.
    fn match_tuning(&self, surround: bool) {
        let (from, to) = if surround {
            (Correction::Stereo, Correction::Surround)
        } else {
            (Correction::Surround, Correction::Stereo)
        };
        if self.eq.state().correction == from {
            self.eq.send(eq::Command::Correction(to, eq::no_reply()));
        }
    }

    /// Follows the headset's dongle coming and going, and restarts a graph
    /// that died (PipeWire restarted, say).
    fn watch(&mut self) {
        if !self.state.lock().unwrap().enabled {
            return;
        }
        let target = headset_sink().ok().flatten();
        // PipeWire removes the sink without stopping the process when its
        // target vanishes, so the node is checked as well as the process.
        let healthy = self.graph.as_mut().is_some_and(Graph::is_running)
            && target.is_some()
            && target == self.target
            && pwgraph::node_id(NODE_NAME).ok().flatten().is_some();
        if healthy {
            return;
        }
        if self.retry_at.is_none_or(|t| Instant::now() >= t) || target != self.target {
            self.apply(false);
        }
    }

    /// Makes the graph match the state. `switched` is true when the user
    /// just turned surround on or off, which also moves the default output.
    fn apply(&mut self, switched: bool) {
        let enabled = self.state.lock().unwrap().enabled;
        let status = if enabled { self.start(switched) } else { self.stop(switched) };
        if let Err(e) = &status {
            eprintln!("quantumengined: surround: {e}");
        }
        self.state.lock().unwrap().status = status.err().unwrap_or_default();
        self.notifier.notify();
    }

    fn start(&mut self, make_default: bool) -> Result<(), String> {
        let target = match headset_sink()? {
            Some(t) => t,
            None => {
                // Nothing to play into, and a sink with nowhere to go would
                // just be a silent default. It starts when the dongle is back.
                self.graph = None;
                self.target = None;
                return Err("Waiting for the headset's dongle.".into());
            }
        };
        if !Path::new(HRTF).is_file() {
            return Err(format!("Needs libmysofa's HRTF data ({HRTF}); install libmysofa."));
        }
        let running = self.graph.as_mut().is_some_and(Graph::is_running) && pwgraph::node_id(NODE_NAME)?.is_some();
        if !running || self.target.as_deref() != Some(&target) {
            self.graph = None;
            let brir = self.brir_file()?;
            let graph = Graph::start(NODE_NAME, "the PipeWire surround filter", &config(NODE_NAME, &target, &brir))
                .inspect_err(|_| self.retry_at = Some(Instant::now() + RETRY_AFTER))?;
            self.graph = Some(graph);
            self.target = Some(target);
        }
        if make_default {
            let id = pwgraph::node_id(NODE_NAME)?.ok_or("the surround output disappeared")?;
            pwgraph::set_default(id)?;
        }
        Ok(())
    }

    /// Builds the BRIR file the first time surround starts.
    fn brir_file(&mut self) -> Result<PathBuf, String> {
        if let Some(path) = self.brir.as_ref().filter(|p| p.is_file()) {
            return Ok(path.clone());
        }
        let azimuths: Vec<f64> = SPEAKERS.iter().map(|(_, az)| *az).collect();
        let brirs = brir::build(Path::new(HRTF), &azimuths)?;
        let dir = pwgraph::runtime_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let path = dir.join(format!("{NODE_NAME}_brir.wav"));
        brir::write_wav(&path, &brirs)?;
        self.brir = Some(path.clone());
        Ok(path)
    }

    fn stop(&mut self, restore_default: bool) -> Result<(), String> {
        let was_default = restore_default && pwgraph::default_sink()?.as_deref() == Some(NODE_NAME);
        self.graph = None;
        self.target = None;
        if was_default {
            // WirePlumber would pick some output on its own once ours is
            // gone; the headset is the one that was swapped out for it.
            if let Some(headset) = output::find()? {
                pwgraph::set_default(headset.node_id)?;
            }
        }
        Ok(())
    }
}

/// The node name of the headset's game output, if its dongle is plugged in.
fn headset_sink() -> Result<Option<String>, String> {
    Ok(output::find()?.map(|o| o.node_name))
}

fn config(node_name: &str, target: &str, brir: &Path) -> String {
    let brir = quoted(&brir.to_string_lossy());
    let mut nodes = String::new();
    let mut links = String::new();
    let mut spatial_gains = String::new();
    let mut bass_gains = String::new();
    let mut node = |line: String| {
        let _ = writeln!(nodes, "          {line}");
    };
    let mut link = |from: &str, to: &str| {
        let _ = writeln!(links, "          {{ output = \"{from}\" input = \"{to}\" }}");
    };

    for (i, (channel, _)) in SPEAKERS.iter().enumerate() {
        let n = i + 1;
        // One input feeds three places, both ears' convolvers and the bass
        // sum, so it goes through a copy node first.
        node(format!("{{ type = builtin label = copy name = in{channel} }}"));
        // One convolver per ear; the WAV holds left then right for each
        // speaker, in SPEAKERS order.
        for (ear, side) in ["L", "R"].iter().enumerate() {
            let wav_channel = 2 * i + ear;
            node(format!(
                "{{ type = builtin label = convolver name = cv{channel}{side} config = {{ filename = {brir} channel = {wav_channel} blocksize = {BLOCKSIZE} tailsize = {TAILSIZE} }} }}"
            ));
            link(&format!("in{channel}:Out"), &format!("cv{channel}{side}:In"));
            link(&format!("cv{channel}{side}:Out"), &format!("hrtf{side}:In {n}"));
        }
        link(&format!("in{channel}:Out"), &format!("bass:In {n}"));
        let _ = write!(spatial_gains, " \"Gain {n}\" = {TRIM:.2}");
        let _ = write!(bass_gains, " \"Gain {n}\" = {BASS_GAIN:.2}");
    }
    let lfe = SPEAKERS.len() + 1;
    let _ = write!(bass_gains, " \"Gain {lfe}\" = {BASS_GAIN:.2}");

    node(format!("{{ type = builtin label = mixer name = hrtfL control = {{{spatial_gains} }} }}"));
    node(format!("{{ type = builtin label = mixer name = hrtfR control = {{{spatial_gains} }} }}"));
    // HX3D's LFE channel EQ: a 15 Hz high-pass and a gentle shelf.
    node(format!("{{ type = builtin label = bq_highpass name = lfeHp control = {{ \"Freq\" = 15.0 \"Q\" = {BUTTERWORTH_Q:.4} }} }}"));
    node("{ type = builtin label = bq_lowshelf name = lfeShelf control = { \"Freq\" = 500.0 \"Q\" = 0.7071 \"Gain\" = -2.0 } }".into());
    node(format!("{{ type = builtin label = mixer name = bass control = {{{bass_gains} }} }}"));
    link("lfeHp:Out", "lfeShelf:In");
    link("lfeShelf:Out", &format!("bass:In {lfe}"));

    // The crossover: the binaural sum loses its lows, the bass sum its highs.
    let bq = |label: &str, name: &str| {
        format!("{{ type = builtin label = {label} name = {name} control = {{ \"Freq\" = {CROSSOVER_HZ:.1} \"Q\" = {BUTTERWORTH_Q:.4} }} }}")
    };
    for side in ["L", "R"] {
        node(bq("bq_highpass", &format!("hp{side}1")));
        node(bq("bq_highpass", &format!("hp{side}2")));
        link(&format!("hrtf{side}:Out"), &format!("hp{side}1:In"));
        link(&format!("hp{side}1:Out"), &format!("hp{side}2:In"));
        node(format!("{{ type = builtin label = mixer name = out{side} }}"));
        link(&format!("hp{side}2:Out"), &format!("out{side}:In 1"));
        link("lp2:Out", &format!("out{side}:In 2"));
    }
    node(bq("bq_lowpass", "lp1"));
    node(bq("bq_lowpass", "lp2"));
    link("bass:Out", "lp1:In");
    link("lp1:Out", "lp2:In");

    let inputs: Vec<String> = POSITIONS
        .split(' ')
        .map(|c| if c == "LFE" { "\"lfeHp:In\"".to_string() } else { format!("\"in{c}:In\"") })
        .collect();
    let inputs = inputs.join(" ");
    let name = quoted(node_name);
    let output_name = quoted(&format!("{node_name}.output"));
    let description = quoted(DESCRIPTION);
    let target = quoted(target);
    format!(
        r#"# Written by quantumengined; regenerated every time surround starts.
context.properties = {{ log.level = 0 }}
context.spa-libs = {{
  audio.convert.* = audioconvert/libspa-audioconvert
  support.* = support/libspa-support
}}
context.modules = [
  {{ name = libpipewire-module-rt args = {{ }} flags = [ ifexists nofail ] }}
  {{ name = libpipewire-module-protocol-native }}
  {{ name = libpipewire-module-client-node }}
  {{ name = libpipewire-module-adapter }}
  {{ name = libpipewire-module-filter-chain
    args = {{
      node.description = {description}
      media.name = {description}
      filter.graph = {{
        nodes = [
{nodes}        ]
        links = [
{links}        ]
        inputs = [ {inputs} ]
        outputs = [ "outL:Out" "outR:Out" ]
      }}
      capture.props = {{
        node.name = {name}
        media.class = Audio/Sink
        audio.rate = {RATE}
        audio.channels = 8
        audio.position = [ {POSITIONS} ]
        node.latency = {LATENCY}
        # Stereo stays in the two front speakers; PipeWire's default upmix
        # would spread diffuse copies into the sides and rear.
        channelmix.upmix = false
      }}
      playback.props = {{
        node.name = {output_name}
        node.passive = true
        node.latency = {LATENCY}
        audio.rate = {RATE}
        audio.channels = 2
        audio.position = [ FL FR ]
        stream.dont-remix = true
        target.object = {target}
        # Never fall back to another output (that could be this sink itself):
        # if the headset goes away, this waits for it.
        node.dont-fallback = true
        node.dont-reconnect = true
      }}
    }}
  }}
]
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_renders_seven_speakers_and_keeps_bass_out_of_the_hrtf() {
        let conf = config("test_surround", "some_headset", Path::new("/run/brir.wav"));
        assert_eq!(conf.matches("label = convolver").count(), 14);
        assert!(conf.contains("audio.position = [ FL FR FC LFE RL RR SL SR ]"));
        assert!(conf.contains("inputs = [ \"inFL:In\" \"inFR:In\" \"inFC:In\" \"lfeHp:In\" \"inRL:In\""));
        // SL is the sixth speaker: WAV channels 10 (left ear) and 11 (right).
        assert!(conf.contains(
            "name = cvSLR config = { filename = \"/run/brir.wav\" channel = 11 blocksize = 256 tailsize = 4096 }"
        ));
        // Every speaker feeds both the HRTF and the bass sum; LFE only the bass.
        assert_eq!(conf.matches("input = \"bass:In ").count(), 8);
        assert_eq!(conf.matches("input = \"hrtfL:In ").count(), 7);
        assert!(conf.contains("{ output = \"lfeShelf:Out\" input = \"bass:In 8\" }"));
        // Both ears get the high-passed binaural sum and the shared bass.
        assert!(conf.contains("{ output = \"lp2:Out\" input = \"outL:In 2\" }"));
        assert!(conf.contains("{ output = \"lp2:Out\" input = \"outR:In 2\" }"));
        assert_eq!(conf.matches("label = bq_highpass name = hp").count(), 4);
        assert!(conf.contains("\"Gain 1\" = 0.90"));
        assert!(conf.contains("\"Gain 8\" = 0.45"));
        assert!(conf.contains("channelmix.upmix = false"));
        assert!(conf.contains("target.object = \"some_headset\""));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Run with `cargo test -- --ignored`. Plays into an existing output but
    /// is never made the default and nothing plays into it, so it stays
    /// silent. (A target that doesn't exist makes PipeWire remove the sink.)
    #[test]
    #[ignore = "starts a real PipeWire filter in the current session"]
    fn graph_starts_with_eight_channels_and_goes_away() {
        let name = "quantumengine_surround_test";
        let headset = output::find().unwrap().map(|o| o.node_name);
        let (_, target) = pwgraph::find_node(|p| p["media.class"] == "Audio/Sink" && p["node.name"].as_str() != headset.as_deref())
            .unwrap()
            .expect("an audio output to play into");
        let azimuths: Vec<f64> = SPEAKERS.iter().map(|(_, az)| *az).collect();
        let wav = pwgraph::runtime_dir().join("quantumengine_surround_test_brir.wav");
        brir::write_wav(&wav, &brir::build(Path::new(HRTF), &azimuths).unwrap()).unwrap();
        let graph = Graph::start(name, "the test surround filter", &config(name, &target, &wav)).unwrap();
        let found = pwgraph::find_node(|p| p["node.name"] == name && p["audio.channels"] == 8).unwrap();
        assert!(found.is_some(), "an 8-channel sink called {name}");
        drop(graph);
        std::thread::sleep(Duration::from_millis(500));
        assert!(pwgraph::node_id(name).unwrap().is_none());
    }
}
