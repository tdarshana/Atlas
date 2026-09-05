//! Claude Code's three places for MCP servers, plus its plugins'.
//!
//! `~/.claude.json` holds the user's own servers at the top level (`user` scope) and,
//! under `projects["<absolute root>"]`, that repository's machine-local servers
//! (`local` scope) with a `disabledMcpServers` list of names. `<root>/.mcp.json` is the
//! file checked into the repository (`project` scope); Claude Code will not start one of
//! its servers until the user approves it, which it records as `enabledMcpjsonServers`
//! and `disabledMcpjsonServers` in the same per-project block.
//!
//! A plugin's servers come from `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/.mcp.json`
//! at the version directory with the newest modification time, and are on when
//! `~/.claude/settings.json` lists `<plugin>@<marketplace>` in `enabledPlugins`. Within a
//! project, Claude Code switches one of them off through the same `disabledMcpServers`
//! list as a local server, under the key `plugin:<plugin>:<server>` (no marketplace).
//! Nothing here edits a plugin: its files belong to whoever published it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::paths::installed_plugins;

use crate::models::{McpServerScope, McpServerSource, Project};

use super::generic_json;
use super::Found;

/// The user's own file, `~/.claude.json`.
pub fn config_path(home: &Path) -> PathBuf {
    home.join(".claude.json")
}

/// The user-level servers: `mcpServers` at the top of `~/.claude.json`. There is no
/// global switch for one, only the per-project `disabledMcpServers` a project listing
/// applies, so `can_toggle` is false here.
pub fn user_servers(home: &Path, found: &mut Found) {
    let path = config_path(home);
    let Some(config) = generic_json::read_file(&path, found) else { return };
    generic_json::collect(
        &config["mcpServers"],
        McpServerSource::Claude,
        McpServerScope::User,
        McpServerScope::User.as_str(),
        &path,
        None,
        None,
        false,
        true,
        &|_| true,
        found,
    );
}

/// One project's machine-local servers, from its block in `~/.claude.json`, gated by that
/// block's `disabledMcpServers`.
pub fn local_servers(home: &Path, project: &Project, found: &mut Found) {
    let path = config_path(home);
    let Some(config) = generic_json::read_file(&path, found) else { return };
    let block = project_block(&config, &project.root_path);
    let disabled: HashSet<String> = names(block.map(|b| &b["disabledMcpServers"]));
    let Some(block) = block else { return };
    generic_json::collect(
        &block["mcpServers"],
        McpServerSource::Claude,
        McpServerScope::Local,
        McpServerScope::Local.as_str(),
        &path,
        Some(project.id),
        None,
        true,
        true,
        &|name| !disabled.contains(name),
        found,
    );
}

/// The servers the repository itself ships in `<root>/.mcp.json`, each on only once the
/// user has approved it (`enabledMcpjsonServers` in `~/.claude.json`).
pub fn project_servers(home: &Path, project: &Project, found: &mut Found) {
    let path = Path::new(&project.root_path).join(".mcp.json");
    let Some(file) = generic_json::read_file(&path, found) else { return };
    let approved: HashSet<String> = match generic_json::read_file(&config_path(home), found) {
        Some(config) => names(project_block(&config, &project.root_path).map(|b| &b["enabledMcpjsonServers"])),
        None => HashSet::new(),
    };
    generic_json::collect(
        &file["mcpServers"],
        McpServerSource::Claude,
        McpServerScope::Project,
        McpServerScope::Project.as_str(),
        &path,
        Some(project.id),
        None,
        true,
        true,
        &|name| approved.contains(name),
        found,
    );
}

/// The key Claude Code files a plugin server under in a project's `disabledMcpServers`:
/// `plugin:<plugin>:<server>`, with `plugin` being the `<marketplace>/<plugin>` label an
/// entry carries, so the marketplace is dropped.
pub fn plugin_switch_key(plugin: &str, name: &str) -> String {
    let plugin = plugin.rsplit('/').next().unwrap_or(plugin);
    format!("plugin:{plugin}:{name}")
}

/// Every installed plugin's `.mcp.json`, at the version directory with the newest
/// modification time. Directories ending in `.clone` or starting with `temp_` are a
/// half-finished install and are skipped, the same rule skill discovery uses.
///
/// With a project, the project's `disabledMcpServers` is applied on top of the plugin's
/// own switch and every server of an enabled plugin can be toggled; without one there is
/// nothing per-project to flip, so the rows only follow their plugin.
pub fn plugin_servers(home: &Path, project: Option<&Project>, found: &mut Found) {
    let enabled = enabled_plugins(home, found);
    let disabled: HashSet<String> = match project {
        Some(p) => generic_json::read_file(&config_path(home), found)
            .map(|config| names(project_block(&config, &p.root_path).map(|b| &b["disabledMcpServers"])))
            .unwrap_or_default(),
        None => HashSet::new(),
    };
    for plugin in installed_plugins(home, &mut found.warnings) {
        let label = plugin.label();
        let path = plugin.version_dir.join(".mcp.json");
        let Some(file) = generic_json::read_file(&path, found) else { continue };
        // Claude Code keys `enabledPlugins` the other way round from the cache tree.
        let on = enabled.contains(&format!("{}@{}", plugin.name, plugin.marketplace));
        generic_json::collect(
            &file["mcpServers"],
            McpServerSource::Plugin,
            McpServerScope::Plugin,
            &label,
            &path,
            None,
            Some(&label),
            on && project.is_some(),
            false,
            &|name| on && !disabled.contains(&plugin_switch_key(&label, name)),
            found,
        );
    }
}

/// The `<plugin>@<marketplace>` keys `~/.claude/settings.json` has switched on.
fn enabled_plugins(home: &Path, found: &mut Found) -> HashSet<String> {
    let path = home.join(".claude/settings.json");
    let Some(settings) = generic_json::read_file(&path, found) else { return HashSet::new() };
    settings["enabledPlugins"]
        .as_object()
        .map(|o| o.iter().filter(|(_, v)| v.as_bool() == Some(true)).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default()
}

/// One project's block in `~/.claude.json`, keyed by its absolute root path.
pub fn project_block<'a>(config: &'a Value, root: &str) -> Option<&'a Value> {
    config.get("projects")?.as_object()?.get(root)
}

/// A JSON array of strings as a set. An absent or malformed list is an empty one: a
/// missing `disabledMcpServers` means nothing is disabled.
fn names(value: Option<&Value>) -> HashSet<String> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}
