//! The EQ's PipeWire filter-chain (run by [`Graph`], in a child `pipewire -c`
//! process).
//!
//! The chain is a fixed row of [`SLOTS`] `bq_raw` biquads, one per band, with
//! unused slots set to pass-through. `bq_raw` is the one builtin whose
//! coefficients can be changed while it runs, so switching profiles is a single
//! `pw-cli set-param` with no restart and no gap in the audio, whatever mix of
//! filter types the new curve needs.
//!
//! The sink is a WirePlumber smart filter targeting the headset's game output:
//! WirePlumber routes anything headed for that device through the EQ first,
//! never picks a smart filter as the default device, and leaves the chat
//! output alone.

use super::biquad::Coefficients;
use crate::pwgraph::{node_id, quoted, Graph};
use std::fmt::Write as _;
use std::process::{Command, Stdio};

/// Up to 18 bands of a custom profile, 5 of JBL's tuning, and the turn-down.
pub const SLOTS: usize = 24;
/// The filter runs at a fixed rate so the coefficients computed for it are
/// right; PipeWire resamples around it if the graph runs at another rate.
pub const RATE: f64 = 48_000.0;

pub type Chain = [Coefficients; SLOTS];

const NODE_NAME: &str = "quantumengine_eq";

/// The sink the EQ sits in front of, as WirePlumber match rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub card: String,
    pub profile: String,
}

impl Target {
    /// The Quantum 810's game output, which the EQ waits on when no headset is
    /// plugged in. Matching on the card and profile rather than the node name
    /// means it works with any 810 dongle, plugged in or not.
    pub fn quantum_810() -> Self {
        Self { card: "JBL Quantum810 Wireless".into(), profile: "stereo-game-output".into() }
    }

    /// The output of the headset found by [`crate::output`].
    pub fn of(output: &crate::output::Output) -> Self {
        Self { card: output.card.clone(), profile: output.profile.clone() }
    }
}

pub struct Filter {
    graph: Graph,
}

impl Filter {
    pub fn start(target: &Target, chain: &Chain) -> Result<Self, String> {
        Self::start_named(NODE_NAME, target, chain)
    }

    fn start_named(node_name: &str, target: &Target, chain: &Chain) -> Result<Self, String> {
        Graph::start(node_name, "the PipeWire EQ filter", &config(node_name, target, chain)).map(|graph| Self { graph })
    }

    /// Replaces every slot's coefficients in place.
    pub fn set(&mut self, chain: &Chain) -> Result<(), String> {
        let id = node_id(self.graph.node_name())?.ok_or("the PipeWire EQ filter has gone away")?;
        let out = Command::new("pw-cli")
            .args(["set-param", &id.to_string(), "Props", &props(chain)])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("cannot run pw-cli: {e}"))?;
        // pw-cli reports a failed set-param on stdout and still exits 0.
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        if !out.status.success() || text.contains("Error") || text.contains("error") {
            return Err(format!("pw-cli set-param failed: {}", text.trim()));
        }
        Ok(())
    }

    pub fn is_running(&mut self) -> bool {
        self.graph.is_running()
    }
}

fn num(x: f64) -> String {
    format!("{x:.15}")
}

fn config(node_name: &str, target: &Target, chain: &Chain) -> String {
    let mut nodes = String::new();
    let mut links = String::new();
    for (i, c) in chain.iter().enumerate() {
        let n = i + 1;
        let _ = writeln!(
            nodes,
            "          {{ type = builtin name = eq_{n} label = bq_raw config = {{ coefficients = [ {{ rate = {rate} b0 = {} b1 = {} b2 = {} a0 = 1.0 a1 = {} a2 = {} }} ] }} }}",
            num(c.b0),
            num(c.b1),
            num(c.b2),
            num(c.a1),
            num(c.a2),
            rate = RATE as u32,
        );
        if n < SLOTS {
            let _ = writeln!(links, "          {{ output = \"eq_{n}:Out\" input = \"eq_{}:In\" }}", n + 1);
        }
    }
    let rate = RATE as u32;
    let name = quoted(node_name);
    let output_name = quoted(&format!("{node_name}.output"));
    let card = quoted(&target.card);
    let profile = quoted(&target.profile);
    format!(
        r#"# Written by quantumengined; regenerated every time the EQ starts.
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
      node.description = "QuantumEngine EQ"
      media.name = "QuantumEngine EQ"
      filter.graph = {{
        nodes = [
{nodes}        ]
        links = [
{links}        ]
      }}
      audio.channels = 2
      audio.position = [ FL FR ]
      capture.props = {{
        node.name = {name}
        media.class = Audio/Sink
        audio.rate = {rate}
        filter.smart = true
        filter.smart.name = {name}
        filter.smart.target = {{ alsa.card_name = {card} device.profile.name = {profile} }}
      }}
      playback.props = {{
        node.name = {output_name}
        node.passive = true
        stream.dont-remix = true
        audio.rate = {rate}
      }}
    }}
  }}
]
"#
    )
}

fn props(chain: &Chain) -> String {
    let mut params = String::new();
    for (i, c) in chain.iter().enumerate() {
        let n = i + 1;
        for (port, value) in [("b0", c.b0), ("b1", c.b1), ("b2", c.b2), ("a0", 1.0), ("a1", c.a1), ("a2", c.a2)] {
            let _ = write!(params, " \"eq_{n}:{port}\" {}", num(value));
        }
    }
    format!("{{ params = [{params} ] }}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Chain {
        [Coefficients::IDENTITY; SLOTS]
    }

    #[test]
    fn config_chains_every_slot_and_targets_the_game_output() {
        let conf = config("test_eq", &Target::quantum_810(), &identity());
        assert_eq!(conf.matches("label = bq_raw").count(), SLOTS);
        assert_eq!(conf.matches("output = \"eq_").count(), SLOTS - 1);
        assert!(conf.contains("{ output = \"eq_15:Out\" input = \"eq_16:In\" }"));
        assert!(conf.contains(
            "filter.smart.target = { alsa.card_name = \"JBL Quantum810 Wireless\" device.profile.name = \"stereo-game-output\" }"
        ));
        assert!(conf.contains("node.name = \"test_eq.output\""));
    }

    #[test]
    fn props_set_all_six_coefficients_of_every_slot() {
        let p = props(&identity());
        assert_eq!(p.matches("\"eq_").count(), SLOTS * 6);
        assert!(p.starts_with("{ params = [ \"eq_1:b0\" 1.000000000000000"));
    }

    #[test]
    fn numbers_never_use_exponent_notation() {
        assert_eq!(num(1e-9), "0.000000001000000");
        assert!(!num(-1.999_999_999).contains('e'));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use std::time::Duration;

    /// Run with `cargo test -- --ignored`. Matches no device, so nothing is
    /// ever routed through it and it never becomes the default output.
    #[test]
    #[ignore = "starts a real PipeWire filter in the current session"]
    fn filter_starts_takes_new_coefficients_and_goes_away() {
        let nowhere = Target { card: "quantumengine-test-matches-nothing".into(), profile: "none".into() };
        let name = "quantumengine_eq_test";
        let mut filter = Filter::start_named(name, &nowhere, &[Coefficients::IDENTITY; SLOTS]).unwrap();
        assert!(filter.is_running());

        let mut chain = [Coefficients::IDENTITY; SLOTS];
        chain[0] = Coefficients::IDENTITY.scaled(0.5);
        filter.set(&chain).unwrap();

        let id = node_id(name).unwrap().expect("the test filter's node");
        let out = Command::new("pw-cli").args(["enum-params", &id.to_string(), "Props"]).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let after = text.split("\"eq_1:b0\"").nth(1).expect("eq_1:b0 in the node's Props");
        let value: f64 = after.split_whitespace().nth(1).unwrap().parse().unwrap();
        assert!((value - 0.5).abs() < 1e-6, "eq_1:b0 reads {value}");

        drop(filter);
        std::thread::sleep(Duration::from_millis(500));
        assert!(node_id(name).unwrap().is_none());
    }
}
