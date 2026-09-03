use std::path::PathBuf;

use atlas_cli::daemon_ctl;
use atlas_core::paths::AtlasPaths;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
use tauri_plugin_positioner::{Position, WindowExt};

mod commands;
mod notify_poller;

use commands::platform::{
    about_info, app_exit, app_relaunch, autostart_get, autostart_set, clipboard_write,
    install_shortcut, log_dir, notification_permission, notify, open_log_folder, shortcut_set,
    ui_state_all, ui_state_get, ui_state_set, update_check, update_install, vault_list,
    vault_lock, vault_put_key, vault_reapply, vault_set_passphrase, vault_status, vault_unlock,
    window_center, window_move, ShortcutRegistration, UpdateState, VaultState,
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

/// How long to wait, in total, for the daemon to answer before giving up on restoring
/// the global shortcut at boot. Matches the scale the daemon's own integration test
/// harness gives itself to come up (20s, in `atlasd/tests/api.rs`).
const SHORTCUT_RESTORE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const SHORTCUT_RESTORE_POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// Reads `ui.global_shortcut` from the daemon, once it answers, and registers it. Runs
/// once at boot as its own background task, in parallel with (not blocking) window
/// creation: a daemon that never comes up within `SHORTCUT_RESTORE_TIMEOUT`, or a
/// stored shortcut that fails to register (already claimed by another app, say), is
/// logged and otherwise left alone, since the user can always set one from Settings.
fn restore_global_shortcut_at_boot(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
        let deadline = tokio::time::Instant::now() + SHORTCUT_RESTORE_TIMEOUT;
        let settings = loop {
            if let Some(port) = daemon_ctl::daemon_info(&AtlasPaths::discover()).and_then(|v| v.get("port")?.as_u64()) {
                let base = format!("http://127.0.0.1:{port}/api/v1");
                if let Ok(resp) = client.get(format!("{base}/settings")).send().await {
                    if resp.status().is_success() {
                        if let Ok(settings) = resp.json::<serde_json::Value>().await {
                            break Some(settings);
                        }
                    }
                }
            }
            if tokio::time::Instant::now() >= deadline {
                break None;
            }
            tokio::time::sleep(SHORTCUT_RESTORE_POLL).await;
        };

        let Some(settings) = settings else {
            log::warn!("could not reach the daemon within {SHORTCUT_RESTORE_TIMEOUT:?}; the global shortcut was not restored at boot");
            return;
        };
        let Some(accelerator) = settings.get("ui.global_shortcut").and_then(|v| v.as_str()) else {
            return;
        };
        if let Err(e) = install_shortcut(&app, accelerator) {
            log::warn!("could not restore the global shortcut '{accelerator}' at boot: {e}");
        }
    });
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
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // `tauri-plugin-stronghold` is used as a plain Rust library (see
        // `commands::platform`'s vault commands), not registered here: the webview never
        // invokes its own commands, so it needs no managed state or capability entry.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(ShortcutRegistration::default())
        .manage(VaultState::default())
        .manage(UpdateState::default())
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
            // The notification poller is a plain tokio task, not a Tauri command: nothing
            // in the webview drives it, and it must keep running whether or not a Settings
            // page is open to read `ui.notify.*`.
            notify_poller::spawn(app.handle().clone());
            // Restores the last shortcut the user applied, once the daemon is up: a
            // shortcut set before quitting must still work after a relaunch.
            restore_global_shortcut_at_boot(app.handle().clone());
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
            log_dir,
            autostart_get,
            autostart_set,
            shortcut_set,
            notify,
            notification_permission,
            clipboard_write,
            about_info,
            open_log_folder,
            vault_status,
            vault_set_passphrase,
            vault_unlock,
            vault_lock,
            vault_put_key,
            vault_list,
            vault_reapply,
            update_check,
            update_install
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
