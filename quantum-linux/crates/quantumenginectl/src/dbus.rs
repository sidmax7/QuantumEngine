//! Connects `quantumenginectl` to the `quantumengine --tray` service over
//! D-Bus, starting it if it is not already running. Like the window and the
//! tray icon, this CLI never opens the headset itself — see
//! `quantumengine-dbus` for the shared contract.

use quantumengine_dbus::HeadsetProxyBlocking;
use std::time::{Duration, Instant};

/// How long to wait for a `quantumengine --tray` this module started itself
/// to claim the bus name.
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(5);

/// Connects to the running service, or starts one and waits for it.
pub fn connect() -> Result<HeadsetProxyBlocking<'static>, String> {
    let conn = zbus::blocking::Connection::session()
        .map_err(|e| format!("cannot reach the session bus: {e}"))?;
    let bus = zbus::blocking::fdo::DBusProxy::new(&conn)
        .map_err(|e| format!("cannot reach the session bus: {e}"))?;
    let name = || quantumengine_dbus::BUS_NAME.try_into().expect("a valid bus name");
    if !bus.name_has_owner(name()).unwrap_or(false) {
        start_service()?;
        let deadline = Instant::now() + SERVICE_START_TIMEOUT;
        while !bus.name_has_owner(name()).unwrap_or(false) {
            if Instant::now() >= deadline {
                return Err("quantumengine --tray did not start in time".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    // No property cache: a one-shot CLI invocation reads each property at
    // most once anyway, so there is nothing for a cache to save.
    HeadsetProxyBlocking::builder(&conn)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .map_err(|e| format!("cannot reach the headset service: {e}"))
}

/// Starts `quantumengine --tray`, found next to this executable — a dev
/// build and the packaged install both put both binaries in the same
/// directory.
///
/// Its stdio is detached (not inherited) rather than left to default: the
/// service outlives this one-shot CLI invocation, so if it inherited, say, a
/// pipe this command's own output was captured into, that pipe would never
/// see EOF and whatever is reading it would hang forever waiting for a
/// process that has no reason to exit.
fn start_service() -> Result<(), String> {
    let mut exe = std::env::current_exe().map_err(|e| format!("cannot find my own executable: {e}"))?;
    exe.set_file_name("quantumengine");
    std::process::Command::new(&exe)
        .arg("--tray")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))
}
