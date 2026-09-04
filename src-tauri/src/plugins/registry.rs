// Where installed plugins live on disk, and the state file (`plugins.json`) that tracks
// which are enabled and where each came from. `list` never drops a plugin whose manifest
// fails validation or whose `api` does not match this app; it reports why instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::manifest::{compatible, Manifest, ATLAS_API_VERSION};

/// `<app data>/plugins`. Each plugin lives at `<plugins_dir>/<id>/`.
pub fn plugins_dir(app_data: &Path) -> PathBuf {
    app_data.join("plugins")
}

fn state_path(app_data: &Path) -> PathBuf {
    plugins_dir(app_data).join("plugins.json")
}

/// Where an installed plugin came from, recorded so a later reinstall or an "update"
/// feature knows how to fetch it again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StateEntry {
    enabled: bool,
    installed_at: String,
    source: SourceRef,
}

type State = HashMap<String, StateEntry>;

fn read_state(app_data: &Path) -> Result<State, String> {
    let path = state_path(app_data);
    if !path.is_file() {
        return Ok(State::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn write_state(app_data: &Path, state: &State) -> Result<(), String> {
    std::fs::create_dir_all(plugins_dir(app_data)).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(state_path(app_data), text).map_err(|e| e.to_string())
}

/// `time`'s RFC 3339 formatting, matching how `update_check` already stamps the update's
/// publish date in `commands/platform.rs`.
fn now_rfc3339() -> Result<String, String> {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| e.to_string())
}

/// Adds or replaces the state entry for `id`. Called once a plugin's files are already
/// on disk, by [`super::install`].
pub(super) fn record_install(app_data: &Path, id: &str, enabled: bool, source: SourceRef) -> Result<(), String> {
    let mut state = read_state(app_data)?;
    state.insert(id.to_string(), StateEntry { enabled, installed_at: now_rfc3339()?, source });
    write_state(app_data, &state)
}

/// One installed plugin as reported to the app: always present once its folder and
/// manifest exist, even when `compatible` is false.
#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub manifest: Manifest,
    pub enabled: bool,
    pub compatible: bool,
    pub reason: Option<String>,
    pub dir: PathBuf,
}

/// Every installed plugin, sorted by id. A plugin whose manifest fails
/// [`Manifest::validate`] or whose `api` does not match [`ATLAS_API_VERSION`] is still
/// listed, with `compatible: false`, `enabled: false` and `reason` set; only a directory
/// with no readable `atlas-plugin.json` at all is skipped, since there is no manifest to
/// report.
pub fn list(app_data: &Path) -> Result<Vec<PluginInfo>, String> {
    let state = read_state(app_data)?;
    let dir = plugins_dir(app_data);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut ids: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    ids.sort();

    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let plugin_dir = dir.join(&id);
        let manifest_path = plugin_dir.join("atlas-plugin.json");
        let Ok(text) = std::fs::read_to_string(&manifest_path) else { continue };
        let Ok(manifest) = Manifest::parse(&text) else { continue };

        let recorded_enabled = state.get(&id).map(|e| e.enabled).unwrap_or(false);
        let reason = manifest.validate(&plugin_dir).err().or_else(|| {
            if compatible(&manifest.api) {
                None
            } else {
                Some(format!(
                    "'{id}' needs API {} but this app provides {ATLAS_API_VERSION}.",
                    manifest.api
                ))
            }
        });
        let is_compatible = reason.is_none();
        out.push(PluginInfo {
            id,
            manifest,
            enabled: is_compatible && recorded_enabled,
            compatible: is_compatible,
            reason,
            dir: plugin_dir,
        });
    }
    Ok(out)
}

/// Enables or disables `id`. Refuses to enable a plugin `list` reports as incompatible,
/// with the same reason `list` would give.
pub fn set_enabled(app_data: &Path, id: &str, enabled: bool) -> Result<(), String> {
    if enabled {
        let info = list(app_data)?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("No plugin '{id}' is installed."))?;
        if !info.compatible {
            return Err(info.reason.unwrap_or_else(|| format!("'{id}' is not compatible.")));
        }
    }
    let mut state = read_state(app_data)?;
    let entry = state.get_mut(id).ok_or_else(|| format!("No plugin '{id}' is installed."))?;
    entry.enabled = enabled;
    write_state(app_data, &state)
}

/// Removes `id`'s folder and state entry. Errs if `id` is not installed rather than
/// silently doing nothing.
pub fn uninstall(app_data: &Path, id: &str) -> Result<(), String> {
    let mut state = read_state(app_data)?;
    if state.remove(id).is_none() {
        return Err(format!("No plugin '{id}' is installed."));
    }
    write_state(app_data, &state)?;
    let dir = plugins_dir(app_data).join(id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "atlas-desktop-plugins-registry-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_manifest(plugin_dir: &Path, json: &str) {
        std::fs::create_dir_all(plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("atlas-plugin.json"), json).unwrap();
        std::fs::write(plugin_dir.join("main.js"), "// stub").unwrap();
    }

    fn compatible_manifest(id: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"Test","version":"1.0.0","description":"d","author":"a","api":">=1.0 <2","main":"main.js","permissions":[],"contributes":{{}}}}"#
        )
    }

    fn incompatible_manifest(id: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"Test","version":"1.0.0","description":"d","author":"a","api":">=2","main":"main.js","permissions":[],"contributes":{{}}}}"#
        )
    }

    #[test]
    fn list_is_empty_with_no_plugins_dir() {
        let app_data = scratch_dir("empty");
        assert!(list(&app_data).unwrap().is_empty());
    }

    #[test]
    fn an_incompatible_plugin_lists_disabled_with_a_reason_and_cannot_be_enabled() {
        let app_data = scratch_dir("incompatible");
        let plugin_dir = plugins_dir(&app_data).join("too-new");
        write_manifest(&plugin_dir, &incompatible_manifest("too-new"));
        record_install(&app_data, "too-new", true, SourceRef { kind: "folder".into(), value: "x".into() }).unwrap();

        let infos = list(&app_data).unwrap();
        assert_eq!(infos.len(), 1);
        assert!(!infos[0].compatible);
        assert!(!infos[0].enabled);
        assert!(infos[0].reason.is_some());

        let err = set_enabled(&app_data, "too-new", true).unwrap_err();
        assert_eq!(err, infos[0].reason.clone().unwrap());
    }

    #[test]
    fn set_enabled_toggles_a_compatible_plugin() {
        let app_data = scratch_dir("toggle");
        let plugin_dir = plugins_dir(&app_data).join("ok");
        write_manifest(&plugin_dir, &compatible_manifest("ok"));
        record_install(&app_data, "ok", true, SourceRef { kind: "folder".into(), value: "x".into() }).unwrap();

        set_enabled(&app_data, "ok", false).unwrap();
        assert!(!list(&app_data).unwrap()[0].enabled);
        set_enabled(&app_data, "ok", true).unwrap();
        assert!(list(&app_data).unwrap()[0].enabled);
    }

    #[test]
    fn set_enabled_on_an_unknown_id_is_an_error() {
        let app_data = scratch_dir("unknown-toggle");
        let err = set_enabled(&app_data, "nope", true).unwrap_err();
        assert_eq!(err, "No plugin 'nope' is installed.");
    }

    #[test]
    fn uninstall_removes_the_folder_and_the_state_entry() {
        let app_data = scratch_dir("uninstall");
        let plugin_dir = plugins_dir(&app_data).join("gone-soon");
        write_manifest(&plugin_dir, &compatible_manifest("gone-soon"));
        record_install(&app_data, "gone-soon", true, SourceRef { kind: "folder".into(), value: "x".into() }).unwrap();
        assert_eq!(list(&app_data).unwrap().len(), 1);

        uninstall(&app_data, "gone-soon").unwrap();
        assert!(list(&app_data).unwrap().is_empty());
        assert!(!plugin_dir.exists());
    }

    #[test]
    fn uninstall_on_an_unknown_id_is_an_error() {
        let app_data = scratch_dir("unknown-uninstall");
        let err = uninstall(&app_data, "nope").unwrap_err();
        assert_eq!(err, "No plugin 'nope' is installed.");
    }
}
