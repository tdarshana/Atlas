// Tauri commands over the plugin manifest, install and registry layers: list what is
// installed, install from a folder or a GitHub repository, enable or disable one,
// uninstall it, and read its `main` file's contents for the web host to execute. Every
// command resolves `app_data` from `app.path().app_data_dir()` and runs its blocking
// filesystem or network work inside `spawn_blocking`, matching `commands/platform.rs`.

pub mod install;
pub mod manifest;
pub mod protocol;
pub mod registry;

use std::path::{Path, PathBuf};

use tauri::{Manager, Runtime};

use registry::PluginInfo;

fn app_data_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// Every installed plugin, compatible or not; see [`registry::list`].
#[tauri::command]
pub async fn plugins_list<R: Runtime>(app: tauri::AppHandle<R>) -> Result<Vec<PluginInfo>, String> {
    let app_data = app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || registry::list(&app_data)).await.map_err(|e| e.to_string())?
}

/// Installs the plugin at the local folder `path`.
#[tauri::command]
pub async fn plugin_install_folder<R: Runtime>(app: tauri::AppHandle<R>, path: String) -> Result<PluginInfo, String> {
    let app_data = app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || install::install_from_folder(&app_data, Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

/// Installs the plugin at the GitHub repository URL `url`.
#[tauri::command]
pub async fn plugin_install_github<R: Runtime>(app: tauri::AppHandle<R>, url: String) -> Result<PluginInfo, String> {
    let app_data = app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || install::install_from_github(&app_data, &url))
        .await
        .map_err(|e| e.to_string())?
}

/// Enables or disables `id`, returning its updated [`PluginInfo`]. Refuses to enable an
/// incompatible plugin; see [`registry::set_enabled`].
#[tauri::command]
pub async fn plugin_set_enabled<R: Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    enabled: bool,
) -> Result<PluginInfo, String> {
    let app_data = app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        registry::set_enabled(&app_data, &id, enabled)?;
        registry::list(&app_data)?.into_iter().find(|p| p.id == id).ok_or_else(|| format!("No plugin '{id}' is installed."))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Replaces the permissions `id` is granted, returning its updated [`PluginInfo`].
/// Refuses anything the manifest does not declare; see [`registry::set_permissions`].
#[tauri::command]
pub async fn plugin_set_permissions<R: Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    granted: Vec<manifest::Permission>,
) -> Result<PluginInfo, String> {
    let app_data = app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        registry::set_permissions(&app_data, &id, &granted)?;
        registry::list(&app_data)?.into_iter().find(|p| p.id == id).ok_or_else(|| format!("No plugin '{id}' is installed."))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Removes `id` and its files. See [`registry::uninstall`].
#[tauri::command]
pub async fn plugin_uninstall<R: Runtime>(app: tauri::AppHandle<R>, id: String) -> Result<(), String> {
    let app_data = app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || registry::uninstall(&app_data, &id)).await.map_err(|e| e.to_string())?
}

/// The contents of `id`'s manifest `main` file. Refuses a disabled or an incompatible
/// plugin: the web host must not be handed code from either.
#[tauri::command]
pub async fn plugin_read_main<R: Runtime>(app: tauri::AppHandle<R>, id: String) -> Result<String, String> {
    let app_data = app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let info = registry::list(&app_data)?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("No plugin '{id}' is installed."))?;
        if !info.compatible {
            return Err(info.reason.unwrap_or_else(|| format!("'{id}' is not compatible.")));
        }
        if !info.enabled {
            return Err(format!("'{id}' is disabled."));
        }
        // `compatible` is only ever true once `manifest` parsed, so this is always
        // `Some` by the time the two checks above let execution reach here.
        let manifest = info.manifest.ok_or_else(|| format!("'{id}' has no manifest."))?;
        std::fs::read_to_string(info.dir.join(&manifest.main)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The contents of one file inside `id`'s folder, under the same path rules the
/// `atlas-plugin` protocol enforces (no traversal, no encoded separators, no symlink that
/// leaves the folder) and the same refusal of a disabled or incompatible plugin. Used for
/// a contributed theme's CSS file, which the app reads rather than the frame.
#[tauri::command]
pub async fn plugin_read_file<R: Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    path: String,
) -> Result<String, String> {
    let app_data = app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = protocol::resolve_file(&app_data, &id, &path).map_err(|(_, message)| message)?;
        std::fs::read_to_string(resolved).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
