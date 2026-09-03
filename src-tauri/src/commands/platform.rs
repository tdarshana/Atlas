// Thin Rust-first commands over the foundation plugins: persisted UI state through the
// store plugin, window placement through positioner, process control through the `tauri`
// core the process plugin sits on, and the log plugin's own directory. The page only
// calls these; it never imports a plugin's own JS package.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atlas_cli::daemon_ctl;
use atlas_core::paths::AtlasPaths;
use tauri::{Emitter, Manager, Runtime, WebviewWindow};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_positioner::{Position, WindowExt};
use tauri_plugin_store::{Store, StoreExt};
use tauri_plugin_stronghold::kdf::KeyDerivation;
use tauri_plugin_stronghold::stronghold::Stronghold;
use tauri_plugin_updater::UpdaterExt;

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

/// Reveals the log folder in the OS file browser. Rust-first like every other command
/// here: the page used to import `@tauri-apps/plugin-opener` itself for this, which this
/// replaces.
#[tauri::command]
pub fn open_log_folder<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    tauri_plugin_opener::reveal_item_in_dir(dir).map_err(|e| e.to_string())
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

// ---- Vault (stronghold) ----
//
// Used as a plain Rust library, not as a registered Tauri plugin: the passphrase and
// every scoped key stay on this side of the IPC boundary, and the webview never invokes
// `tauri-plugin-stronghold`'s own commands, so no stronghold capability entry is needed.
// A vault holds one Stronghold client (`VAULT_CLIENT`); a scope's key is a record in
// that client's own key/value store, which (unlike the top-level `Stronghold::store()`)
// is part of what `Stronghold::save` commits to the snapshot file.

/// The vault's snapshot file, in the app data dir.
const VAULT_FILE: &str = "atlas.hold";
/// The argon2 salt beside it. Generated once, on the first `vault_set_passphrase`; the
/// salt itself is not secret, only the passphrase is.
const VAULT_SALT_FILE: &str = "atlas-vault.salt";
/// The one Stronghold client every scoped key is stored under.
const VAULT_CLIENT: &[u8] = b"atlas";

/// The unlocked vault, held only in memory: `None` while locked or missing. The
/// passphrase itself is never kept past the argon2 hash that opens or creates it, and
/// is never written to disk.
#[derive(Default)]
pub struct VaultState(pub Mutex<Option<Stronghold>>);

fn vault_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.path().app_data_dir().map(|d| d.join(VAULT_FILE)).map_err(|e| e.to_string())
}

fn vault_salt_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.path().app_data_dir().map(|d| d.join(VAULT_SALT_FILE)).map_err(|e| e.to_string())
}

/// `"missing"` when `atlas.hold` does not exist yet, `"unlocked"` while this app process
/// holds the open vault in memory, `"locked"` otherwise.
#[tauri::command]
pub fn vault_status<R: Runtime>(
    app: tauri::AppHandle<R>,
    vault: tauri::State<'_, VaultState>,
) -> Result<String, String> {
    if vault.0.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return Ok("unlocked".to_string());
    }
    Ok(if vault_path(&app)?.exists() { "locked" } else { "missing" }.to_string())
}

/// Creates `atlas.hold` and its one client, keyed by an argon2 hash of `passphrase`.
/// Refuses if a vault already exists here; `vault_unlock` is how an existing one opens.
#[tauri::command]
pub fn vault_set_passphrase<R: Runtime>(
    app: tauri::AppHandle<R>,
    vault: tauri::State<'_, VaultState>,
    passphrase: String,
) -> Result<(), String> {
    let path = vault_path(&app)?;
    if path.exists() {
        return Err("A vault already exists; unlock it instead.".to_string());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let key = KeyDerivation::argon2(&passphrase, &vault_salt_path(&app)?);
    let stronghold = Stronghold::new(&path, key).map_err(|e| e.to_string())?;
    stronghold.create_client(VAULT_CLIENT).map_err(|e| e.to_string())?;
    stronghold.save().map_err(|e| e.to_string())?;
    *vault.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(stronghold);
    Ok(())
}

/// Opens the existing `atlas.hold` with an argon2 hash of `passphrase`. A wrong
/// passphrase fails to decrypt the snapshot, which is reported as a plain sentence
/// rather than the crypto error underneath it.
#[tauri::command]
pub fn vault_unlock<R: Runtime>(
    app: tauri::AppHandle<R>,
    vault: tauri::State<'_, VaultState>,
    passphrase: String,
) -> Result<(), String> {
    let path = vault_path(&app)?;
    if !path.exists() {
        return Err("No vault exists yet; set a passphrase first.".to_string());
    }
    let key = KeyDerivation::argon2(&passphrase, &vault_salt_path(&app)?);
    let stronghold = Stronghold::new(&path, key).map_err(|_| "Incorrect passphrase.".to_string())?;
    stronghold
        .load_client(VAULT_CLIENT)
        .or_else(|_| stronghold.create_client(VAULT_CLIENT))
        .map_err(|e| e.to_string())?;
    *vault.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(stronghold);
    Ok(())
}

/// Saves the vault, then drops it from memory. Every key it holds stays on disk,
/// encrypted; only the open, in-memory handle is gone.
#[tauri::command]
pub fn vault_lock(vault: tauri::State<'_, VaultState>) -> Result<(), String> {
    let mut guard = vault.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(stronghold) = guard.as_ref() {
        stronghold.save().map_err(|e| e.to_string())?;
    }
    *guard = None;
    Ok(())
}

/// Stores `key` under `scope` (`"global"` or `"project/<id>"`) and saves the vault.
/// Errs with the same sentence the Extraction cards show when the vault is locked.
#[tauri::command]
pub fn vault_put_key(vault: tauri::State<'_, VaultState>, scope: String, key: String) -> Result<(), String> {
    let guard = vault.0.lock().unwrap_or_else(|e| e.into_inner());
    let stronghold = guard.as_ref().ok_or_else(|| "Vault locked, key not mirrored.".to_string())?;
    let client = stronghold.get_client(VAULT_CLIENT).map_err(|e| e.to_string())?;
    client.store().insert(scope.into_bytes(), key.into_bytes(), None).map_err(|e| e.to_string())?;
    stronghold.save().map_err(|e| e.to_string())
}

/// Every scope the vault currently holds a key for, sorted.
#[tauri::command]
pub fn vault_list(vault: tauri::State<'_, VaultState>) -> Result<Vec<String>, String> {
    let guard = vault.0.lock().unwrap_or_else(|e| e.into_inner());
    let stronghold = guard.as_ref().ok_or_else(|| "Vault locked, key not mirrored.".to_string())?;
    let client = stronghold.get_client(VAULT_CLIENT).map_err(|e| e.to_string())?;
    let mut scopes: Vec<String> = client
        .store()
        .keys()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|k| String::from_utf8(k).ok())
        .collect();
    scopes.sort();
    Ok(scopes)
}

/// The daemon's base API url, resolved fresh from `daemon.json` rather than cached: the
/// port can change across a restart. Mirrors `notify_poller::base_url`.
fn daemon_base_url() -> Result<String, String> {
    let port = daemon_ctl::daemon_info(&AtlasPaths::discover())
        .and_then(|v| v.get("port")?.as_u64())
        .ok_or_else(|| "The daemon is not running.".to_string())?;
    Ok(format!("http://127.0.0.1:{port}/api/v1"))
}

/// Sends the vault's stored key for `scope` back to the daemon: `"global"` writes
/// `extraction.api_key` through `PUT /settings`; `"project/<id>"` reads the project's
/// current extraction override (so its other fields survive) and writes the key back
/// through `PUT /projects/{id}/extraction`.
#[tauri::command]
pub async fn vault_reapply(vault: tauri::State<'_, VaultState>, scope: String) -> Result<(), String> {
    let key = {
        let guard = vault.0.lock().unwrap_or_else(|e| e.into_inner());
        let stronghold = guard.as_ref().ok_or_else(|| "Vault locked, key not mirrored.".to_string())?;
        let client = stronghold.get_client(VAULT_CLIENT).map_err(|e| e.to_string())?;
        let raw = client
            .store()
            .get(scope.as_bytes())
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("No key stored for '{scope}'."))?;
        String::from_utf8(raw).map_err(|e| e.to_string())?
    };
    let base = daemon_base_url()?;
    let http = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().map_err(|e| e.to_string())?;
    if scope == "global" {
        let resp = http
            .put(format!("{base}/settings"))
            .json(&serde_json::json!({ "extraction.api_key": key }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        return if resp.status().is_success() { Ok(()) } else { Err(format!("The daemon refused the key ({}).", resp.status())) };
    }
    let Some(id) = scope.strip_prefix("project/") else {
        return Err(format!("'{scope}' is not a vault scope."));
    };
    let project: serde_json::Value = http
        .get(format!("{base}/projects/{id}"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let mut extraction = project.get("extraction").cloned().unwrap_or(serde_json::Value::Null);
    if extraction.is_null() {
        extraction = serde_json::json!({});
    }
    extraction["api_key"] = serde_json::Value::String(key);
    let resp = http
        .put(format!("{base}/projects/{id}/extraction"))
        .json(&extraction)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() { Ok(()) } else { Err(format!("The daemon refused the key ({}).", resp.status())) }
}

// ---- Updater ----

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
    app_relaunch(app)
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

    /// Serializes every test in this module that points `HOME` at a scratch directory:
    /// `HOME` is process-global and `cargo test` runs this binary's tests on several
    /// threads by default, so two such tests running at once would each see the other's
    /// directory. Held for the life of the [`HomeGuard`] that acquired it.
    static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A guard that points `HOME` at a scratch directory for the life of one test and
    /// restores it on drop, so the store and vault commands resolve the app data dir
    /// under a throwaway path instead of the real user's home. `_lock` is never read;
    /// it exists only to hold `HOME_LOCK` until this guard drops.
    struct HomeGuard {
        previous: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl HomeGuard {
        fn set(dir: &std::path::Path) -> Self {
            let _lock = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let previous = std::env::var_os("HOME");
            // SAFETY: serialized against every other `HomeGuard` by `HOME_LOCK`, held
            // until this guard drops.
            unsafe { std::env::set_var("HOME", dir) };
            Self { previous, _lock }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            // SAFETY: as above.
            unsafe {
                match &self.previous {
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

    /// One test covering the whole vault lifecycle rather than several: `Stronghold`'s
    /// argon2 hash is not cheap, and the file it opens is the same scratch directory
    /// `HomeGuard` serializes every run through.
    #[test]
    fn vault_lifecycle_covers_missing_set_lock_unlock_and_scoped_keys() {
        let dir = std::env::temp_dir().join(format!("atlas-desktop-vault-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the test's scratch dir");
        let _home = HomeGuard::set(&dir);

        let app = mock_builder()
            .manage(VaultState::default())
            .build(mock_context(noop_assets()))
            .expect("failed to build mock app");
        let handle = app.handle().clone();

        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "missing");

        vault_set_passphrase(handle.clone(), handle.state::<VaultState>(), "correct horse".into()).unwrap();
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "unlocked");

        // A vault already on disk refuses a second `vault_set_passphrase` rather than
        // silently re-keying it.
        let err = vault_set_passphrase(handle.clone(), handle.state::<VaultState>(), "another".into()).unwrap_err();
        assert_eq!(err, "A vault already exists; unlock it instead.");

        vault_put_key(handle.state::<VaultState>(), "global".into(), "sk-real".into()).unwrap();
        vault_put_key(handle.state::<VaultState>(), "project/abc".into(), "sk-proj".into()).unwrap();
        assert_eq!(
            vault_list(handle.state::<VaultState>()).unwrap(),
            vec!["global".to_string(), "project/abc".to_string()]
        );

        vault_lock(handle.state::<VaultState>()).unwrap();
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "locked");

        let locked_err = vault_put_key(handle.state::<VaultState>(), "global".into(), "sk-new".into()).unwrap_err();
        assert_eq!(locked_err, "Vault locked, key not mirrored.");

        let wrong_err = vault_unlock(handle.clone(), handle.state::<VaultState>(), "not it".into()).unwrap_err();
        assert_eq!(wrong_err, "Incorrect passphrase.");
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "locked");

        // The keys stored before locking are still there once unlocked with the right
        // passphrase: they were saved to the snapshot, not only held in memory.
        vault_unlock(handle.clone(), handle.state::<VaultState>(), "correct horse".into()).unwrap();
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "unlocked");
        assert_eq!(
            vault_list(handle.state::<VaultState>()).unwrap(),
            vec!["global".to_string(), "project/abc".to_string()]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

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
