// Thin Rust-first commands over the foundation plugins: persisted UI state through the
// store plugin, window placement through positioner, process control through the `tauri`
// core the process plugin sits on, and the log plugin's own directory. The page only
// calls these; it never imports a plugin's own JS package.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tauri::{Manager, Runtime, WebviewWindow};
use tauri_plugin_positioner::{Position, WindowExt};
use tauri_plugin_store::{Store, StoreExt};

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
