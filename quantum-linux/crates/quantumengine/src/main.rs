//! Desktop control panel for the JBL Quantum 810 Wireless.

mod app;
mod device;

use eframe::egui;

/// The app icon, also installed system-wide by the package (see
/// `packaging/icons`). Embedded here too so the window has one even when run
/// straight out of `target/release`, unpackaged.
const ICON_PNG: &[u8] = include_bytes!("../icon.png");

fn main() -> eframe::Result {
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
