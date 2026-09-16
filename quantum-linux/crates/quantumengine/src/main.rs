//! Desktop control panel for the JBL Quantum 810 Wireless.

mod app;
mod device;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Quantum Linux")
            .with_app_id("quantum-gui")
            .with_inner_size([420.0, 620.0])
            .with_min_inner_size([360.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "quantum-gui",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
