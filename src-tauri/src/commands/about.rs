// The About card and the native About panel, plus the log folder they point at.

use tauri::{Manager, Runtime};

/// Where the log plugin writes its files, for the About card and the palette's
/// `Open log folder`.
#[tauri::command]
pub fn log_dir<R: Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    app.path()
        .app_log_dir()
        .map(|dir| dir.display().to_string())
        .map_err(|e| e.to_string())
}

/// Reveals the log folder in the OS file browser. Rust-first like every other command
/// here: the page used to import `@tauri-apps/plugin-opener` itself for this, which this
/// replaces.
#[tauri::command]
pub fn open_log_folder<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    tauri_plugin_opener::reveal_item_in_dir(dir).map_err(|e| e.to_string())
}

/// The About card's static facts about this install: the app version, the Tauri
/// version (kept for the diagnostics report, no longer shown), OS, architecture,
/// locale, and the two directories the app writes to.
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
    gather_about(&app)
}

fn gather_about<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<AboutInfo, String> {
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

/// What the native About window shows under the version: the same facts the Settings
/// About card lists, one per line, without the Tauri version. `daemon` is the daemon's
/// version and database path once it has answered, `None` before that.
pub fn about_credits(info: &AboutInfo, daemon: Option<(&str, &str)>) -> String {
    let (version, db) = daemon.unwrap_or(("starting", "not connected yet"));
    format!(
        "Daemon version: {version}\nOS: {} {} ({})\nLocale: {}\nDatabase: {db}\nLog folder: {}\nData folder: {}",
        info.os_type,
        info.os_version,
        info.arch,
        info.locale.as_deref().unwrap_or("unknown"),
        info.log_dir,
        info.data_dir
    )
}

/// Installs the app menu with an About item that carries `about_credits`. The rest of
/// the menu is Tauri's default (Edit, View, Window), so only the About panel changes.
/// macOS only: that is where the panel exists.
#[cfg(target_os = "macos")]
pub fn install_app_menu<R: Runtime>(app: &tauri::AppHandle<R>, daemon: Option<(&str, &str)>) -> Result<(), String> {
    use tauri::menu::{AboutMetadata, IsMenuItem, Menu, PredefinedMenuItem, Submenu};
    let info = gather_about(app)?;
    let meta = AboutMetadata {
        name: Some("Atlas".into()),
        version: Some(info.app_version.clone()),
        credits: Some(about_credits(&info, daemon)),
        ..Default::default()
    };
    let e = |err: tauri::Error| err.to_string();
    let app_menu = Submenu::with_items(
        app,
        "Atlas",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About Atlas"), Some(meta)).map_err(e)?,
            &PredefinedMenuItem::separator(app).map_err(e)?,
            &PredefinedMenuItem::services(app, None).map_err(e)?,
            &PredefinedMenuItem::separator(app).map_err(e)?,
            &PredefinedMenuItem::hide(app, None).map_err(e)?,
            &PredefinedMenuItem::hide_others(app, None).map_err(e)?,
            &PredefinedMenuItem::show_all(app, None).map_err(e)?,
            &PredefinedMenuItem::separator(app).map_err(e)?,
            &PredefinedMenuItem::quit(app, None).map_err(e)?,
        ],
    )
    .map_err(e)?;
    let default = Menu::default(app).map_err(e)?;
    let rest = default.items().map_err(e)?;
    let mut items: Vec<&dyn IsMenuItem<R>> = vec![&app_menu];
    for item in rest.iter().skip(1) {
        items.push(item);
    }
    let menu = Menu::with_items(app, &items).map_err(e)?;
    app.set_menu(menu).map_err(e)?;
    Ok(())
}

/// Rebuilds the About panel's text once the daemon has answered, so it names the
/// daemon's version and database. A no-op off macOS.
#[tauri::command]
pub fn about_menu_refresh<R: Runtime>(app: tauri::AppHandle<R>, daemon_version: String, db_path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        install_app_menu(&app, Some((&daemon_version, &db_path)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, daemon_version, db_path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn about_credits_lists_every_fact_but_the_tauri_version() {
        let info = super::AboutInfo {
            app_version: "0.1.0".into(),
            tauri_version: "2.11.5".into(),
            os_type: "macos".into(),
            os_version: "26.6.2".into(),
            arch: "aarch64".into(),
            locale: Some("en-SG".into()),
            log_dir: "/logs".into(),
            data_dir: "/data".into(),
        };
        let text = super::about_credits(&info, Some(("0.1.0", "/home/atlas.duckdb")));
        assert_eq!(
            text,
            "Daemon version: 0.1.0\nOS: macos 26.6.2 (aarch64)\nLocale: en-SG\nDatabase: /home/atlas.duckdb\nLog folder: /logs\nData folder: /data"
        );
        assert!(!text.contains("2.11.5"), "the Tauri version is not shown");
        let early = super::about_credits(&info, None);
        assert!(early.starts_with("Daemon version: starting\n"), "{early}");
        assert!(early.contains("Database: not connected yet"), "{early}");
    }
}
