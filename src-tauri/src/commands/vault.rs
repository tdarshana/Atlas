// ---- Vault (stronghold) ----
//
// Used as a plain Rust library, not as a registered Tauri plugin: the passphrase and
// every scoped key stay on this side of the IPC boundary, and the webview never invokes
// `tauri-plugin-stronghold`'s own commands, so no stronghold capability entry is needed.
// A vault holds one Stronghold client (`VAULT_CLIENT`); a scope's key is a record in
// that client's own key/value store, which (unlike the top-level `Stronghold::store()`)
// is part of what `Stronghold::save` commits to the snapshot file.

use std::path::PathBuf;
use std::sync::Mutex;

use atlas_core::backend::{ProjectBackend, StatusBackend};
use atlas_core::models::ProjectExtraction;
use tauri::{Manager, Runtime};
use tauri_plugin_stronghold::kdf::KeyDerivation;
use tauri_plugin_stronghold::stronghold::Stronghold;

use super::platform::daemon_backend;

/// The vault's snapshot file, in the app data dir.
const VAULT_FILE: &str = "atlas.hold";
/// The argon2 salt beside it. Generated once, on the first `vault_set_passphrase`; the
/// salt itself is not secret, only the passphrase is.
const VAULT_SALT_FILE: &str = "atlas-vault.salt";
/// The one Stronghold client every scoped key is stored under.
const VAULT_CLIENT: &[u8] = b"atlas";

/// The actor the daemon records a mirrored key under, the same label the webview's own
/// writes carry.
const ACTOR: &str = "desktop";

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

/// The exact length `KeyDerivation::argon2` requires the salt file to be
/// (`tauri-plugin-stronghold`'s own `HASH_LENGTH`). A mismatch otherwise panics deep
/// inside that helper's `salt.clone_from_slice(&tmp)`, so a truncated or otherwise
/// corrupt `atlas-vault.salt` is checked for here and reported as the same kind of
/// plain sentence every other vault error uses.
const VAULT_SALT_LEN: u64 = 32;

fn check_salt_file(salt_path: &std::path::Path) -> Result<(), String> {
    if salt_path.is_file() {
        let len = std::fs::metadata(salt_path).map_err(|e| e.to_string())?.len();
        if len != VAULT_SALT_LEN {
            return Err("The vault salt file is damaged.".to_string());
        }
    }
    Ok(())
}

/// Takes the unlocked vault out of `state`, runs `f` on a blocking thread (where the
/// scrypt-backed snapshot decrypt/re-encrypt actually happens; see [`VaultState`]), then
/// puts it back before returning `f`'s result either way, so a failed operation never
/// locks the vault as a side effect. Errs with the sentence every vault command already
/// used when there is nothing to take.
async fn with_vault<T, F>(state: &VaultState, f: F) -> Result<T, String>
where
    F: FnOnce(&Stronghold) -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    let stronghold = {
        let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.take().ok_or_else(|| "Vault locked, key not mirrored.".to_string())?
    };
    let (stronghold, result) = tauri::async_runtime::spawn_blocking(move || {
        let result = f(&stronghold);
        (stronghold, result)
    })
    .await
    .map_err(|e| e.to_string())?;
    *state.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(stronghold);
    result
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
///
/// `async`, with the actual snapshot work in `spawn_blocking`: `Stronghold::new` wraps
/// the snapshot's file key with scrypt, which is not cheap (about a second, tuned for
/// release; far longer unoptimised under `cargo test`), and every vault command runs
/// that on the caller's thread. On the main thread that would freeze the window for the
/// whole call; off it, on a blocking-pool thread, the webview stays responsive.
#[tauri::command]
pub async fn vault_set_passphrase<R: Runtime>(
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
    let salt_path = vault_salt_path(&app)?;
    check_salt_file(&salt_path)?;
    let stronghold = tauri::async_runtime::spawn_blocking(move || {
        let key = KeyDerivation::argon2(&passphrase, &salt_path);
        let stronghold = Stronghold::new(&path, key).map_err(|e| e.to_string())?;
        stronghold.create_client(VAULT_CLIENT).map_err(|e| e.to_string())?;
        stronghold.save().map_err(|e| e.to_string())?;
        Ok::<Stronghold, String>(stronghold)
    })
    .await
    .map_err(|e| e.to_string())??;
    *vault.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(stronghold);
    Ok(())
}

/// Opens the existing `atlas.hold` with an argon2 hash of `passphrase`. A wrong
/// passphrase fails to decrypt the snapshot, which is reported as a plain sentence
/// rather than the crypto error underneath it. See [`vault_set_passphrase`] for why this
/// is `async` with the work in `spawn_blocking`.
#[tauri::command]
pub async fn vault_unlock<R: Runtime>(
    app: tauri::AppHandle<R>,
    vault: tauri::State<'_, VaultState>,
    passphrase: String,
) -> Result<(), String> {
    let path = vault_path(&app)?;
    if !path.exists() {
        return Err("No vault exists yet; set a passphrase first.".to_string());
    }
    let salt_path = vault_salt_path(&app)?;
    check_salt_file(&salt_path)?;
    let stronghold = tauri::async_runtime::spawn_blocking(move || {
        let key = KeyDerivation::argon2(&passphrase, &salt_path);
        let stronghold = Stronghold::new(&path, key).map_err(|_| "Incorrect passphrase.".to_string())?;
        stronghold
            .load_client(VAULT_CLIENT)
            .or_else(|_| stronghold.create_client(VAULT_CLIENT))
            .map_err(|e| e.to_string())?;
        Ok::<Stronghold, String>(stronghold)
    })
    .await
    .map_err(|e| e.to_string())??;
    *vault.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(stronghold);
    Ok(())
}

/// Saves the vault, then drops it from memory. Every key it holds stays on disk,
/// encrypted; only the open, in-memory handle is gone. A failed save leaves the vault
/// exactly as it was (still unlocked, nothing dropped) rather than losing the handle to
/// an in-memory copy that was never written out.
#[tauri::command]
pub async fn vault_lock(vault: tauri::State<'_, VaultState>) -> Result<(), String> {
    let stronghold = {
        let mut guard = vault.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    let Some(stronghold) = stronghold else { return Ok(()) };
    let (stronghold, result) = tauri::async_runtime::spawn_blocking(move || {
        let result = stronghold.save().map_err(|e| e.to_string());
        (stronghold, result)
    })
    .await
    .map_err(|e| e.to_string())?;
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            *vault.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(stronghold);
            Err(e)
        }
    }
}

/// Stores `key` under `scope` (`"global"` or `"project/<id>"`) and saves the vault.
/// Errs with the same sentence the Extraction cards show when the vault is locked.
#[tauri::command]
pub async fn vault_put_key(vault: tauri::State<'_, VaultState>, scope: String, key: String) -> Result<(), String> {
    with_vault(&vault, move |stronghold| {
        let client = stronghold.get_client(VAULT_CLIENT).map_err(|e| e.to_string())?;
        client.store().insert(scope.into_bytes(), key.into_bytes(), None).map_err(|e| e.to_string())?;
        stronghold.save().map_err(|e| e.to_string())
    })
    .await
}

/// Every scope the vault currently holds a key for, sorted.
#[tauri::command]
pub async fn vault_list(vault: tauri::State<'_, VaultState>) -> Result<Vec<String>, String> {
    with_vault(&vault, |stronghold| {
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
    })
    .await
}

/// Sends the vault's stored key for `scope` back to the daemon: `"global"` writes
/// `extraction.api_key` through `PUT /settings`; `"project/<id>"` reads the project's
/// current extraction override (so its other fields survive) and writes the key back
/// through `PUT /projects/{id}/extraction`. Both go through `RemoteBackend`, the same
/// client the CLI uses.
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
    let daemon = daemon_backend()?;
    let refused = |e: atlas_core::AtlasError| format!("The daemon refused the key ({e}).");
    if scope == "global" {
        let mut values = serde_json::Map::new();
        values.insert("extraction.api_key".into(), serde_json::Value::String(key));
        daemon.set_settings(values, ACTOR).await.map_err(refused)?;
        return Ok(());
    }
    let Some(id) = scope.strip_prefix("project/") else {
        return Err(format!("'{scope}' is not a vault scope."));
    };
    let id: uuid::Uuid = id.parse().map_err(|_| format!("'{scope}' is not a vault scope."))?;
    let project = daemon.get_project(id).await.map_err(refused)?;
    let mut extraction: ProjectExtraction = project.extraction.unwrap_or_default();
    extraction.api_key = Some(key);
    daemon.set_project_extraction(id, Some(extraction), ACTOR).await.map_err(refused)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::HomeGuard;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    /// One set, lock, unlock, put, list cycle: enough to prove the vault's basic
    /// lifecycle wiring without paying for the several scrypt rounds the exhaustive
    /// version below costs. See [`vault_lifecycle_covers_missing_set_lock_unlock_and_scoped_keys_exhaustively`]
    /// for the wrong-passphrase, already-exists and multi-scope coverage.
    #[tokio::test]
    async fn vault_lifecycle_covers_missing_set_lock_unlock_and_scoped_keys() {
        let dir = std::env::temp_dir().join(format!("atlas-desktop-vault-test-{}-lean", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the test's scratch dir");
        let _home = HomeGuard::set(&dir);

        let app = mock_builder()
            .manage(VaultState::default())
            .build(mock_context(noop_assets()))
            .expect("failed to build mock app");
        let handle = app.handle().clone();

        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "missing");

        vault_set_passphrase(handle.clone(), handle.state::<VaultState>(), "correct horse".into()).await.unwrap();
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "unlocked");

        vault_lock(handle.state::<VaultState>()).await.unwrap();
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "locked");

        vault_unlock(handle.clone(), handle.state::<VaultState>(), "correct horse".into()).await.unwrap();
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "unlocked");

        vault_put_key(handle.state::<VaultState>(), "global".into(), "sk-real".into()).await.unwrap();
        assert_eq!(vault_list(handle.state::<VaultState>()).await.unwrap(), vec!["global".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The wrong-passphrase, already-exists, locked-write and multi-scope coverage the
    /// lean cycle above does not exercise. Several snapshot encrypt/decrypt cycles, each
    /// paying scrypt's unoptimised debug cost (minutes, not the release-tuned ~1s; see
    /// I2 in `.superpowers/sdd/2026-09-03-phase11-tauri-plugins/final-review-report.md`),
    /// so this stays out of the everyday `cargo test --workspace` gate.
    #[ignore = "slow: several scrypt rounds in debug"]
    #[tokio::test]
    async fn vault_lifecycle_covers_missing_set_lock_unlock_and_scoped_keys_exhaustively() {
        let dir = std::env::temp_dir().join(format!("atlas-desktop-vault-test-{}-thorough", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the test's scratch dir");
        let _home = HomeGuard::set(&dir);

        let app = mock_builder()
            .manage(VaultState::default())
            .build(mock_context(noop_assets()))
            .expect("failed to build mock app");
        let handle = app.handle().clone();

        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "missing");

        vault_set_passphrase(handle.clone(), handle.state::<VaultState>(), "correct horse".into()).await.unwrap();
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "unlocked");

        // A vault already on disk refuses a second `vault_set_passphrase` rather than
        // silently re-keying it.
        let err = vault_set_passphrase(handle.clone(), handle.state::<VaultState>(), "another".into()).await.unwrap_err();
        assert_eq!(err, "A vault already exists; unlock it instead.");

        vault_put_key(handle.state::<VaultState>(), "global".into(), "sk-real".into()).await.unwrap();
        vault_put_key(handle.state::<VaultState>(), "project/abc".into(), "sk-proj".into()).await.unwrap();
        assert_eq!(
            vault_list(handle.state::<VaultState>()).await.unwrap(),
            vec!["global".to_string(), "project/abc".to_string()]
        );

        vault_lock(handle.state::<VaultState>()).await.unwrap();
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "locked");

        let locked_err = vault_put_key(handle.state::<VaultState>(), "global".into(), "sk-new".into()).await.unwrap_err();
        assert_eq!(locked_err, "Vault locked, key not mirrored.");

        let wrong_err = vault_unlock(handle.clone(), handle.state::<VaultState>(), "not it".into()).await.unwrap_err();
        assert_eq!(wrong_err, "Incorrect passphrase.");
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "locked");

        // The keys stored before locking are still there once unlocked with the right
        // passphrase: they were saved to the snapshot, not only held in memory.
        vault_unlock(handle.clone(), handle.state::<VaultState>(), "correct horse".into()).await.unwrap();
        assert_eq!(vault_status(handle.clone(), handle.state::<VaultState>()).unwrap(), "unlocked");
        assert_eq!(
            vault_list(handle.state::<VaultState>()).await.unwrap(),
            vec!["global".to_string(), "project/abc".to_string()]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
