// The updater: check once, install what was found, relaunch.

use std::sync::Mutex;

use tauri::{Emitter, Runtime};
use tauri_plugin_updater::UpdaterExt;

use super::window::app_relaunch;

/// Mirrors `tauri.conf.json`'s `plugins.updater.pubkey`. A placeholder is not a valid
/// minisign public key, so `update_check` refuses before ever calling the plugin rather
/// than surfacing a parse error that means nothing to the user. Flip this to `true` only
/// once both that key and this constant have been replaced together, per docs/usage.md,
/// Desktop platform.
const UPDATER_PUBKEY_CONFIGURED: bool = false;

const UPDATE_PROGRESS_EVENT: &str = "atlas:update-progress";

/// The last `update_check` result, held so `update_install` does not have to check
/// again: the daemon-style flow here is check once, install what was found.
#[derive(Default)]
pub struct UpdateState(pub Mutex<Option<tauri_plugin_updater::Update>>);

#[derive(Debug, serde::Serialize)]
pub struct UpdateCheckResult {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub date: Option<String>,
}

/// Checks the endpoint in `tauri.conf.json` for a newer release. While the updater's
/// pubkey is still the repo's placeholder this returns the plain sentence the About card
/// shows instead, without making a request.
#[tauri::command]
pub async fn update_check<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, UpdateState>,
) -> Result<UpdateCheckResult, String> {
    if !UPDATER_PUBKEY_CONFIGURED {
        return Err("updates are not configured".to_string());
    }
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await.map_err(|e| e.to_string())? {
        Some(update) => {
            let date = update
                .date
                .and_then(|d| d.format(&time::format_description::well_known::Rfc3339).ok());
            let result = UpdateCheckResult {
                available: true,
                version: Some(update.version.clone()),
                notes: update.body.clone(),
                date,
            };
            *state.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(update);
            Ok(result)
        }
        None => {
            *state.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(UpdateCheckResult { available: false, version: None, notes: None, date: None })
        }
    }
}

/// Downloads and installs the update `update_check` last found, streaming progress as
/// `atlas:update-progress` events (`{ downloaded, total }`, `total` absent when the
/// server did not send a content length), then relaunches the app the same way
/// `app_relaunch` does. Never returns on success: the process is replaced first.
#[tauri::command]
pub async fn update_install<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, UpdateState>,
) -> Result<(), String> {
    if !UPDATER_PUBKEY_CONFIGURED {
        return Err("updates are not configured".to_string());
    }
    let update = state.0.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or_else(|| "Check for updates first.".to_string())?;
    let handle = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk_len, total| {
                downloaded += chunk_len as u64;
                let _ = handle.emit(UPDATE_PROGRESS_EVENT, serde_json::json!({ "downloaded": downloaded, "total": total }));
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    app_relaunch(app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[tokio::test]
    async fn update_check_refuses_while_the_pubkey_is_the_placeholder() {
        let app = mock_builder()
            .manage(UpdateState::default())
            .build(mock_context(noop_assets()))
            .expect("failed to build mock app");
        let handle = app.handle().clone();
        let err = update_check(handle.clone(), handle.state::<UpdateState>()).await.unwrap_err();
        assert_eq!(err, "updates are not configured");
    }

    #[tokio::test]
    async fn update_install_refuses_while_the_pubkey_is_the_placeholder() {
        let app = mock_builder()
            .manage(UpdateState::default())
            .build(mock_context(noop_assets()))
            .expect("failed to build mock app");
        let handle = app.handle().clone();
        let err = update_install(handle.clone(), handle.state::<UpdateState>()).await.unwrap_err();
        assert_eq!(err, "updates are not configured");
    }
}
