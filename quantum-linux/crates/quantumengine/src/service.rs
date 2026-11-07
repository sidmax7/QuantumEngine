//! The connection to `quantumengined`: finds it (starting it if needed),
//! keeps [`StateDto`] current, and pushes every change to the UI as a
//! [`StateEvent`].
//!
//! State is read with one `GetAll` call a few times a second. The service
//! answers properties from memory, so this costs it nothing, and unlike a
//! `PropertiesChanged` subscription it also notices the service going away.

use crate::dto::{EqDto, HeadsetDto, ModelDto, Service, StateDto, StateEvent, SurroundDto, UnverifiedDto, UpdateDto, VolumeDto};
use quantumengine_dbus::HeadsetProxyBlocking;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_specta::Event;
use zbus::blocking::fdo::{DBusProxy, PropertiesProxy};
use zbus::blocking::Connection;
use zbus::zvariant::OwnedValue;

const POLL_EVERY: Duration = Duration::from_millis(200);
const RECONNECT_EVERY: Duration = Duration::from_secs(1);
/// How long to wait for a `quantumengined` this window started to claim its
/// bus name.
const START_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest one D-Bus call may go unanswered.
const CALL_TIMEOUT: Duration = Duration::from_secs(20);

/// What the commands share with the watcher thread.
#[derive(Default)]
pub struct Link {
    state: Mutex<StateDto>,
    connection: Mutex<Option<Connection>>,
}

impl Link {
    pub fn state(&self) -> StateDto {
        self.state.lock().unwrap().clone()
    }

    /// The bus connection to the service, or why there isn't one.
    pub fn connection(&self) -> Result<Connection, String> {
        self.connection.lock().unwrap().clone().ok_or_else(|| "The background service isn't running.".into())
    }

    /// A proxy for one call, or why there isn't one.
    pub fn proxy(&self) -> Result<HeadsetProxyBlocking<'static>, String> {
        let conn = self.connection()?;
        HeadsetProxyBlocking::builder(&conn)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .map_err(|e| format!("Cannot reach the background service: {e}"))
    }
}

/// Starts the watcher thread. It runs for the life of the app.
pub fn watch(app: AppHandle, link: Arc<Link>) {
    std::thread::Builder::new()
        .name("service".into())
        .spawn(move || loop {
            match connect() {
                Ok(conn) => {
                    *link.connection.lock().unwrap() = Some(conn.clone());
                    let reason = follow(&app, &link, &conn);
                    *link.connection.lock().unwrap() = None;
                    publish(&app, &link, |s| {
                        s.service = Service::Down;
                        s.service_error = reason;
                    });
                }
                Err(e) => publish(&app, &link, |s| {
                    s.service = Service::Down;
                    s.service_error = e;
                }),
            }
            std::thread::sleep(RECONNECT_EVERY);
        })
        .expect("failed to start the service thread");
}

/// Updates the state and tells the UI, if anything changed.
fn publish(app: &AppHandle, link: &Link, change: impl FnOnce(&mut StateDto)) {
    let mut state = link.state.lock().unwrap();
    let before = state.clone();
    change(&mut state);
    if *state != before {
        drop(StateEvent(state.clone()).emit(app));
    }
}

/// Connects to the session bus and makes sure the service is on it.
fn connect() -> Result<Connection, String> {
    // A call the service never answers must not hang this thread (or a
    // command) for good; see `quantumengined`'s `dbus_service::claim`.
    let conn = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.method_timeout(CALL_TIMEOUT).build())
        .map_err(|e| format!("Cannot reach the session bus: {e}"))?;
    let bus = DBusProxy::new(&conn).map_err(|e| format!("Cannot reach the session bus: {e}"))?;
    let name = || quantumengine_dbus::BUS_NAME.try_into().expect("a valid bus name");
    if !bus.name_has_owner(name()).unwrap_or(false) {
        start_service()?;
        let deadline = Instant::now() + START_TIMEOUT;
        while !bus.name_has_owner(name()).unwrap_or(false) {
            if Instant::now() >= deadline {
                return Err("The background service (quantumengined) didn't start.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(conn)
}

/// Polls the service until it stops answering; returns why.
fn follow(app: &AppHandle, link: &Link, conn: &Connection) -> String {
    let props = match PropertiesProxy::builder(conn)
        .destination(quantumengine_dbus::BUS_NAME)
        .and_then(|b| b.path(quantumengine_dbus::OBJECT_PATH))
        .and_then(|b| b.cache_properties(zbus::proxy::CacheProperties::No).build())
    {
        Ok(props) => props,
        Err(e) => return format!("Cannot reach the background service: {e}"),
    };
    let iface = zbus::names::InterfaceName::try_from(quantumengine_dbus::BUS_NAME).expect("a valid interface name");
    loop {
        match props.get_all(iface.clone()) {
            Ok(all) => {
                let fresh = read(all);
                publish(app, link, |s| *s = fresh);
            }
            Err(e) => return format!("Lost the background service: {e}"),
        }
        std::thread::sleep(POLL_EVERY);
    }
}

fn read(mut all: HashMap<String, OwnedValue>) -> StateDto {
    let mut take = |name: &str| all.remove(name);
    let service_version: String = get(take("Version"));
    let model_id: String = get(take("ModelId"));
    let unverified_id: String = get(take("UnverifiedDevice"));
    StateDto {
        service: Service::Up,
        service_error: String::new(),
        // Services before 0.3.0 don't report a version at all.
        service_outdated: older(&service_version, env!("CARGO_PKG_VERSION")),
        headset: HeadsetDto {
            link: get(take("Link")),
            link_detail: get(take("LinkDetail")),
            battery: get(take("Battery")),
            anc: get(take("Anc")),
            sidetone: get(take("Sidetone")),
            mic_active: get(take("MicActive")),
            dial: get(take("Dial")),
            lights_on: get(take("LightsOn")),
            device_name: get(take("DeviceName")),
            serial: get(take("Serial")),
            firmware: get(take("Firmware")),
            busy: get(take("Busy")),
            last_error: get(take("LastError")),
        },
        eq: EqDto {
            enabled: get(take("EqEnabled")),
            profile: get(take("EqProfile")),
            profiles: get(take("EqProfiles")),
            correction: get(take("EqCorrection")),
            status: get(take("EqStatus")),
            custom: get(take("EqCustomProfiles")),
            previewing: get(take("EqPreviewing")),
        },
        surround: SurroundDto { enabled: get(take("SurroundEnabled")), status: get(take("SurroundStatus")) },
        volume: VolumeDto { available: get(take("VolumeAvailable")), percent: get(take("Volume")) },
        model: ModelDto::lookup(&model_id),
        unverified: UnverifiedDto {
            model: ModelDto::lookup(&unverified_id),
            id: unverified_id,
            name: get(take("UnverifiedName")),
            access: get(take("UnverifiedAccess")),
            in_use: get(take("UnverifiedInUse")),
            allowed: get(take("UnverifiedAllowed")),
        },
        update: UpdateDto {
            service_version,
            available: get(take("UpdateVersion")),
            can_install: get(take("UpdateCanInstall")),
            status: get(take("UpdateStatus")),
            busy: get(take("UpdateBusy")),
            checks: get(take("UpdateChecks")),
        },
    }
}

/// Whether `version` is before `current`; an empty or unreadable one is.
fn older(version: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|n| n.parse().ok()).collect() };
    match (parse(version), parse(current)) {
        (Some(v), Some(c)) => v < c,
        _ => true,
    }
}

/// Stops the running service so the watcher starts the one installed now.
/// Only ever stops a QuantumEngine process: the bus name's owner is checked
/// by its executable's name first.
pub fn restart(link: &Link) -> Result<(), String> {
    let conn = link.connection()?;
    let bus = DBusProxy::new(&conn).map_err(|e| e.to_string())?;
    let name = || -> zbus::names::BusName<'static> { quantumengine_dbus::BUS_NAME.try_into().expect("a valid bus name") };
    let pid = bus.get_connection_unix_process_id(name()).map_err(|e| format!("Cannot find the background service: {e}"))?;
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).map_err(|e| format!("Cannot inspect the background service: {e}"))?;
    let file = exe.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    if !file.trim_end_matches(" (deleted)").starts_with("quantumengine") {
        return Err(format!("The process holding the service name is {file}, not QuantumEngine; leaving it alone."));
    }
    let killed = std::process::Command::new("kill").arg(pid.to_string()).status().map_err(|e| e.to_string())?;
    if !killed.success() {
        return Err("Couldn't stop the old background service.".into());
    }
    let deadline = Instant::now() + START_TIMEOUT;
    while bus.name_has_owner(name()).unwrap_or(false) {
        if Instant::now() >= deadline {
            return Err("The old background service didn't stop.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // The watcher notices the service is gone and starts the current one.
    Ok(())
}

/// A property's value, or its type's default if it is missing or of another
/// type (an older service, for instance).
fn get<T: TryFrom<OwnedValue> + Default>(value: Option<OwnedValue>) -> T {
    value.and_then(|v| T::try_from(v).ok()).unwrap_or_default()
}

/// Starts `quantumengined`, detached: the one next to this executable when
/// there is one (a development build), else whichever is on `PATH`.
///
/// Its stdio is not inherited: the service outlives this window, and a pipe
/// it held open would leave whoever reads this window's output waiting
/// forever.
fn start_service() -> Result<(), String> {
    use std::process::{Command, Stdio};
    let sibling = std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("quantumengined"))
        .filter(|p| p.is_file());
    let program = sibling.unwrap_or_else(|| "quantumengined".into());
    Command::new(&program)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("Cannot start the background service ({}): {e}", program.display()))
}

#[cfg(test)]
mod tests {
    use super::older;

    #[test]
    fn versions_compare_numerically() {
        assert!(older("", "0.3.0"));
        assert!(older("0.2.1", "0.3.0"));
        assert!(older("0.9.0", "0.10.0"));
        assert!(!older("0.3.0", "0.3.0"));
        assert!(!older("0.3.1", "0.3.0"));
    }
}
