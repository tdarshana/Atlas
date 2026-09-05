// OS notifications and their permission, over the notification plugin.

use tauri::Runtime;
use tauri_plugin_notification::NotificationExt;

/// Shows an OS notification.
#[tauri::command]
pub fn notify<R: Runtime>(app: tauri::AppHandle<R>, title: String, body: String) -> Result<(), String> {
    app.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
}

/// `"granted"`, `"denied"` or `"default"` (not yet decided).
fn permission_state_str(state: tauri::plugin::PermissionState) -> String {
    use tauri::plugin::PermissionState;
    match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::Prompt | PermissionState::PromptWithRationale => "default",
    }
    .to_string()
}

/// `"granted"`, `"denied"` or `"default"` (not yet decided).
#[tauri::command]
pub fn notification_permission<R: Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    let state = app.notification().permission_state().map_err(|e| e.to_string())?;
    Ok(permission_state_str(state))
}

/// Asks the OS for notification permission when it has not been decided yet, so the
/// Notifications card has something to offer beyond the hint. A no-op the OS answers
/// immediately (with the existing decision) when permission was already granted or
/// denied.
#[tauri::command]
pub fn notification_request_permission<R: Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    let state = app.notification().request_permission().map_err(|e| e.to_string())?;
    Ok(permission_state_str(state))
}
