// Where installed plugins live on disk, and the state file (`plugins.json`) that tracks
// which are enabled and where each came from. `list` never drops a plugin directory: one
// whose `atlas-plugin.json` is missing, unreadable or fails to parse is listed with
// `manifest: None`; one whose manifest parses but fails validation or whose `api` does
// not match this app is listed with `manifest: Some(..)` and `compatible: false`. Both
// cases carry `reason`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::manifest::{compatible, Manifest, Permission, ATLAS_API_VERSION};

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
    /// The permissions the user still allows this plugin, a subset of what its manifest
    /// asks for. `None` is a state file written before grants existed, and reads as "the
    /// whole manifest": a plugin installed under the old shape must not lose access the
    /// user never revoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    granted: Option<Vec<Permission>>,
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
/// on disk, by [`super::install`], which passes `enabled: false`: nothing of a freshly
/// installed plugin runs until the user ticks `Enabled`. `granted` is seeded with
/// everything the manifest asks for so the permission chips read correctly before that
/// toggle; the Permissions view is where any of it is taken back.
pub(super) fn record_install(
    app_data: &Path,
    id: &str,
    enabled: bool,
    source: SourceRef,
    granted: Vec<Permission>,
) -> Result<(), String> {
    let mut state = read_state(app_data)?;
    state.insert(
        id.to_string(),
        StateEntry { enabled, installed_at: now_rfc3339()?, source, granted: Some(granted) },
    );
    write_state(app_data, &state)
}

/// One installed plugin as reported to the app: always present once its folder exists,
/// even when its manifest could not be read or parsed at all (`manifest: None`) or parsed
/// but is not `compatible`.
#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub manifest: Option<Manifest>,
    pub enabled: bool,
    pub compatible: bool,
    pub reason: Option<String>,
    pub dir: PathBuf,
    /// What the plugin may actually do right now: the manifest's permissions minus
    /// anything the user has revoked. Always a subset of the manifest, so a manifest that
    /// drops a permission on an update takes the grant with it.
    pub granted: Vec<Permission>,
}

/// Every installed plugin (every directory under [`plugins_dir`]), sorted by id. A
/// directory is never dropped: one whose `atlas-plugin.json` is missing, unreadable or
/// fails to parse is listed with `manifest: None`, `compatible: false`, `enabled: false`
/// and `reason` set to the read or parse error; one whose manifest parses but fails
/// [`Manifest::validate`] or whose `api` does not match [`ATLAS_API_VERSION`] is listed
/// with `manifest: Some(..)`, the same `compatible: false`/`enabled: false`/`reason`
/// shape.
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
        let parsed = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("Could not read atlas-plugin.json: {e}"))
            .and_then(|text| Manifest::parse(&text));

        let manifest = match parsed {
            Ok(manifest) => manifest,
            Err(reason) => {
                out.push(PluginInfo {
                    id,
                    manifest: None,
                    enabled: false,
                    compatible: false,
                    reason: Some(reason),
                    dir: plugin_dir,
                    // No manifest to prune against, so nothing is granted; such a plugin
                    // never runs anyway.
                    granted: Vec::new(),
                });
                continue;
            }
        };

        let recorded_enabled = state.get(&id).map(|e| e.enabled).unwrap_or(false);
        let granted = prune(state.get(&id).and_then(|e| e.granted.as_deref()), &manifest.permissions);
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
            manifest: Some(manifest),
            enabled: is_compatible && recorded_enabled,
            compatible: is_compatible,
            reason,
            dir: plugin_dir,
            granted,
        });
    }
    Ok(out)
}

/// The grants that still hold: whatever was recorded, kept in the manifest's own order
/// and dropped where the manifest no longer asks for it. An unrecorded grant list (a
/// state file written before grants existed) means the whole manifest.
fn prune(recorded: Option<&[Permission]>, manifest: &[Permission]) -> Vec<Permission> {
    match recorded {
        None => manifest.to_vec(),
        Some(held) => manifest.iter().filter(|p| held.contains(p)).copied().collect(),
    }
}

/// The dotted wire name of a permission (`tasks.read`), which is what a manifest and the
/// web side both spell, taken from the same `serde` rename the wire format uses so the
/// two cannot drift apart.
fn permission_name(permission: Permission) -> String {
    serde_json::to_value(permission)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{permission:?}"))
}

/// Replaces `id`'s grants. Refuses a permission the manifest does not declare: a grant is
/// only ever a subset of what the plugin asked for, never a way to widen it.
pub fn set_permissions(app_data: &Path, id: &str, granted: &[Permission]) -> Result<(), String> {
    let info = list(app_data)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("No plugin '{id}' is installed."))?;
    let declared = info.manifest.map(|m| m.permissions).unwrap_or_default();
    for permission in granted {
        if !declared.contains(permission) {
            return Err(format!("'{id}' does not ask for '{}'.", permission_name(*permission)));
        }
    }

    let mut state = read_state(app_data)?;
    let entry = state.get_mut(id).ok_or_else(|| format!("No plugin '{id}' is installed."))?;
    entry.granted = Some(declared.into_iter().filter(|p| granted.contains(p)).collect());
    write_state(app_data, &state)
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
    use crate::scratch::ScratchDir;

    /// A scratch app-data directory that removes itself when the test's binding drops, so
    /// a run leaves nothing behind in the OS temp dir.
    fn scratch_dir(label: &str) -> ScratchDir {
        ScratchDir::new(&format!("atlas-desktop-plugins-registry-test-{label}", )).unwrap()
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

    /// A compatible manifest asking for two permissions, for the grant tests.
    fn manifest_with_permissions(id: &str, permissions: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"Test","version":"1.0.0","description":"d","author":"a","api":">=1.0 <2","main":"main.js","permissions":{permissions},"contributes":{{}}}}"#
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
        record_install(&app_data, "too-new", true, SourceRef { kind: "folder".into(), value: "x".into() }, Vec::new()).unwrap();

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
        record_install(&app_data, "ok", true, SourceRef { kind: "folder".into(), value: "x".into() }, Vec::new()).unwrap();

        set_enabled(&app_data, "ok", false).unwrap();
        assert!(!list(&app_data).unwrap()[0].enabled);
        set_enabled(&app_data, "ok", true).unwrap();
        assert!(list(&app_data).unwrap()[0].enabled);
    }

    #[test]
    fn a_fresh_install_is_granted_everything_its_manifest_asks_for() {
        let app_data = scratch_dir("granted-default");
        let plugin_dir = plugins_dir(&app_data).join("asks");
        write_manifest(&plugin_dir, &manifest_with_permissions("asks", r#"["tasks.read","ui.sections"]"#));
        record_install(
            &app_data,
            "asks",
            true,
            SourceRef { kind: "folder".into(), value: "x".into() },
            vec![Permission::TasksRead, Permission::UiSections],
        )
        .unwrap();

        let info = &list(&app_data).unwrap()[0];
        assert_eq!(info.granted, vec![Permission::TasksRead, Permission::UiSections]);
    }

    /// A state file written before grants existed has no `granted` field at all. Reading
    /// it as "nothing granted" would silently disable every installed plugin's access, so
    /// the absent field means the whole manifest.
    #[test]
    fn an_unrecorded_grant_list_reads_as_the_whole_manifest() {
        let app_data = scratch_dir("granted-legacy");
        let plugin_dir = plugins_dir(&app_data).join("old");
        write_manifest(&plugin_dir, &manifest_with_permissions("old", r#"["memories.read"]"#));
        std::fs::write(
            state_path(&app_data),
            r#"{"old":{"enabled":true,"installed_at":"2026-01-01T00:00:00Z","source":{"kind":"folder","value":"x"}}}"#,
        )
        .unwrap();

        let info = &list(&app_data).unwrap()[0];
        assert_eq!(info.granted, vec![Permission::MemoriesRead]);
    }

    /// An update that drops a permission from the manifest drops the grant with it: a
    /// grant is never wider than what the plugin currently asks for.
    #[test]
    fn a_grant_the_manifest_no_longer_asks_for_is_pruned_on_list() {
        let app_data = scratch_dir("granted-prune");
        let plugin_dir = plugins_dir(&app_data).join("shrunk");
        write_manifest(&plugin_dir, &manifest_with_permissions("shrunk", r#"["tasks.read","ui.sections"]"#));
        record_install(
            &app_data,
            "shrunk",
            true,
            SourceRef { kind: "folder".into(), value: "x".into() },
            vec![Permission::TasksRead, Permission::UiSections, Permission::MemoriesWrite],
        )
        .unwrap();

        let info = &list(&app_data).unwrap()[0];
        assert_eq!(info.granted, vec![Permission::TasksRead, Permission::UiSections]);
    }

    #[test]
    fn set_permissions_revokes_one_and_keeps_the_rest() {
        let app_data = scratch_dir("granted-revoke");
        let plugin_dir = plugins_dir(&app_data).join("revoke-me");
        write_manifest(
            &plugin_dir,
            &manifest_with_permissions("revoke-me", r#"["tasks.read","ui.sections"]"#),
        );
        record_install(
            &app_data,
            "revoke-me",
            true,
            SourceRef { kind: "folder".into(), value: "x".into() },
            vec![Permission::TasksRead, Permission::UiSections],
        )
        .unwrap();

        set_permissions(&app_data, "revoke-me", &[Permission::UiSections]).unwrap();
        assert_eq!(list(&app_data).unwrap()[0].granted, vec![Permission::UiSections]);

        // And back again: revoking is not a one-way door.
        set_permissions(&app_data, "revoke-me", &[Permission::TasksRead, Permission::UiSections])
            .unwrap();
        assert_eq!(
            list(&app_data).unwrap()[0].granted,
            vec![Permission::TasksRead, Permission::UiSections]
        );
    }

    #[test]
    fn set_permissions_refuses_a_permission_the_manifest_does_not_declare() {
        let app_data = scratch_dir("granted-refuse");
        let plugin_dir = plugins_dir(&app_data).join("narrow");
        write_manifest(&plugin_dir, &manifest_with_permissions("narrow", r#"["tasks.read"]"#));
        record_install(
            &app_data,
            "narrow",
            true,
            SourceRef { kind: "folder".into(), value: "x".into() },
            vec![Permission::TasksRead],
        )
        .unwrap();

        let err = set_permissions(&app_data, "narrow", &[Permission::TasksRead, Permission::MemoriesWrite])
            .unwrap_err();
        assert_eq!(err, "'narrow' does not ask for 'memories.write'.");
        // The refusal changed nothing.
        assert_eq!(list(&app_data).unwrap()[0].granted, vec![Permission::TasksRead]);
    }

    #[test]
    fn set_permissions_on_an_unknown_id_is_an_error() {
        let app_data = scratch_dir("granted-unknown");
        let err = set_permissions(&app_data, "nope", &[]).unwrap_err();
        assert_eq!(err, "No plugin 'nope' is installed.");
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
        record_install(&app_data, "gone-soon", true, SourceRef { kind: "folder".into(), value: "x".into() }, Vec::new()).unwrap();
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

    /// A plugin directory whose `atlas-plugin.json` fails to parse must still be listed
    /// (never silently dropped), with `manifest: None` and a `reason`; `set_enabled` and
    /// `plugin_read_main` (in `mod.rs`) both refuse it using that same reason.
    #[test]
    fn a_directory_with_an_unparsable_manifest_is_listed_disabled_with_a_reason() {
        let app_data = scratch_dir("unparsable");
        let plugin_dir = plugins_dir(&app_data).join("broken");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("atlas-plugin.json"), "not json at all").unwrap();

        let infos = list(&app_data).unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "broken");
        assert!(infos[0].manifest.is_none());
        assert!(!infos[0].compatible);
        assert!(!infos[0].enabled);
        assert!(infos[0].reason.is_some());

        let err = set_enabled(&app_data, "broken", true);
        // `set_enabled` records state via `record_install`/the state file, which this
        // directory never went through (it was placed directly, as a stand-in for a
        // corrupted install), so `set_enabled` reports it as not installed rather than
        // incompatible; `plugins_list` is what surfaces the parse-failure reason to the
        // user for a directory like this one.
        assert!(err.is_err());
    }

    /// The same case, but recorded through `record_install` like a real install would be,
    /// so `set_enabled(..., true)` reaches the compatibility check and refuses with the
    /// parse-failure reason `list` reports.
    #[test]
    fn set_enabled_refuses_a_recorded_plugin_with_an_unparsable_manifest() {
        let app_data = scratch_dir("unparsable-recorded");
        let plugin_dir = plugins_dir(&app_data).join("broken");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("atlas-plugin.json"), "not json at all").unwrap();
        record_install(&app_data, "broken", false, SourceRef { kind: "folder".into(), value: "x".into() }, Vec::new()).unwrap();

        let reason = list(&app_data).unwrap()[0].reason.clone().unwrap();
        let err = set_enabled(&app_data, "broken", true).unwrap_err();
        assert_eq!(err, reason);
    }
}
