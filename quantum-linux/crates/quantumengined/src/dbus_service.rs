//! The D-Bus service `quantumengined` runs. See `quantumengine-dbus`
//! for the interface contract this implements.
//!
//! [`HeadsetIface`] never touches the headset directly: property getters
//! read the cache the [`device`] worker thread keeps up to date, and the
//! `Set*` methods send a [`device::Command`] down the same channel the
//! window would, then block on its reply. That keeps every caller — this
//! service's own D-Bus clients, however many there are — funnelled through
//! the one thread that is allowed to talk to the dongle.

use crate::device::{self, Link, Shared};
use crate::eq::{self, profiles::Correction};
use crate::surround;
use crate::update;
use jbl_quantum::{Anc, Colour, Effect, Sidetone, Zone, ZoneLighting, MAX_SLOTS};
use quantumengine_dbus::{EqBand, ZoneLightingArg};
use std::sync::mpsc::{self, Sender};
use zbus::fdo;
use zbus::interface;

pub struct HeadsetIface {
    shared: Shared,
    commands: Sender<device::Command>,
    eq: eq::Handle,
    surround: surround::Handle,
    updater: update::Handle,
    notifier: crate::notifier::Notifier,
}

#[interface(name = "org.quantumengine.Headset1")]
impl HeadsetIface {
    #[zbus(property)]
    fn link(&self) -> String {
        link_str(&self.shared.lock().unwrap().link)
    }

    #[zbus(property)]
    fn link_detail(&self) -> String {
        match &self.shared.lock().unwrap().link {
            Link::NoAccess(msg) => msg.clone(),
            _ => String::new(),
        }
    }

    #[zbus(property)]
    fn battery(&self) -> u8 {
        self.shared.lock().unwrap().battery.unwrap_or(0)
    }

    #[zbus(property)]
    fn anc(&self) -> String {
        self.shared.lock().unwrap().anc.map_or("off", Anc::as_str).to_string()
    }

    #[zbus(property)]
    fn sidetone(&self) -> String {
        self.shared.lock().unwrap().sidetone.map_or("off", Sidetone::as_str).to_string()
    }

    #[zbus(property)]
    fn mic_active(&self) -> bool {
        self.shared.lock().unwrap().mic_active.unwrap_or(false)
    }

    #[zbus(property)]
    fn dial(&self) -> u8 {
        self.shared.lock().unwrap().dial.unwrap_or(8)
    }

    #[zbus(property)]
    fn lights_on(&self) -> bool {
        self.shared.lock().unwrap().lights_on.unwrap_or(false)
    }

    #[zbus(property)]
    fn device_name(&self) -> String {
        self.shared.lock().unwrap().device_name.clone()
    }

    #[zbus(property)]
    fn serial(&self) -> String {
        self.shared.lock().unwrap().serial.clone()
    }

    #[zbus(property)]
    fn firmware(&self) -> Vec<String> {
        self.shared.lock().unwrap().firmware.clone()
    }

    #[zbus(property)]
    fn busy(&self) -> bool {
        self.shared.lock().unwrap().pending.is_some()
    }

    #[zbus(property)]
    fn last_error(&self) -> String {
        self.shared.lock().unwrap().error.clone().unwrap_or_default()
    }

    fn set_anc(&self, mode: &str) -> fdo::Result<String> {
        let mode = parse_anc(mode)?;
        let (reply, rx) = mpsc::channel();
        self.dispatch(device::Command::Anc(mode, reply))?;
        recv(rx).map(|m| m.as_str().to_string())
    }

    fn set_sidetone(&self, level: &str) -> fdo::Result<String> {
        let level = parse_sidetone(level)?;
        let (reply, rx) = mpsc::channel();
        self.dispatch(device::Command::Sidetone(level, reply))?;
        recv(rx).map(|l| l.as_str().to_string())
    }

    fn set_lights(&self, on: bool) -> fdo::Result<bool> {
        let (reply, rx) = mpsc::channel();
        self.dispatch(device::Command::Lights(on, reply))?;
        recv(rx)
    }

    fn set_lighting(&self, zones: Vec<ZoneLightingArg>) -> fdo::Result<()> {
        let mut zones = zones.into_iter().map(parse_zone_lighting).collect::<fdo::Result<Vec<_>>>()?;
        // Only the zones this model has, as JBL's own app sends them: a
        // Quantum 400 has a logo and no ring.
        let model = jbl_quantum::models::lookup_id(&self.shared.lock().unwrap().model_id);
        if let Some(m) = model.filter(|m| !m.zones.is_empty()) {
            zones.retain(|z| m.zones.iter().any(|lz| lz.as_str() == z.zone.as_str()));
            if zones.is_empty() {
                return Err(fdo::Error::InvalidArgs(format!("the {} has no such light", m.name)));
            }
        }
        let (reply, rx) = mpsc::channel();
        self.dispatch(device::Command::Lighting(zones, reply))?;
        recv(rx)
    }

    #[zbus(property)]
    fn model_id(&self) -> String {
        self.shared.lock().unwrap().model_id.clone()
    }

    #[zbus(property)]
    fn unverified_device(&self) -> String {
        self.unverified(|u| u.id.clone())
    }

    #[zbus(property)]
    fn unverified_name(&self) -> String {
        self.unverified(|u| u.name.clone())
    }

    #[zbus(property)]
    fn unverified_access(&self) -> String {
        // A request for access in progress (or that failed) explains more than
        // the plain "cannot open".
        let asked = device::grant_status();
        if asked.is_empty() { self.unverified(|u| u.access.clone()) } else { asked }
    }

    #[zbus(property)]
    fn unverified_in_use(&self) -> bool {
        self.unverified(|u| u.in_use)
    }

    #[zbus(property)]
    fn unverified_allowed(&self) -> Vec<String> {
        self.unverified(|u| u.allowed.clone())
    }

    fn probe_unverified(&self) -> fdo::Result<String> {
        let (reply, rx) = mpsc::channel();
        self.dispatch(device::Command::Probe(reply))?;
        recv(rx)
    }

    fn set_unverified(&self, in_use: bool, allowed: Vec<String>) -> fdo::Result<()> {
        let (reply, rx) = mpsc::channel();
        self.dispatch(device::Command::SetUnverified(in_use, allowed, reply))?;
        recv(rx)
    }

    /// Asks for access to the unverified device. The password prompt can take
    /// as long as the user does, so it runs on its own thread and this
    /// returns at once; its progress is `UnverifiedAccess`.
    fn grant_unverified_access(&self) -> fdo::Result<()> {
        let id = self.unverified(|u| u.id.clone());
        let (vid, pid) = jbl_quantum::access::parse_id(&id)
            .ok_or_else(|| fdo::Error::Failed("no unverified model is plugged in".into()))?;
        if !device::grant_status().is_empty() && device::grant_status().starts_with("Waiting") {
            return Ok(());
        }
        device::set_grant_status("Waiting for your password…");
        self.notifier.notify();
        let notifier = self.notifier.clone();
        std::thread::spawn(move || {
            match jbl_quantum::access::grant(vid, pid) {
                // The new rule applies to the device already plugged in; the
                // worker's next look finds it open.
                Ok(()) => device::set_grant_status(""),
                Err(e) => device::set_grant_status(format!("Couldn't get access: {e}")),
            }
            notifier.notify();
        });
        Ok(())
    }

    #[zbus(property)]
    fn eq_enabled(&self) -> bool {
        self.eq.state().enabled
    }

    #[zbus(property)]
    fn eq_profile(&self) -> String {
        self.eq.state().profile.to_string()
    }

    #[zbus(property)]
    fn eq_profiles(&self) -> Vec<String> {
        self.eq.state().profile_names()
    }

    #[zbus(property)]
    fn eq_custom_profiles(&self) -> Vec<String> {
        self.eq.state().custom.into_iter().map(|p| p.name).collect()
    }

    #[zbus(property)]
    fn eq_previewing(&self) -> bool {
        self.eq.state().previewing
    }

    #[zbus(property)]
    fn eq_correction(&self) -> String {
        self.eq.state().correction.as_str().to_string()
    }

    #[zbus(property)]
    fn eq_status(&self) -> String {
        self.eq.state().status
    }

    fn set_eq_enabled(&self, enabled: bool) -> fdo::Result<bool> {
        self.eq.call(|r| eq::Command::Enable(enabled, r)).map_err(fdo::Error::Failed)
    }

    fn set_eq_profile(&self, name: &str) -> fdo::Result<String> {
        if self.eq.state().find(name).is_none() {
            return Err(fdo::Error::InvalidArgs(format!("unknown EQ profile '{name}'")));
        }
        let name = name.to_string();
        self.eq.call(|r| eq::Command::Profile(name, r)).map_err(fdo::Error::Failed)
    }

    fn eq_response(&self, profile: &str, correction: &str) -> fdo::Result<Vec<f64>> {
        let (_, bands) = self
            .eq
            .state()
            .find(profile)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown EQ profile '{profile}'")))?;
        response(&bands, correction)
    }

    fn eq_profile_bands(&self, name: &str) -> fdo::Result<Vec<EqBand>> {
        let (_, bands) =
            self.eq.state().find(name).ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown EQ profile '{name}'")))?;
        Ok(bands.iter().map(band_arg).collect())
    }

    fn eq_bands_response(&self, bands: Vec<EqBand>, correction: &str) -> fdo::Result<Vec<f64>> {
        let bands = parse_bands(bands)?;
        eq::custom::check_bands(&bands).map_err(fdo::Error::InvalidArgs)?;
        response(&bands, correction)
    }

    fn save_eq_profile(&self, original: &str, name: &str, bands: Vec<EqBand>) -> fdo::Result<String> {
        let bands = parse_bands(bands)?;
        let original = (!original.is_empty()).then(|| original.to_string());
        let name = name.to_string();
        self.eq
            .call(|reply| eq::Command::Save { original, name, bands, reply })
            .map_err(fdo::Error::InvalidArgs)
    }

    fn delete_eq_profile(&self, name: &str) -> fdo::Result<()> {
        let name = name.to_string();
        self.eq.call(|r| eq::Command::Delete(name, r)).map_err(fdo::Error::InvalidArgs)
    }

    fn preview_eq_bands(&self, bands: Vec<EqBand>) -> fdo::Result<()> {
        let bands = parse_bands(bands)?;
        self.eq.call(|r| eq::Command::Preview(Some(bands), r)).map_err(fdo::Error::InvalidArgs)
    }

    fn end_eq_preview(&self) -> fdo::Result<()> {
        self.eq.call(|r| eq::Command::Preview(None, r)).map_err(fdo::Error::Failed)
    }

    fn import_eq_profile(&self, name: &str, text: &str) -> fdo::Result<String> {
        let (name, text) = (name.to_string(), text.to_string());
        self.eq.call(|reply| eq::Command::Import { name, text, reply }).map_err(fdo::Error::InvalidArgs)
    }

    #[zbus(property)]
    fn surround_enabled(&self) -> bool {
        self.surround.state().enabled
    }

    #[zbus(property)]
    fn surround_status(&self) -> String {
        self.surround.state().status
    }

    fn set_surround_enabled(&self, enabled: bool) -> fdo::Result<bool> {
        self.surround.call(|r| surround::Command::Enable(enabled, r)).map_err(fdo::Error::Failed)
    }

    fn set_eq_correction(&self, correction: &str) -> fdo::Result<String> {
        let c = parse_correction(correction)?;
        self.eq
            .call(|r| eq::Command::Correction(c, r))
            .map(|c| c.as_str().to_string())
            .map_err(fdo::Error::Failed)
    }

    #[zbus(property)]
    fn version(&self) -> String {
        update::VERSION.to_string()
    }

    #[zbus(property)]
    fn update_version(&self) -> String {
        self.updater.state().available.map(|r| r.version).unwrap_or_default()
    }

    #[zbus(property)]
    fn update_url(&self) -> String {
        self.updater.state().available.map(|r| r.url).unwrap_or_default()
    }

    #[zbus(property)]
    fn update_can_install(&self) -> bool {
        self.updater.state().can_install()
    }

    #[zbus(property)]
    fn update_status(&self) -> String {
        self.updater.state().status
    }

    #[zbus(property)]
    fn update_busy(&self) -> bool {
        self.updater.state().busy
    }

    #[zbus(property)]
    fn update_checks(&self) -> bool {
        self.updater.state().checks_enabled
    }

    fn check_for_updates(&self) -> fdo::Result<String> {
        self.updater
            .call(update::Command::Check)
            .map(Option::unwrap_or_default)
            .map_err(fdo::Error::Failed)
    }

    fn install_update(&self) -> fdo::Result<()> {
        self.updater.call(update::Command::Install).map_err(fdo::Error::Failed)
    }

    fn set_update_checks(&self, enabled: bool) -> fdo::Result<bool> {
        self.updater.call(|r| update::Command::SetChecks(enabled, r)).map_err(fdo::Error::Failed)
    }
}

impl HeadsetIface {
    /// Reads something about the unverified model, or the type's default when
    /// there isn't one.
    fn unverified<T: Default>(&self, f: impl FnOnce(&device::Unverified) -> T) -> T {
        self.shared.lock().unwrap().unverified.as_ref().map(f).unwrap_or_default()
    }

    fn dispatch(&self, cmd: device::Command) -> fdo::Result<()> {
        self.commands
            .send(cmd)
            .map_err(|_| fdo::Error::Failed("the headset worker has stopped".into()))
    }
}

/// Wait for a command's outcome and turn it into the D-Bus error `Set*`
/// methods return on failure.
fn recv<T>(rx: mpsc::Receiver<Result<T, jbl_quantum::Error>>) -> fdo::Result<T> {
    match rx.recv() {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(fdo::Error::Failed(e.to_string())),
        Err(_) => Err(fdo::Error::Failed("no reply from the headset worker".into())),
    }
}

fn parse_anc(s: &str) -> fdo::Result<Anc> {
    match s {
        "off" => Ok(Anc::Off),
        "on" => Ok(Anc::On),
        "talkthru" => Ok(Anc::TalkThru),
        other => Err(fdo::Error::InvalidArgs(format!(
            "unknown ANC mode '{other}' (want off, on or talkthru)"
        ))),
    }
}

fn parse_sidetone(s: &str) -> fdo::Result<Sidetone> {
    match s {
        "off" => Ok(Sidetone::Off),
        "low" => Ok(Sidetone::Low),
        "mid" => Ok(Sidetone::Mid),
        "high" => Ok(Sidetone::High),
        other => Err(fdo::Error::InvalidArgs(format!(
            "unknown sidetone level '{other}' (want off, low, mid or high)"
        ))),
    }
}

fn parse_zone_lighting(arg: ZoneLightingArg) -> fdo::Result<ZoneLighting> {
    let zone = match arg.zone.as_str() {
        "logo" => Zone::Logo,
        "ring" => Zone::Ring,
        other => {
            return Err(fdo::Error::InvalidArgs(format!("unknown zone '{other}' (want logo or ring)")))
        }
    };
    if arg.colours.is_empty() || arg.colours.len() > MAX_SLOTS {
        return Err(fdo::Error::InvalidArgs(format!(
            "palette must have 1 to {MAX_SLOTS} colours, got {}",
            arg.colours.len()
        )));
    }
    if arg.brightness > 100 {
        return Err(fdo::Error::InvalidArgs(format!("brightness must be 0-100, got {}", arg.brightness)));
    }
    if arg.speed > 100 {
        return Err(fdo::Error::InvalidArgs(format!("speed must be 0-100, got {}", arg.speed)));
    }
    let effect = Effect(arg.effect);
    if !effect.is_known() {
        return Err(fdo::Error::InvalidArgs(format!("unknown effect {} (want 0-6)", arg.effect)));
    }
    if !effect.suits(zone) {
        return Err(fdo::Error::InvalidArgs(format!(
            "{} only runs on the ring, not the logo",
            effect.name().unwrap_or("that effect")
        )));
    }
    Ok(ZoneLighting {
        zone,
        brightness: arg.brightness,
        speed: arg.speed,
        palette: arg.colours.iter().map(|&(r, g, b)| Colour::new(r, g, b)).collect(),
        effect,
    })
}

fn link_str(link: &Link) -> String {
    match link {
        Link::NoDongle => "no_dongle",
        Link::NoAccess(_) => "no_access",
        Link::Unverified => "unverified",
        Link::HeadsetOff => "headset_off",
        Link::Ready => "ready",
    }
    .to_string()
}

/// Claims [`quantumengine_dbus::BUS_NAME`] on the session bus.
///
/// Returns `None` when another service already owns the name. The claim is
/// made without replacement on purpose: zbus's default flags would let a
/// second instance take the name over and leave the first running as a
/// duplicate tray icon that still holds the dongle open. The caller must then
/// exit before it starts anything else — a tray icon, the EQ filter, or the
/// headset worker.
///
/// The connection must be kept alive for as long as the service runs;
/// dropping it releases the name.
pub fn claim() -> zbus::Result<Option<zbus::blocking::Connection>> {
    let connection = zbus::blocking::Connection::session()?;
    // Start answering calls before taking the name. zbus only begins
    // dispatching once the object server is first touched, and the interface
    // is registered a moment later (after the EQ, surround and updater start);
    // a call that arrived in between used to get no reply at all, and a
    // client without a timeout (the widget's first `quantumenginectl status`
    // at login) waited forever. Now such a call gets an immediate "unknown
    // object" error, which clients retry.
    connection.object_server();
    match connection.request_name_with_flags(
        quantumengine_dbus::BUS_NAME,
        fdo::RequestNameFlags::DoNotQueue.into(),
    ) {
        Ok(fdo::RequestNameReply::PrimaryOwner | fdo::RequestNameReply::AlreadyOwner) => Ok(Some(connection)),
        // zbus reports a taken name as an error rather than an `Exists` reply.
        Ok(_) | Err(zbus::Error::NameTaken) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Serves the interface at [`quantumengine_dbus::OBJECT_PATH`] on a
/// connection from [`claim`], reading from `shared`, `eq` and `updater` and
/// sending to their command channels — all set up by the caller, which owns
/// the threads that drain them (see `main::run_tray`).
///
/// Returns a callback the caller must invoke whenever anything the interface
/// exposes changes. It emits a `PropertiesChanged` for every property — a
/// broad brush rather than diffing field by field, but changes only happen on
/// user action, a physical control or the updater, so the traffic is small.
pub fn serve(
    connection: &zbus::blocking::Connection,
    shared: Shared,
    commands: Sender<device::Command>,
    eq: eq::Handle,
    surround: surround::Handle,
    updater: update::Handle,
    notifier: crate::notifier::Notifier,
) -> zbus::Result<impl Fn() + Send + Sync + use<>> {
    connection
        .object_server()
        .at(quantumengine_dbus::OBJECT_PATH, HeadsetIface { shared, commands, eq, surround, updater, notifier })?;

    let iface_ref = connection
        .object_server()
        .interface::<_, HeadsetIface>(quantumengine_dbus::OBJECT_PATH)?;

    let notify_changed = move || {
        let iface = iface_ref.get();
        let emitter = iface_ref.signal_emitter();
        // Best-effort: a failure here means a client missed one update, not
        // that the service is broken, so log-and-continue rather than panic.
        for result in [
            zbus::block_on(iface.link_changed(emitter)),
            zbus::block_on(iface.link_detail_changed(emitter)),
            zbus::block_on(iface.battery_changed(emitter)),
            zbus::block_on(iface.anc_changed(emitter)),
            zbus::block_on(iface.sidetone_changed(emitter)),
            zbus::block_on(iface.mic_active_changed(emitter)),
            zbus::block_on(iface.dial_changed(emitter)),
            zbus::block_on(iface.lights_on_changed(emitter)),
            zbus::block_on(iface.device_name_changed(emitter)),
            zbus::block_on(iface.serial_changed(emitter)),
            zbus::block_on(iface.firmware_changed(emitter)),
            zbus::block_on(iface.busy_changed(emitter)),
            zbus::block_on(iface.last_error_changed(emitter)),
            zbus::block_on(iface.model_id_changed(emitter)),
            zbus::block_on(iface.unverified_device_changed(emitter)),
            zbus::block_on(iface.unverified_name_changed(emitter)),
            zbus::block_on(iface.unverified_access_changed(emitter)),
            zbus::block_on(iface.unverified_in_use_changed(emitter)),
            zbus::block_on(iface.unverified_allowed_changed(emitter)),
            zbus::block_on(iface.eq_enabled_changed(emitter)),
            zbus::block_on(iface.eq_profiles_changed(emitter)),
            zbus::block_on(iface.eq_custom_profiles_changed(emitter)),
            zbus::block_on(iface.eq_previewing_changed(emitter)),
            zbus::block_on(iface.eq_profile_changed(emitter)),
            zbus::block_on(iface.eq_correction_changed(emitter)),
            zbus::block_on(iface.eq_status_changed(emitter)),
            zbus::block_on(iface.surround_enabled_changed(emitter)),
            zbus::block_on(iface.surround_status_changed(emitter)),
            zbus::block_on(iface.update_version_changed(emitter)),
            zbus::block_on(iface.update_url_changed(emitter)),
            zbus::block_on(iface.update_can_install_changed(emitter)),
            zbus::block_on(iface.update_status_changed(emitter)),
            zbus::block_on(iface.update_busy_changed(emitter)),
            zbus::block_on(iface.update_checks_changed(emitter)),
        ] {
            if let Err(e) = result {
                eprintln!("quantumengined: failed to emit a property change: {e}");
            }
        }
    };

    Ok(notify_changed)
}

fn parse_correction(s: &str) -> fdo::Result<Correction> {
    Correction::parse(s)
        .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown correction '{s}' (want off, stereo or surround)")))
}

fn response(bands: &[eq::biquad::Band], correction: &str) -> fdo::Result<Vec<f64>> {
    let c = parse_correction(correction)?;
    let freqs = (0..quantumengine_dbus::EQ_RESPONSE_POINTS).map(quantumengine_dbus::eq_response_freq);
    Ok(eq::response(c, bands, freqs))
}

fn band_arg(b: &eq::biquad::Band) -> EqBand {
    (b.kind.as_str().to_string(), b.freq, b.gain_db, b.q)
}

fn parse_bands(bands: Vec<EqBand>) -> fdo::Result<Vec<eq::biquad::Band>> {
    bands
        .into_iter()
        .map(|(kind, freq, gain_db, q)| {
            let kind = eq::biquad::FilterType::parse(&kind).ok_or_else(|| {
                fdo::Error::InvalidArgs(format!(
                    "unknown filter type '{kind}' (want bell, lowshelf, highshelf, highpass or lowpass)"
                ))
            })?;
            Ok(eq::biquad::Band { kind, freq, gain_db, q })
        })
        .collect()
}
