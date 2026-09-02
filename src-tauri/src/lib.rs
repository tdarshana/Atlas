use std::path::PathBuf;

use atlas_cli::daemon_ctl;
use atlas_core::paths::AtlasPaths;
use tauri::Manager;

const DEFAULT_PORT: u16 = 7433;

/// The `atlasd` bundled as a Tauri sidecar, which the bundler places next to the app
/// binary. Absent under `tauri dev` and in any build made without
/// `scripts/prepare-sidecar.sh`, in which case the daemon is looked up as usual.
fn sidecar_atlasd(app: &tauri::AppHandle) -> Option<PathBuf> {
    let sidecar = tauri::process::current_binary(&app.env()).ok()?.with_file_name("atlasd");
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

/// Where to look when the daemon fails to start.
#[tauri::command]
fn log_path() -> String {
    AtlasPaths::discover().log_file().display().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![daemon_ensure, daemon_info, log_path])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
