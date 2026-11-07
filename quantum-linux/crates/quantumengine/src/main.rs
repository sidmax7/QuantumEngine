//! The QuantumEngine window: a Tauri shell around the Vue interface in
//! `ui/`. Thin by design. It never opens the headset; every command is a
//! D-Bus call to `quantumengined` (see `quantumengine-dbus`), which this
//! starts when it isn't running.
//!
//! `quantumengine --tray` used to be the service itself, and now just
//! hands over to `quantumengined`, so older autostart entries and scripts
//! keep working.

mod commands;
mod dto;
mod lighting;
mod service;
mod window_state;

use std::sync::Arc;
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder};

/// The command and event list, shared by the app and the bindings export.
fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::state,
            commands::info,
            commands::set_anc,
            commands::set_sidetone,
            commands::set_lights,
            commands::lighting,
            commands::apply_lighting,
            commands::set_eq_enabled,
            commands::set_eq_profile,
            commands::set_eq_correction,
            commands::eq_response,
            commands::eq_profile_bands,
            commands::eq_bands_response,
            commands::save_eq_profile,
            commands::delete_eq_profile,
            commands::preview_eq_bands,
            commands::end_eq_preview,
            commands::import_eq_profile,
            commands::set_surround_enabled,
            commands::set_volume,
            commands::probe_unverified,
            commands::set_unverified,
            commands::grant_unverified_access,
            commands::check_for_updates,
            commands::install_update,
            commands::set_update_checks,
            commands::restart_service,
            commands::open_release_page,
            commands::open_project_page,
        ])
        .events(collect_events![dto::StateEvent])
}

fn main() {
    if std::env::args().any(|a| a == "--tray") {
        let err = hand_over_to_service();
        eprintln!("quantumengine: cannot start quantumengined: {err}");
        std::process::exit(1);
    }
    linux_webview_workarounds();

    let builder = specta_builder();
    let link = Arc::new(service::Link::default());
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                drop(window.unminimize());
                drop(window.show());
                drop(window.set_focus());
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::clone(&link))
        .manage(window_state::Tracker::default())
        .on_window_event(window_state::on_event)
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            if let Some(window) = app.get_webview_window("main") {
                window_state::restore(&window);
            }
            service::watch(app.handle().clone(), link);
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("quantumengine: the window failed to start: {e}");
        std::process::exit(1);
    }
}

/// Replaces this process with `quantumengined`. Only returns on failure.
fn hand_over_to_service() -> std::io::Error {
    use std::os::unix::process::CommandExt;
    let sibling = std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("quantumengined"))
        .filter(|p| p.is_file());
    std::process::Command::new(sibling.unwrap_or_else(|| "quantumengined".into())).exec()
}

/// WebKitGTK's DMA-BUF renderer fails on a number of GPU, driver and Wayland
/// combinations: the window stays blank, or the app exits with "Error 71
/// (Protocol error) dispatching to Wayland display". Turning it off costs a
/// little rendering speed and works everywhere. Anyone who wants it back
/// sets `WEBKIT_DISABLE_DMABUF_RENDERER=0` themselves.
fn linux_webview_workarounds() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // Called first thing in `main`, before any other thread exists.
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use specta_typescript::Typescript;

    /// The committed `ui/src/bindings.ts` must match the Rust types.
    /// Regenerate with `QUANTUMENGINE_UPDATE_BINDINGS=1 cargo test -p quantumengine`.
    #[test]
    fn bindings_are_up_to_date() {
        let committed = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/bindings.ts");
        let fresh = std::env::temp_dir().join(format!("quantumengine-bindings-{}.ts", std::process::id()));
        specta_builder().export(Typescript::default(), &fresh).expect("export bindings");
        let fresh_text = std::fs::read_to_string(&fresh).unwrap();
        std::fs::remove_file(&fresh).unwrap();
        if std::env::var_os("QUANTUMENGINE_UPDATE_BINDINGS").is_some() {
            std::fs::write(&committed, &fresh_text).unwrap();
            return;
        }
        let committed_text = std::fs::read_to_string(&committed).unwrap_or_default();
        assert!(
            committed_text == fresh_text,
            "ui/src/bindings.ts is stale; run QUANTUMENGINE_UPDATE_BINDINGS=1 cargo test -p quantumengine"
        );
    }
}
