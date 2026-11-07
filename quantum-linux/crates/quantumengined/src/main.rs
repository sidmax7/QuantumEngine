//! The QuantumEngine background service.
//!
//! `quantumengined` owns the headset, the EQ filter, the update checks and
//! the tray icon, and exposes all of them on D-Bus (see `quantumengine-dbus`).
//! It is the only process that ever calls `jbl_quantum::Headset` directly; the
//! window (`quantumengine`), the CLI and the Plasma widget are all D-Bus
//! clients of it. It used to be `quantumengine --tray`, which still starts
//! it.

mod brir;
mod dbus_service;
mod device;
mod eq;
mod notifier;
mod output;
mod pwgraph;
mod settings;
mod surround;
mod tray;
mod update;
mod volume;

use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("quantumengined: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Claims the D-Bus name, then starts the EQ, surround, the updater, the D-Bus
/// interface, the tray icon, and the one worker thread that owns the headset.
/// Everything else in the process (the tray's own render methods, and every
/// D-Bus client) only ever reads their state or sends them commands.
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(connection) = dbus_service::claim()? else {
        // The widget, the window, `quantumenginectl` and the login autostart
        // can all start a service at about the same moment; whichever loses
        // the bus name just leaves, before it starts the EQ, makes a tray
        // icon or opens the dongle.
        println!("quantumengined: already running");
        return Ok(());
    };

    let shared = device::Shared::default();
    let (commands, command_rx) = std::sync::mpsc::channel();
    let notifier = notifier::Notifier::default();
    let eq = eq::spawn(notifier.clone());
    let surround = surround::spawn(notifier.clone(), eq.clone());
    let updater = update::spawn(notifier.clone());
    let volume = volume::spawn(notifier.clone());

    let dbus_refresh = dbus_service::serve(
        &connection,
        shared.clone(),
        commands.clone(),
        dbus_service::Workers { eq: eq.clone(), surround: surround.clone(), updater: updater.clone(), volume },
        notifier.clone(),
    )?;
    // The icon is a convenience; the headset, EQ and updates must keep
    // working for the window, widget and CLI without it.
    let tray_refresh = match tray::spawn(shared.clone(), commands, eq, surround, updater) {
        Ok(handle) => Some(tray::refresh(handle)),
        Err(e) => {
            eprintln!("quantumengined: no tray icon: {e}");
            None
        }
    };
    notifier.set(move || {
        dbus_refresh();
        if let Some(refresh) = &tray_refresh {
            refresh();
        }
    });

    std::thread::Builder::new()
        .name("headset".into())
        .spawn(move || device::run(shared, command_rx, move || notifier.notify()))
        .expect("failed to start the headset thread");

    println!("quantumengined: serving {} on the session bus", quantumengine_dbus::BUS_NAME);
    // Keeps the bus name claimed for the process's lifetime; the tray icon's
    // own background thread is what actually keeps the process alive day to
    // day.
    let _connection = connection;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
