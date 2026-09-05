// The menu bar item. The daemon outlives the app (it is spawned into its own process
// group and nothing stops it on quit), so closing the window hides Atlas to the menu bar
// rather than quitting, and the item shows whether the daemon is up, reopens the window,
// starts or stops the daemon, and quits with or without it.

use std::sync::Mutex;

use atlas_client::daemon_ctl;
use atlas_core::paths::AtlasPaths;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::cli::sidecar_bin;
use crate::DEFAULT_PORT;

/// The items whose text or enabled state follows the daemon.
struct Items<R: Runtime> {
    status: MenuItem<R>,
    toggle: MenuItem<R>,
}

/// Managed state: the live menu items, and the last state they were told about.
pub struct TrayState<R: Runtime> {
    items: Mutex<Option<Items<R>>>,
    running: Mutex<Option<u16>>,
}

impl<R: Runtime> Default for TrayState<R> {
    fn default() -> Self {
        Self { items: Mutex::new(None), running: Mutex::new(None) }
    }
}

/// The text the two dynamic items show for a daemon state: `Some(port)` when it answers,
/// `None` when it does not.
pub fn labels(running: Option<u16>) -> (String, &'static str) {
    match running {
        Some(port) => (format!("Daemon: running on {port}"), "Stop daemon"),
        None => ("Daemon: stopped".to_string(), "Start daemon"),
    }
}

/// Builds the item and its menu. Called once from setup.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let (status_text, toggle_text) = labels(None);
    let status = MenuItem::with_id(app, "tray-status", status_text, false, None::<&str>)?;
    let open = MenuItem::with_id(app, "tray-open", "Open Atlas", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "tray-toggle", toggle_text, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit Atlas", true, None::<&str>)?;
    let quit_stop = MenuItem::with_id(app, "tray-quit-stop", "Quit and stop daemon", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&status, &PredefinedMenuItem::separator(app)?, &open, &toggle, &PredefinedMenuItem::separator(app)?, &quit, &quit_stop],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .icon(tauri::include_image!("icons/tray.png"))
        .tooltip("Atlas")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()));
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true);
    }
    builder.build(app)?;

    let state = app.state::<TrayState<R>>();
    *state.items.lock().unwrap_or_else(|e| e.into_inner()) = Some(Items { status, toggle });
    Ok(())
}

/// Tells the item whether the daemon answers, and on which port. The notify poller
/// calls this on every tick; the tray's own start and stop call it as they finish.
pub fn set_running<R: Runtime>(app: &AppHandle<R>, running: Option<u16>) {
    let Some(state) = app.try_state::<TrayState<R>>() else { return };
    *state.running.lock().unwrap_or_else(|e| e.into_inner()) = running;
    let items = state.items.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(items) = items.as_ref() {
        let (status, toggle) = labels(running);
        let _ = items.status.set_text(status);
        let _ = items.toggle.set_text(toggle);
    }
}

fn running<R: Runtime>(app: &AppHandle<R>) -> Option<u16> {
    app.try_state::<TrayState<R>>().and_then(|s| *s.running.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Brings the main window back from the menu bar: a regular app again (Dock icon and
/// menu bar), then shown and focused.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Hides the main window to the menu bar instead of closing it. On macOS the app also
/// leaves the Dock, so what remains is the menu bar item alone.
pub fn hide_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        "tray-open" => show_main(app),
        "tray-toggle" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let paths = AtlasPaths::discover();
                if running(&app).is_some() {
                    match daemon_ctl::stop_daemon(&paths).await {
                        Ok(_) => set_running(&app, None),
                        Err(e) => log::warn!("tray: could not stop the daemon: {e}"),
                    }
                } else {
                    match daemon_ctl::ensure_daemon_with(&paths, DEFAULT_PORT, sidecar_bin(&app, "atlasd")).await {
                        Ok(port) => set_running(&app, Some(port)),
                        Err(e) => log::warn!("tray: could not start the daemon: {e}"),
                    }
                }
            });
        }
        "tray-quit" => app.exit(0),
        "tray-quit-stop" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = daemon_ctl::stop_daemon(&AtlasPaths::discover()).await {
                    log::warn!("tray: could not stop the daemon before quitting: {e}");
                }
                app.exit(0);
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_labels_follow_the_daemon_state() {
        assert_eq!(labels(Some(7433)), ("Daemon: running on 7433".to_string(), "Stop daemon"));
        assert_eq!(labels(None), ("Daemon: stopped".to_string(), "Start daemon"));
    }
}
