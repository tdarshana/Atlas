// Thin Rust-first commands over the foundation plugins: persisted UI state through the
// store plugin, window placement through positioner, process control through the `tauri`
// core the process plugin sits on, and the log plugin's own directory. The page only
// calls these; it never imports a plugin's own JS package.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager, Runtime, WebviewWindow};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_positioner::{Position, WindowExt};
use tauri_plugin_store::{Store, StoreExt};

/// The event the global shortcut and the palette's own Mod+K both raise; the shell's
/// shortcut layer (`src/lib/shell/shortcuts.ts`) listens for it on `window`.
const PALETTE_EVENT: &str = "atlas:palette";

/// Where the persisted UI state lives, resolved by the store plugin against the app
/// data dir.
const UI_STATE_FILE: &str = "atlas-ui.json";

/// How long to wait after a change before the store plugin writes it to disk.
const UI_STATE_DEBOUNCE_MS: u64 = 300;

/// The `atlas-ui.json` store, created with the debounce on first use and returned as
/// already-loaded on every call after. Generic over the runtime so the mock runtime
/// used in tests goes through the same code as the real app.
fn ui_state_store<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Arc<Store<R>>, String> {
    app.store_builder(UI_STATE_FILE)
        .auto_save(Duration::from_millis(UI_STATE_DEBOUNCE_MS))
        .build()
        .map_err(|e| e.to_string())
}

fn main_window<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<WebviewWindow<R>, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "The main window is not open.".to_string())
}

/// One value from the persisted UI state, or `None` when the key was never set.
#[tauri::command]
pub fn ui_state_get<R: Runtime>(
    app: tauri::AppHandle<R>,
    key: String,
) -> Result<Option<serde_json::Value>, String> {
    Ok(ui_state_store(&app)?.get(key))
}

/// Sets one value in the persisted UI state. The store plugin writes it to disk after
/// the debounce.
#[tauri::command]
pub fn ui_state_set<R: Runtime>(
    app: tauri::AppHandle<R>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    ui_state_store(&app)?.set(key, value);
    Ok(())
}

/// The whole persisted UI state, for the one-time migration out of `localStorage` and
/// for diagnostics.
#[tauri::command]
pub fn ui_state_all<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<HashMap<String, serde_json::Value>, String> {
    Ok(ui_state_store(&app)?.entries().into_iter().collect())
}

/// Centres the main window on its current screen.
#[tauri::command]
pub fn window_center<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    main_window(&app)?
        .move_window(Position::Center)
        .map_err(|e| e.to_string())
}

/// The four corners and the centre, the only positions `window_move` accepts.
fn parse_position(position: &str) -> Result<Position, String> {
    match position {
        "top-left" => Ok(Position::TopLeft),
        "top-right" => Ok(Position::TopRight),
        "bottom-left" => Ok(Position::BottomLeft),
        "bottom-right" => Ok(Position::BottomRight),
        "center" => Ok(Position::Center),
        other => Err(format!("\"{other}\" is not a window position.")),
    }
}

/// Moves the main window to one of the four corners or the centre of its screen.
#[tauri::command]
pub fn window_move<R: Runtime>(app: tauri::AppHandle<R>, position: String) -> Result<(), String> {
    let target = parse_position(&position)?;
    main_window(&app)?
        .move_window(target)
        .map_err(|e| e.to_string())
}

/// Restarts the app. Never returns: the process is replaced before this command's
/// caller sees a response.
#[tauri::command]
pub fn app_relaunch<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    app.restart()
}

/// Exits the app.
#[tauri::command]
pub fn app_exit<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

/// Where the log plugin writes its files, for the About card and the palette's
/// `Open log folder`.
#[tauri::command]
pub fn log_dir<R: Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    app.path()
        .app_log_dir()
        .map(|dir| dir.display().to_string())
        .map_err(|e| e.to_string())
}

/// Whether the app is registered to launch at login (a macOS launch agent).
#[tauri::command]
pub fn autostart_get<R: Runtime>(app: tauri::AppHandle<R>) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Turns launch-at-login on or off.
#[tauri::command]
pub fn autostart_set<R: Runtime>(app: tauri::AppHandle<R>, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled { manager.enable() } else { manager.disable() }.map_err(|e| e.to_string())
}

/// The accelerator this app process last managed to register, if any. Tracked so
/// [`install_shortcut`] knows exactly what to unregister when it replaces it, rather
/// than reaching for `unregister_all` (which would also remove the new one if it ever
/// raced a second registration) or reading it back off the plugin, which does not
/// expose a "what is currently registered" query.
#[derive(Default)]
pub struct ShortcutRegistration(pub std::sync::Mutex<Option<String>>);

/// Registers `accelerator` as the app's one global shortcut. Registers the new one
/// *before* touching whatever was registered before it: `validate_accelerator` only
/// checks the string's shape, not whether the OS will actually grant it (another app
/// may already hold the same combination), so unregistering the old shortcut first
/// would leave the user with no working shortcut at all on a failed attempt to change
/// it. On success the previous accelerator, if different, is unregistered; on failure
/// it is left exactly as it was. Pressing the shortcut shows and focuses the main
/// window and emits `atlas:palette`, which the shell's shortcut layer bridges onto the
/// same event Mod+K raises from inside the webview.
///
/// Shared by the `shortcut_set` command and the boot-time restore in `notify_poller`,
/// so both paths track the same "currently registered" state.
pub fn install_shortcut<R: Runtime>(app: &tauri::AppHandle<R>, accelerator: &str) -> Result<(), String> {
    let global_shortcut = app.global_shortcut();
    global_shortcut
        .on_shortcut(accelerator, move |app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            let _ = app.emit(PALETTE_EVENT, ());
        })
        .map_err(|e| e.to_string())?;

    let registration = app.state::<ShortcutRegistration>();
    let previous = {
        let mut guard = registration.0.lock().unwrap_or_else(|e| e.into_inner());
        let previous = guard.clone();
        *guard = Some(accelerator.to_string());
        previous
    };
    if let Some(stale) = accelerator_to_unregister(previous.as_deref(), accelerator) {
        let _ = global_shortcut.unregister(stale.as_str());
    }
    Ok(())
}

/// The previously-registered accelerator to unregister now that `new` has just
/// registered successfully, or `None` when there was nothing before it or the same
/// combination was simply reapplied (which must never unregister the one just
/// installed). Split out from [`install_shortcut`] so the ordering it fixes (the new
/// shortcut is already live by the time this is even consulted) is testable without a
/// running global-shortcut plugin, which needs a real OS event loop and so cannot run
/// in a unit test.
fn accelerator_to_unregister(previous: Option<&str>, new: &str) -> Option<String> {
    previous.filter(|p| *p != new).map(str::to_string)
}

/// Validates `accelerator` (the same check `settings.rs` runs on `ui.global_shortcut`,
/// so a bad value comes back as a plain sentence instead of a plugin error) and
/// installs it. See [`install_shortcut`] for the register-before-unregister ordering.
#[tauri::command]
pub fn shortcut_set<R: Runtime>(app: tauri::AppHandle<R>, accelerator: String) -> Result<(), String> {
    atlas_core::settings::validate_accelerator(&accelerator).map_err(|e| e.to_string())?;
    install_shortcut(&app, &accelerator)
}

/// Shows an OS notification.
#[tauri::command]
pub fn notify<R: Runtime>(app: tauri::AppHandle<R>, title: String, body: String) -> Result<(), String> {
    app.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
}

/// `"granted"`, `"denied"` or `"default"` (not yet decided).
#[tauri::command]
pub fn notification_permission<R: Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    use tauri::plugin::PermissionState;
    let state = app.notification().permission_state().map_err(|e| e.to_string())?;
    Ok(match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::Prompt | PermissionState::PromptWithRationale => "default",
    }
    .to_string())
}

/// Writes `text` to the system clipboard. Every copy action in the app goes through
/// this rather than the browser's `navigator.clipboard`, which a Tauri webview does not
/// always grant without a user gesture already in flight.
#[tauri::command]
pub fn clipboard_write<R: Runtime>(app: tauri::AppHandle<R>, text: String) -> Result<(), String> {
    app.clipboard().write_text(text).map_err(|e| e.to_string())
}

/// The About card's static facts about this install: app and Tauri versions, OS,
/// architecture, locale, and the two directories the app writes to.
#[derive(serde::Serialize)]
pub struct AboutInfo {
    pub app_version: String,
    pub tauri_version: String,
    pub os_type: String,
    pub os_version: String,
    pub arch: String,
    pub locale: Option<String>,
    pub log_dir: String,
    pub data_dir: String,
}

#[tauri::command]
pub fn about_info<R: Runtime>(app: tauri::AppHandle<R>) -> Result<AboutInfo, String> {
    Ok(AboutInfo {
        app_version: app.package_info().version.to_string(),
        tauri_version: tauri::VERSION.to_string(),
        os_type: tauri_plugin_os::type_().to_string(),
        os_version: tauri_plugin_os::version().to_string(),
        arch: tauri_plugin_os::arch().to_string(),
        locale: tauri_plugin_os::locale(),
        log_dir: app.path().app_log_dir().map(|d| d.display().to_string()).map_err(|e| e.to_string())?,
        data_dir: app.path().app_data_dir().map(|d| d.display().to_string()).map_err(|e| e.to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn parse_position_accepts_every_documented_value() {
        assert!(matches!(parse_position("top-left"), Ok(Position::TopLeft)));
        assert!(matches!(parse_position("top-right"), Ok(Position::TopRight)));
        assert!(matches!(parse_position("bottom-left"), Ok(Position::BottomLeft)));
        assert!(matches!(parse_position("bottom-right"), Ok(Position::BottomRight)));
        assert!(matches!(parse_position("center"), Ok(Position::Center)));
    }

    #[test]
    fn parse_position_rejects_anything_else_with_a_plain_sentence() {
        assert_eq!(
            parse_position("middle").unwrap_err(),
            "\"middle\" is not a window position."
        );
    }

    #[test]
    fn accelerator_to_unregister_is_none_with_nothing_registered_before() {
        assert_eq!(accelerator_to_unregister(None, "CmdOrCtrl+Shift+K"), None);
    }

    #[test]
    fn accelerator_to_unregister_names_a_different_previous_accelerator() {
        assert_eq!(
            accelerator_to_unregister(Some("CmdOrCtrl+Shift+K"), "Alt+Space"),
            Some("CmdOrCtrl+Shift+K".to_string())
        );
    }

    /// Reapplying the same accelerator must never unregister the one that was just
    /// installed a moment earlier: the whole point of registering before unregistering
    /// is that the newly-live shortcut is never the one this hands back.
    #[test]
    fn accelerator_to_unregister_is_none_when_the_combination_is_unchanged() {
        assert_eq!(accelerator_to_unregister(Some("Alt+Space"), "Alt+Space"), None);
    }

    /// A mock app with no window, so `main_window` fails the way it would if the
    /// window were closed. Neither the store nor positioner plugin is needed: both
    /// commands return before touching either.
    fn windowless_app() -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .build(mock_context(noop_assets()))
            .expect("failed to build mock app")
    }

    #[test]
    fn window_center_errs_with_a_plain_sentence_when_there_is_no_window() {
        let app = windowless_app();
        let err = window_center(app.handle().clone()).unwrap_err();
        assert_eq!(err, "The main window is not open.");
    }

    #[test]
    fn window_move_errs_on_an_unknown_position_before_touching_any_window() {
        let app = windowless_app();
        let err = window_move(app.handle().clone(), "middle".into()).unwrap_err();
        assert_eq!(err, "\"middle\" is not a window position.");
    }

    /// A guard that points `HOME` at a scratch directory for the life of one test and
    /// restores it on drop, so the store commands resolve the app data dir under a
    /// throwaway path instead of the real user's home. Rust unit tests in this crate
    /// run in one process per binary and this is the crate's only test that touches
    /// `HOME`, so there is nothing else in this process to race with it.
    struct HomeGuard(Option<std::ffi::OsString>);

    impl HomeGuard {
        fn set(dir: &std::path::Path) -> Self {
            let previous = std::env::var_os("HOME");
            // SAFETY: single-threaded with respect to `HOME` within this crate's tests;
            // see the doc comment above.
            unsafe { std::env::set_var("HOME", dir) };
            Self(previous)
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            // SAFETY: as above.
            unsafe {
                match &self.0 {
                    Some(v) => std::env::set_var("HOME", v),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    /// Store commands round-trip through the store plugin's in-memory cache (`set`
    /// writes it synchronously; `get` and `entries` read it back), so this does not
    /// need to wait out the 300ms debounce to see its own writes.
    #[test]
    fn ui_state_round_trips_under_a_temp_app_data_dir() {
        let dir = std::env::temp_dir().join(format!("atlas-desktop-ui-state-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the test's scratch dir");
        let _home = HomeGuard::set(&dir);

        let app = mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(mock_context(noop_assets()))
            .expect("failed to build mock app");
        let handle = app.handle().clone();

        assert_eq!(ui_state_get(handle.clone(), "missing".into()).unwrap(), None);

        ui_state_set(handle.clone(), "atlas.theme".into(), serde_json::json!("dark")).unwrap();
        assert_eq!(
            ui_state_get(handle.clone(), "atlas.theme".into()).unwrap(),
            Some(serde_json::json!("dark"))
        );

        ui_state_set(handle.clone(), "atlas.rail".into(), serde_json::json!("expanded")).unwrap();
        let all = ui_state_all(handle).unwrap();
        assert_eq!(all.get("atlas.theme"), Some(&serde_json::json!("dark")));
        assert_eq!(all.get("atlas.rail"), Some(&serde_json::json!("expanded")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
