//! Runs a PipeWire filter-chain in a child `pipewire -c` process, and the few
//! questions about the PipeWire graph the EQ and surround need answered
//! (`pw-dump`, `wpctl`). Both features are one of these graphs.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const START_TIMEOUT: Duration = Duration::from_secs(5);

/// A running `pipewire -c <conf>` whose filter-chain made a node called
/// `node_name`. Dropping it stops the process and the node goes away.
pub struct Graph {
    child: Child,
    node_name: String,
}

impl Graph {
    /// Writes `conf`, starts it and waits for its node. `what` names it in
    /// errors, e.g. "the PipeWire EQ filter".
    pub fn start(node_name: &str, what: &str, conf: &str) -> Result<Self, String> {
        let dir = runtime_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let conf_path = dir.join(format!("{node_name}.conf"));
        let log_path = dir.join(format!("{node_name}.log"));
        std::fs::write(&conf_path, conf).map_err(|e| format!("cannot write {}: {e}", conf_path.display()))?;
        let log = std::fs::File::create(&log_path).map_err(|e| format!("cannot write {}: {e}", log_path.display()))?;

        let mut cmd = Command::new("pipewire");
        cmd.arg("-c").arg(&conf_path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
        // SAFETY: prctl is async-signal-safe and touches no memory of ours.
        // It ties the graph's lifetime to the thread that started it, so a
        // crashed or killed service never leaves an orphan filter running.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(|e| format!("cannot start pipewire: {e}"))?;
        let mut graph = Self { child, node_name: node_name.to_owned() };

        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if let Ok(Some(status)) = graph.child.try_wait() {
                let log = std::fs::read_to_string(&log_path).unwrap_or_default();
                let last = log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no output");
                return Err(format!("{what} stopped ({status}): {last}"));
            }
            if node_id(node_name)?.is_some() {
                return Ok(graph);
            }
            if Instant::now() >= deadline {
                return Err(format!("{what} did not appear in time"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn node_name(&self) -> &str {
        &self.node_name
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Graph {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn runtime_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("quantumengine"),
        None => std::env::temp_dir().join(format!("quantumengine-{}", unsafe { libc::getuid() })),
    }
}

/// Every object in the PipeWire graph, as `pw-dump` prints them.
pub fn dump() -> Result<Vec<serde_json::Value>, String> {
    let out = Command::new("pw-dump")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run pw-dump: {e}"))?;
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cannot read pw-dump output: {e}"))
}

/// The first node whose properties satisfy `matches`, as (id, node.name).
pub fn find_node(matches: impl Fn(&serde_json::Value) -> bool) -> Result<Option<(u64, String)>, String> {
    Ok(dump()?.iter().find_map(|o| {
        let props = o.pointer("/info/props")?;
        if o["type"] != "PipeWire:Interface:Node" || !matches(props) {
            return None;
        }
        Some((o["id"].as_u64()?, props["node.name"].as_str()?.to_owned()))
    }))
}

/// The id of the node called `node_name`, if it exists.
pub fn node_id(node_name: &str) -> Result<Option<u64>, String> {
    Ok(find_node(|p| p["node.name"] == node_name)?.map(|(id, _)| id))
}

/// The node name of the current default output, if there is one.
pub fn default_sink() -> Result<Option<String>, String> {
    Ok(dump()?.iter().find_map(|o| {
        if o["type"] != "PipeWire:Interface:Metadata" || o.pointer("/props/metadata.name")? != "default" {
            return None;
        }
        o["metadata"].as_array()?.iter().find_map(|m| {
            (m["key"] == "default.audio.sink").then(|| m.pointer("/value/name")?.as_str().map(str::to_owned))?
        })
    }))
}

/// Makes node `id` the default output, as choosing it in the desktop's sound
/// settings would. WirePlumber remembers it by name from then on.
pub fn set_default(id: u64) -> Result<(), String> {
    let out = Command::new("wpctl")
        .args(["set-default", &id.to_string()])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run wpctl: {e}"))?;
    if !out.status.success() {
        return Err(format!("wpctl set-default failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

/// A string in PipeWire's config syntax.
pub fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
