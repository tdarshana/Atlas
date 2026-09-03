use std::path::PathBuf;

use atlas_cli::daemon_ctl;
use atlas_core::paths::AtlasPaths;
use tauri::Manager;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
use tauri_plugin_positioner::{Position, WindowExt};

mod commands;

use commands::platform::{
    app_exit, app_relaunch, log_dir, ui_state_all, ui_state_get, ui_state_set, window_center,
    window_move,
};

const DEFAULT_PORT: u16 = 7433;
const MAX_LOG_FILE_SIZE: u128 = 5 * 1024 * 1024;

/// The `atlasd` bundled as a Tauri sidecar, which the bundler places next to the app
/// binary. Absent under `tauri dev` and in any build made without
/// `scripts/prepare-sidecar.sh`, in which case the daemon is looked up as usual.
fn sidecar_atlasd(app: &tauri::AppHandle) -> Option<PathBuf> {
    // `EXE_SUFFIX` is "" everywhere but Windows, where the bundled sidecar is `atlasd.exe`.
    let name = format!("atlasd{}", std::env::consts::EXE_SUFFIX);
    let sidecar = tauri::process::current_binary(&app.env()).ok()?.with_file_name(name);
    sidecar.exists().then_some(sidecar)
}

/// Start the daemon if it is not already up and return the port it answers on.
/// This command is the app's only way to launch a process; the webview never
/// shells out to `atlas` or `atlasd` itself.
#[tauri::command]
async fn daemon_ensure(app: tauri::AppHandle, port: Option<u16>) -> Result<u16, String> {
    daemon_ctl::ensure_daemon_with(
        &AtlasPaths::discover(),
        port.unwrap_or(DEFAULT_PORT),
        sidecar_atlasd(&app),
    )
    .await
    .map_err(|e| e.to_string())
}

/// The contents of `~/.atlas/daemon.json`, or None when no daemon has written one.
#[tauri::command]
fn daemon_info() -> Option<serde_json::Value> {
    daemon_ctl::daemon_info(&AtlasPaths::discover())
}

/// Restart the daemon: stop it the way `atlas daemon stop` does, then start it again
/// through `daemon_ensure`. The Settings screen's MCP card offers this as `Restart`.
#[tauri::command]
async fn daemon_restart(app: tauri::AppHandle, port: Option<u16>) -> Result<u16, String> {
    let paths = AtlasPaths::discover();
    daemon_ctl::stop_daemon(&paths).await.map_err(|e| e.to_string())?;
    daemon_ctl::ensure_daemon_with(&paths, port.unwrap_or(DEFAULT_PORT), sidecar_atlasd(&app))
        .await
        .map_err(|e| e.to_string())
}

/// Where to look when the daemon fails to start.
#[tauri::command]
fn log_path() -> String {
    AtlasPaths::discover().log_file().display().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Docs require this be the first plugin registered: it has to see every other
        // plugin's state as not yet set up when a second launch hands off to it.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(Target::new(TargetKind::Webview))
                .level(if cfg!(debug_assertions) {
                    tauri_plugin_log::log::LevelFilter::Debug
                } else {
                    tauri_plugin_log::log::LevelFilter::Info
                })
                .max_file_size(MAX_LOG_FILE_SIZE)
                .rotation_strategy(RotationStrategy::KeepSome(3))
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_persisted_scope::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_process::init())
        // macOS keeps its own chrome under the overlay title bar; Windows and Linux draw
        // none, so the webview's title bar is the only one there.
        .setup(|app| {
            if cfg!(not(target_os = "macos")) {
                if let Some(window) = app.get_webview_window("main") {
                    window.set_decorations(false)?;
                }
            }
            // window-state restores a saved size and position for us before this closure
            // runs; a first launch has nothing to restore, so that is when positioner
            // centres the window instead of leaving it wherever the OS defaulted it to.
            let state_file = app
                .path()
                .app_config_dir()?
                .join(tauri_plugin_window_state::DEFAULT_FILENAME);
            if !state_file.exists() {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.move_window(Position::Center);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            daemon_ensure,
            daemon_info,
            daemon_restart,
            log_path,
            ui_state_get,
            ui_state_set,
            ui_state_all,
            window_center,
            window_move,
            app_relaunch,
            app_exit,
            log_dir
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
