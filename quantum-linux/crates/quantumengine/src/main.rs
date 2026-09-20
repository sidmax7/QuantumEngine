//! Desktop control panel for the JBL Quantum 810 Wireless.
//!
//! `quantumengine` opens the window. `quantumengine --tray` instead runs the
//! background service that owns the headset and exposes it on D-Bus (see
//! `quantumengine-dbus`); from step 3 onward that mode also owns the tray
//! icon. Only the `--tray` process should ever call `jbl_quantum::Headset`
//! directly — everything else, including this same binary's own window, is
//! meant to become a D-Bus client of it.

mod app;
mod dbus_client;
mod dbus_service;
mod device;
mod tray;

use eframe::egui;
use std::process::ExitCode;

/// The app icon, also installed system-wide by the package (see
/// `packaging/icons`). Embedded here too so the window has one even when run
/// straight out of `target/release`, unpackaged.
const ICON_PNG: &[u8] = include_bytes!("../icon.png");

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--tray") {
        match run_tray() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("quantumengine --tray: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        match run_window() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("quantumengine: {e}");
                ExitCode::FAILURE
            }
        }
    }
}

/// Starts the tray icon, claims the D-Bus name, and starts the one worker
/// thread that owns the headset — everything else in the process (the tray's
/// own render methods, and every D-Bus client) only ever reads `shared` or
/// sends a [`device::Command`], never touching the headset directly.
fn run_tray() -> Result<(), Box<dyn std::error::Error>> {
    let shared = device::Shared::default();
    let (commands, command_rx) = std::sync::mpsc::channel();

    let Some((connection, dbus_refresh)) = dbus_service::serve(shared.clone(), commands.clone())? else {
        // The widget, the window, `quantumenginectl` and the login autostart
        // can all start a service at about the same moment; whichever loses
        // the bus name just leaves, before it makes a tray icon or opens the
        // dongle.
        println!("quantumengine: already running");
        return Ok(());
    };
    let tray_handle = tray::spawn(shared.clone(), commands)?;
    let tray_refresh = tray::refresh(tray_handle);

    std::thread::Builder::new()
        .name("headset".into())
        .spawn(move || {
            device::run(shared, command_rx, move || {
                dbus_refresh();
                tray_refresh();
            })
        })
        .expect("failed to start the headset thread");

    println!("quantumengine: serving {} on the session bus", quantumengine_dbus::BUS_NAME);
    // Keeps the bus name claimed for the process's lifetime; the tray icon's
    // own background thread (owned by `tray_handle`, dropped only on exit)
    // is what actually keeps the process alive day to day.
    let _connection = connection;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn run_window() -> eframe::Result {
    let icon = eframe::icon_data::from_png_bytes(ICON_PNG).expect("bundled icon.png is a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("QuantumEngine")
            .with_app_id("quantumengine")
            .with_icon(icon)
            .with_inner_size([420.0, 620.0])
            .with_min_inner_size([360.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "quantumengine",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
