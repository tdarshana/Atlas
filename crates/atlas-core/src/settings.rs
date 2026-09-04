use crate::db::Db;
use crate::memories::MemoryRepo;
use crate::models::PluginToolDecl;
use crate::{AtlasError, Result};
use duckdb::params;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// The only keys the settings table accepts. `set_many` rejects anything else.
/// `board.stages` is readable here but not writable: see [`SettingsRepo::set_many`].
pub const SETTING_KEYS: &[&str] = &[
    "extraction.enabled",
    "extraction.base_url",
    "extraction.api_key",
    "extraction.model",
    "extraction.auto_accept_min_confidence",
    "embedding.model",
    "daemon.port",
    "board.stages",
    "board.mirror_tasks_md",
    "ui.theme",
    "ui.theme_pack",
    "ui.font_ui",
    "ui.font_mono",
    "ui.font_size",
    "ui.scale",
    "ui.autostart",
    "ui.global_shortcut",
    "ui.notify.review_pending",
    "ui.notify.workflow_runs",
    "ui.notify.daemon_errors",
    "workflows.docs_migrated",
    "mcp.disabled_tools",
    "access.memory_writers",
    "access.task_movers",
    "access.require_review",
];

/// Every MCP tool name `mcp.disabled_tools` may name. The single source of truth for
/// validation here; `atlas-mcp`'s `TOOL_TABLE` is asserted (by test) to cover the same
/// names, since this crate cannot depend on atlas-mcp to import them directly.
pub const MCP_TOOL_NAMES: &[&str] = &[
    "memory_remember",
    "memory_search",
    "memory_list",
    "memory_forget",
    "memory_review",
    "project_list",
    "project_get",
    "project_connect",
    "project_context",
    "task_create",
    "task_list",
    "task_get",
    "task_claim",
    "task_move",
    "task_comment",
    "task_block",
    "task_update",
    "board_stages",
    "practice_list",
    "practice_get",
    "agent_list",
    "agent_get",
    "agent_save",
    "workflow_list",
    "workflow_get",
    "workflow_run",
    "workflow_status",
    "ingest_transcript",
    "status",
    "framework_docs",
    "skill_list",
    "skill_get",
];

/// Rejects any name outside [`MCP_TOOL_NAMES`]. The rule the global
/// `mcp.disabled_tools` setting (`check_type`, below) and a project's
/// `mcp_disabled_tools` override (`projects::ProjectRepo::update`) both validate
/// against, so a name that would silently gate nothing can never be stored by either.
pub fn validate_mcp_tool_names(names: &[String]) -> Result<()> {
    for n in names {
        if !MCP_TOOL_NAMES.contains(&n.as_str()) && !is_plugin_mcp_tool_name(n) {
            return Err(AtlasError::Invalid(format!("unknown MCP tool name '{n}'")));
        }
    }
    Ok(())
}

/// Whether `n` has the shape `atlas-mcp`'s `plugin_tool_name` builds
/// (`plugin__<id>__<name>`). Plugin tools are gated by the same `mcp.disabled_tools`
/// list as the built-ins, but they are not in [`MCP_TOOL_NAMES`]: the set changes as
/// plugins come and go, and a name has to stay disable-able while the plugin that
/// declares it is not running. The shape, not the live registry, is what is checked, so
/// the setting survives a restart with no app connected.
///
/// The two halves are held to exactly what registration would produce: the id with its
/// dashes already rewritten as underscores, and the tool name unchanged. Anything looser
/// would let a name that can never match a real tool be stored, which is the silent
/// no-op [`validate_mcp_tool_names`] exists to prevent.
fn is_plugin_mcp_tool_name(n: &str) -> bool {
    let Some(rest) = n.strip_prefix("plugin__") else { return false };
    let Some((id, name)) = rest.split_once("__") else { return false };
    is_plugin_id(&id.replace('_', "-")) && is_plugin_tool_name(name)
}

/// At most this many tools may be registered under one plugin id, so a plugin cannot
/// flood every MCP client's tool list.
pub const MAX_PLUGIN_TOOLS: usize = 32;
/// The longest description a plugin tool may carry, in characters. Long enough for a
/// useful sentence or two, short enough that 32 of them stay a reasonable tool list.
pub const MAX_PLUGIN_TOOL_DESCRIPTION: usize = 400;

/// Whether `s` matches `^[a-z0-9][a-z0-9-]{1,63}$` and holds neither a `--` nor a
/// trailing `-`: the shape of a plugin id.
///
/// Both dash rules exist because `atlas-mcp`'s `plugin_tool_name` rewrites `-` as `_`
/// and then joins the id to the tool name with `__`, and its inverse splits on the first
/// `__` after the prefix. An id holding `--` would produce a `__` of its own inside the
/// id half. An id *ending* in `-` is subtler: it rewrites to a trailing `_` that runs
/// straight into the separator, so `ab-` with tool `count` encodes to
/// `plugin__ab___count` and the split lands one character early, recovering
/// `("ab", "_count")` rather than `("ab_", "count")`. Such a tool would be listed to
/// every MCP client and fail every call with `unknown plugin tool`, so the id is refused
/// at registration instead. See [`validate_plugin_id`].
fn is_plugin_id(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() { return false; }
    let rest: Vec<char> = chars.collect();
    (1..=63).contains(&rest.len())
        && rest.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        && !s.contains("--")
        && !s.ends_with('-')
}

/// Whether `s` matches `^[a-z][a-z0-9_]{0,47}$` and holds no `__`: the shape of a plugin
/// tool name. The character set is narrower than a plugin id's, since the name is joined
/// into an MCP tool name where `-` would be ambiguous against the `__` separator, and the
/// doubled underscore is excluded for the same reason [`is_plugin_id`] excludes `--`.
fn is_plugin_tool_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false };
    if !first.is_ascii_lowercase() { return false; }
    let rest: Vec<char> = chars.collect();
    rest.len() <= 47
        && rest.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        && !s.contains("__")
}

/// Checks one plugin id on its own, for the caller that has an id and no decls to check
/// it through: `PUT /api/v1/mcp/plugin-tools/{plugin_id}` with an empty tool set still
/// has to refuse an id it would never accept with tools attached.
pub fn validate_plugin_id(id: &str) -> Result<()> {
    if id.contains("--") {
        return Err(AtlasError::Invalid(format!("plugin id {id} cannot contain \"--\" because it would be ambiguous as an MCP tool name")));
    }
    if id.ends_with('-') {
        return Err(AtlasError::Invalid(format!("plugin id {id} cannot end with \"-\" because it would be ambiguous as an MCP tool name")));
    }
    if !is_plugin_id(id) {
        return Err(AtlasError::Invalid(format!("'{id}' is not a plugin id: 2 to 64 characters of a-z, 0-9 and '-', starting with a letter or digit")));
    }
    Ok(())
}

/// Checks a plugin's declared MCP tools before the daemon registers them. Every rule
/// here exists because the decl reaches an MCP client's tool list unchanged: the id and
/// name shapes keep `plugin_tool_name` reversible, the description bound keeps a tool
/// list readable, and `args` has to be a JSON Schema object because that is what
/// `inputSchema` means.
pub fn validate_plugin_tool_decls(decls: &[PluginToolDecl]) -> Result<()> {
    let mut per_plugin: HashMap<&str, Vec<&str>> = HashMap::new();
    for d in decls {
        validate_plugin_id(&d.plugin_id)?;
        // Rejected with its own message rather than folded into the shape error: an
        // author who used `__` as a word separator needs to be told which rule bit.
        if d.name.contains("__") {
            return Err(AtlasError::Invalid(format!("plugin tool name {} cannot contain \"__\" because it would be ambiguous as an MCP tool name", d.name)));
        }
        if !is_plugin_tool_name(&d.name) {
            return Err(AtlasError::Invalid(format!("'{}' is not a plugin tool name: 1 to 48 characters of a-z, 0-9 and '_', starting with a letter", d.name)));
        }
        if d.description.trim().is_empty() {
            return Err(AtlasError::Invalid(format!("plugin tool '{}' needs a description", d.name)));
        }
        if d.description.chars().count() > MAX_PLUGIN_TOOL_DESCRIPTION {
            return Err(AtlasError::Invalid(format!("plugin tool '{}' description is over {MAX_PLUGIN_TOOL_DESCRIPTION} characters", d.name)));
        }
        if d.args.get("type").and_then(|v| v.as_str()) != Some("object") {
            return Err(AtlasError::Invalid(format!("plugin tool '{}' args must be a JSON Schema object with \"type\": \"object\"", d.name)));
        }
        let names = per_plugin.entry(d.plugin_id.as_str()).or_default();
        if names.contains(&d.name.as_str()) {
            return Err(AtlasError::Invalid(format!("plugin '{}' declares the tool '{}' twice", d.plugin_id, d.name)));
        }
        names.push(&d.name);
        if names.len() > MAX_PLUGIN_TOOLS {
            return Err(AtlasError::Invalid(format!("plugin '{}' declares more than {MAX_PLUGIN_TOOLS} tools", d.plugin_id)));
        }
    }
    Ok(())
}

/// `mcp.disabled_tools` default when the setting is unset: `project_connect` writes a
/// project row on any local caller's say-so, and `memory_review` decides which pending
/// memories become active, so both stay opt-in rather than exposed to every agent by
/// default.
pub const DEFAULT_DISABLED_MCP_TOOLS: &[&str] = &["project_connect", "memory_review"];

const API_KEY: &str = "extraction.api_key";
const BASE_URL: &str = "extraction.base_url";
const STAGES: &str = "board.stages";
pub(crate) const MASKED: &str = "***";

/// The modifier names `tauri-plugin-global-shortcut` accepts, lower-cased for a
/// case-insensitive match.
const ACCELERATOR_MODIFIERS: &[&str] =
    &["cmd", "command", "cmdorctrl", "commandorcontrol", "ctrl", "control", "alt", "altgr", "option", "shift", "super", "meta"];

/// Whether `s` is a Tauri accelerator: one or more modifiers and exactly one trailing
/// key, joined by `+` (e.g. `"CmdOrCtrl+Shift+K"`). Checked here, before `ui.global_shortcut`
/// ever reaches the desktop app's `shortcut_set` command, so a value that could never
/// register with the global-shortcut plugin is refused at the settings boundary instead
/// of failing silently in the running app.
pub fn validate_accelerator(s: &str) -> Result<()> {
    let bad = || AtlasError::Invalid(format!("'{s}' is not a valid shortcut: use one or more modifiers and one key joined by '+', e.g. 'CmdOrCtrl+Shift+K'"));
    let parts: Vec<&str> = s.trim().split('+').map(str::trim).collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return Err(bad());
    }
    let (modifiers, key) = parts.split_at(parts.len() - 1);
    for m in modifiers {
        if !ACCELERATOR_MODIFIERS.contains(&m.to_lowercase().as_str()) {
            return Err(bad());
        }
    }
    if ACCELERATOR_MODIFIERS.contains(&key[0].to_lowercase().as_str()) {
        return Err(bad());
    }
    Ok(())
}

/// Whether two base urls name the same endpoint. A trailing slash is not a change
/// of endpoint, and `LlmClient` strips one anyway before building its request url.
pub(crate) fn same_endpoint(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/') == b.trim().trim_end_matches('/')
}

// ---- theme packs (`ui.theme_pack`) ----
//
// A pack is `{ "name", "base": "dark"|"light", "tokens": { "--token": "value" } }`. It
// may only override colour tokens and the two radius tokens (`--radius-sm`,
// `--radius-md`); the allowed names are read out of the same token files the desktop
// app ships (`colors.css`, `spacing.css`) rather than hand-copied here, so an added or
// renamed token follows automatically instead of silently going stale.

const COLORS_CSS: &str = include_str!("../../../src/lib/ds/tokens/colors.css");
const SPACING_CSS: &str = include_str!("../../../src/lib/ds/tokens/spacing.css");

/// The only two length tokens a pack may override, per the design requirements
/// (inputs/buttons and cards/popovers). Checked against `spacing.css` itself, not just
/// asserted, so a rename there fails loudly instead of this list going stale.
const RADIUS_TOKEN_NAMES: &[&str] = &["--radius-sm", "--radius-md"];

/// A pack's JSON text is capped so a client cannot write an unbounded blob into the
/// setting (the token allow list itself is finite, but `name` is a free string).
/// Matches `MAX_THEME_PACK_BYTES` in `src/lib/shell/theme-pack.ts`.
const MAX_THEME_PACK_BYTES: usize = 16 * 1024;

/// `name`'s own cap, tighter than the whole-pack one since it is shown in the Theme
/// select. Matches `MAX_THEME_PACK_NAME_CHARS` in `src/lib/shell/theme-pack.ts`.
const MAX_THEME_PACK_NAME_CHARS: usize = 64;

/// Strips `/* ... */` comments out of a CSS file so a colon inside a comment (e.g. an
/// "AA fix: ..." note) is never mistaken for part of a declaration.
fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = match rest[start + 2..].find("*/") {
            Some(end) => &rest[start + 2 + end + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Every `--name: value;` custom property declared in a CSS file, in source order.
fn parse_declarations(css: &str) -> Vec<(String, String)> {
    let cleaned = strip_css_comments(css);
    let mut out = Vec::new();
    for chunk in cleaned.split(';') {
        let chunk = chunk.trim();
        let Some(rest) = chunk.strip_prefix("--") else { continue };
        let Some((name, value)) = rest.split_once(':') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if !name.is_empty() && !value.is_empty() {
            out.push((format!("--{name}"), value.to_string()));
        }
    }
    out
}

/// The set of custom properties a theme pack may override: every token in
/// `colors.css` whose declared value is a literal colour (not a `var()` alias or a
/// shadow), plus the two named radius tokens, checked present in `spacing.css`.
fn theme_token_allowlist() -> HashSet<String> {
    let mut names: HashSet<String> = HashSet::new();
    for (name, value) in parse_declarations(COLORS_CSS) {
        if is_css_color(&value) {
            names.insert(name);
        }
    }
    for radius in RADIUS_TOKEN_NAMES {
        if parse_declarations(SPACING_CSS).iter().any(|(name, _)| name == radius) {
            names.insert((*radius).to_string());
        }
    }
    names
}

/// Whether `value` is a CSS colour: `#rgb`, `#rrggbb`, `#rrggbbaa`, or a
/// `rgb()`/`rgba()`/`hsl()`/`hsla()`/`oklch()` function call. Not a full CSS grammar
/// check, just enough to keep a pack from writing an arbitrary declaration into a
/// custom property.
fn is_css_color(value: &str) -> bool {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    ["rgb(", "rgba(", "hsl(", "hsla(", "oklch("].iter().any(|p| v.starts_with(p) && v.ends_with(')'))
}

/// Whether `value` is a plain CSS length (`3px`, `0.5rem`, `0`), the shape the two
/// radius tokens take. Matches `isCssLength`'s `/^\d+(\.\d+)?(px|rem|em|%)$/` in
/// `src/lib/shell/theme-pack.ts`: a digit must lead, and a `.` must be followed by at
/// least one digit, so `.px` and `3.` are rejected along with everything else that
/// is not a plain number.
fn is_css_length(value: &str) -> bool {
    let v = value.trim();
    if v == "0" {
        return true;
    }
    for unit in ["px", "rem", "em", "%"] {
        if let Some(num) = v.strip_suffix(unit) {
            let (int_part, frac_part) = match num.split_once('.') {
                Some((i, f)) => (i, Some(f)),
                None => (num, None),
            };
            let int_ok = !int_part.is_empty() && int_part.chars().all(|c| c.is_ascii_digit());
            let frac_ok = frac_part.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()));
            if int_ok && frac_ok {
                return true;
            }
        }
    }
    false
}

/// Validates a theme pack's JSON text against the shape `{ name, base, tokens }`, the
/// known token allow list, and colour/length syntax for each value, before it is ever
/// stored: a pack that failed this can never reach `documentElement.style`.
fn validate_theme_pack(s: &str) -> Result<()> {
    let bad = |msg: String| AtlasError::Invalid(msg);
    if s.len() > MAX_THEME_PACK_BYTES {
        return Err(bad(format!("ui.theme_pack must be at most {MAX_THEME_PACK_BYTES} bytes of JSON")));
    }
    let v: Value = serde_json::from_str(s).map_err(|e| bad(format!("ui.theme_pack must be valid JSON: {e}")))?;
    let obj = v.as_object().ok_or_else(|| bad("ui.theme_pack must be a JSON object".into()))?;
    match obj.get("name").and_then(|n| n.as_str()) {
        Some(n) if n.trim().is_empty() => return Err(bad("ui.theme_pack.name must be a non-empty string".into())),
        Some(n) if n.chars().count() > MAX_THEME_PACK_NAME_CHARS => {
            return Err(bad(format!("ui.theme_pack.name must be at most {MAX_THEME_PACK_NAME_CHARS} characters")));
        }
        Some(_) => {}
        None => return Err(bad("ui.theme_pack.name must be a non-empty string".into())),
    }
    match obj.get("base").and_then(|b| b.as_str()) {
        Some("dark") | Some("light") => {}
        _ => return Err(bad("ui.theme_pack.base must be \"dark\" or \"light\"".into())),
    }
    let tokens = obj.get("tokens").and_then(|t| t.as_object()).ok_or_else(|| bad("ui.theme_pack.tokens must be an object".into()))?;
    let allowed = theme_token_allowlist();
    for (name, value) in tokens {
        if !allowed.contains(name.as_str()) {
            return Err(bad(format!("ui.theme_pack.tokens has an unknown token '{name}'")));
        }
        let value = value.as_str().ok_or_else(|| bad(format!("ui.theme_pack.tokens['{name}'] must be a string")))?;
        let ok = if RADIUS_TOKEN_NAMES.contains(&name.as_str()) { is_css_length(value) } else { is_css_color(value) };
        if !ok {
            let want = if RADIUS_TOKEN_NAMES.contains(&name.as_str()) { "a CSS length" } else { "a CSS colour" };
            return Err(bad(format!("ui.theme_pack.tokens['{name}'] must be {want}")));
        }
    }
    Ok(())
}

/// Rejects a value whose JSON type does not match the key. Without this a client could
/// store, say, an object under `extraction.api_key`, and the extraction worker would then
/// read a value it cannot use out of the database. `Value::Null` means "unset" and is
/// accepted for every key, so the map `get_all` returns can be sent straight back.
fn check_type(key: &str, value: &Value) -> Result<()> {
    let wrong = |want: &str| Err(AtlasError::Invalid(format!("setting '{key}' must be {want}")));
    if value.is_null() {
        return Ok(());
    }
    match key {
        "extraction.api_key" | "extraction.base_url" | "extraction.model" | "embedding.model" => {
            if !value.is_string() {
                return wrong("a string");
            }
        }
        "extraction.enabled" => {
            if !value.is_boolean() {
                return wrong("a boolean");
            }
        }
        "extraction.auto_accept_min_confidence" => match value.as_f64() {
            Some(n) if (0.0..=1.0).contains(&n) => {}
            Some(_) => return wrong("a number between 0 and 1"),
            None => return wrong("a number between 0 and 1"),
        },
        "daemon.port" => match value.as_u64() {
            Some(n) if (1..=65535).contains(&n) => {}
            _ => return wrong("a port number between 1 and 65535"),
        },
        // The board refuses a stage list the repository could not use, before it is
        // stored, so a bad list can never reach a task move.
        "board.stages" => crate::board::parse_stages(value.clone()).map(|_| ())?,
        // A latch, not a preference: `workflow::migrate_docs` sets it once the Markdown
        // workflow documents have become workflows, and reads it to know not to run again.
        "board.mirror_tasks_md" | "workflows.docs_migrated" => {
            if !value.is_boolean() {
                return wrong("a boolean");
            }
        }
        // The desktop mirrors its theme here so a second client opens on the same ramp.
        "ui.theme" => match value.as_str() {
            Some("dark") | Some("light") => {}
            _ => return wrong("\"dark\" or \"light\""),
        },
        // The imported theme pack's JSON text, or null for none. Validated against the
        // known token names and colour/length syntax so a bad pack never reaches
        // `documentElement.style` on any client that reads this setting.
        "ui.theme_pack" => match value.as_str() {
            Some(s) => validate_theme_pack(s)?,
            None => return wrong("a JSON string or null"),
        },
        "ui.font_ui" => match value.as_str() {
            Some("system") | Some("inter") | Some("jetbrains-mono") => {}
            _ => return wrong("\"system\", \"inter\" or \"jetbrains-mono\""),
        },
        "ui.font_mono" => match value.as_str() {
            Some("jetbrains-mono") | Some("system-mono") => {}
            _ => return wrong("\"jetbrains-mono\" or \"system-mono\""),
        },
        // The UI size scale's base step; 11/12/13 per the design requirements (15 is
        // the largest step, never the default and not offered here).
        "ui.font_size" => match value.as_u64() {
            Some(11) | Some(12) | Some(13) => {}
            _ => return wrong("11, 12 or 13"),
        },
        // The whole app's zoom level as an integer percent; 100 is the default.
        "ui.scale" => match value.as_u64() {
            Some(80) | Some(90) | Some(100) | Some(110) | Some(125) | Some(150) => {}
            _ => return wrong("80, 90, 100, 110, 125 or 150"),
        },
        // Mirrors of desktop-only Tauri plugin state (autostart, the notification
        // toggles), kept here so a second client opens on the same settings.
        "ui.autostart" | "ui.notify.review_pending" | "ui.notify.workflow_runs" | "ui.notify.daemon_errors" => {
            if !value.is_boolean() {
                return wrong("a boolean");
            }
        }
        // Checked as an accelerator string, not just "a string": a shortcut the
        // global-shortcut plugin could never register would otherwise be stored and
        // fail silently in the running app.
        "ui.global_shortcut" => match value.as_str() {
            Some(s) => validate_accelerator(s)?,
            None => return wrong("a keyboard accelerator string"),
        },
        // A tool name outside the known list can never match a real tool, so it would
        // silently do nothing while looking like it disabled something. A plugin tool's
        // name is admitted by its shape instead: see `is_plugin_mcp_tool_name`.
        "mcp.disabled_tools" => match value.as_array() {
            Some(names) => {
                for n in names {
                    match n.as_str() {
                        Some(n) if MCP_TOOL_NAMES.contains(&n) || is_plugin_mcp_tool_name(n) => {}
                        Some(n) => return wrong(&format!("an array of known MCP tool names (got '{n}')")),
                        None => return wrong("an array of strings"),
                    }
                }
            }
            None => return wrong("an array of strings"),
        },
        // The global agent-access defaults `projects::effective_access` fills a
        // project's unset `memory_writers`/`task_movers` from. Same shape as the
        // project-level field: null (any actor) or 1 to 64 non-empty, non-duplicate
        // actor strings of at most 128 characters each.
        "access.memory_writers" | "access.task_movers" => match value.as_array() {
            Some(items) => {
                if items.is_empty() || items.len() > 64 {
                    return wrong("null or an array of 1 to 64 actor strings");
                }
                let mut seen = HashSet::new();
                for item in items {
                    match item.as_str() {
                        Some(s) if !s.is_empty() && s.chars().count() <= 128 => {
                            if !seen.insert(s) {
                                return wrong("an array with no duplicate actor strings");
                            }
                        }
                        _ => return wrong("an array of non-empty actor strings of at most 128 characters each"),
                    }
                }
            }
            None => return wrong("null or an array of actor strings"),
        },
        // A floor a project's own `require_review` can only raise; see
        // `projects::effective_access`.
        "access.require_review" => {
            if !value.is_boolean() {
                return wrong("a boolean");
            }
        }
        _ => {}
    }
    Ok(())
}

pub struct SettingsRepo<'a> {
    db: &'a Db,
}

impl<'a> SettingsRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// The unmasked value for a single key, for the extraction worker. `None` when unset.
    pub fn get_raw(&self, key: &str) -> Result<Option<Value>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare("select value::text from settings where key = ?")?;
            let mut rows = st.query(params![key])?;
            match rows.next()? {
                Some(r) => {
                    let text: String = r.get(0)?;
                    Ok(Some(serde_json::from_str(&text)?))
                }
                None => Ok(None),
            }
        })
    }

    /// Every known key, present even when unset (as `Value::Null`); `extraction.api_key`
    /// is masked to `"***"` when it holds a non-empty value.
    pub fn get_all(&self) -> Result<Map<String, Value>> {
        let mut out = Map::new();
        for key in SETTING_KEYS {
            out.insert((*key).to_string(), self.get_raw(key)?.unwrap_or(Value::Null));
        }
        if out.get(API_KEY).and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()) {
            out.insert(API_KEY.to_string(), Value::String(MASKED.to_string()));
        }
        Ok(out)
    }

    /// Upserts each given key. Rejects the whole call with `Invalid` if any key is
    /// unknown, is `board.stages`, or holds a value of the wrong type, before writing
    /// anything. Ignores
    /// `extraction.api_key == "***"` so a masked value read back from `get_all` and
    /// sent straight back does not clobber the real key.
    ///
    /// Answers whether the stored api key was cleared. The daemon has no
    /// authentication of its own, so any process that can reach the loopback port can
    /// repoint `extraction.base_url` and then have the daemon send the stored key to
    /// an endpoint of its choosing. A key entered against one endpoint is therefore
    /// not a key for another: changing the base url without supplying a new key
    /// clears the stored one, and the next model call fails until it is entered again.
    pub fn set_many(&self, values: &Map<String, Value>, actor: &str) -> Result<bool> {
        for key in values.keys() {
            if !SETTING_KEYS.contains(&key.as_str()) {
                return Err(AtlasError::Invalid(format!("unknown setting key '{key}'")));
            }
        }
        // The board route is the only way in. It checks that no stage being dropped
        // still holds tasks, applies the `renames` map that carries tasks across, and
        // takes the task write gate; this call can do none of that. A list written
        // behind the board's back strands tasks in a stage no surface can show or
        // count, and clearing the list back to the default is `PUT /board/stages` with
        // the default four.
        if values.contains_key(STAGES) {
            return Err(AtlasError::Invalid("set board stages through PUT /api/v1/board/stages".into()));
        }
        for (key, value) in values {
            check_type(key, value)?;
        }
        let clear_key = self.base_url_moves_away_from_the_stored_key(values)?;
        for (key, value) in values {
            if key == API_KEY && value.as_str() == Some(MASKED) {
                continue;
            }
            self.write(key, value)?;
            // The api key value itself never goes into the audit log.
            let detail = if key == API_KEY { serde_json::json!({"key": key}) } else { serde_json::json!({"key": key, "value": value}) };
            MemoryRepo::new(self.db).audit(actor, "set", "setting", None, detail)?;
        }
        if clear_key {
            self.write(API_KEY, &Value::String(String::new()))?;
            MemoryRepo::new(self.db).audit(actor, "clear", "setting", None, serde_json::json!({"key": API_KEY, "reason": "base_url changed"}))?;
            // The key itself is never named in the log, only the fact that it is gone.
            tracing::info!("extraction.base_url changed without a new api key; the stored key was cleared");
        }
        Ok(clear_key)
    }

    /// Stores the global stage list. Crate-private because
    /// [`crate::board::TaskRepo::set_global_stages`] is the only caller that has done
    /// the removal checks, the renames and the gate hold that `set_many` refuses to
    /// write without.
    pub(crate) fn set_board_stages(&self, stages: &Value, actor: &str) -> Result<()> {
        check_type(STAGES, stages)?;
        self.write(STAGES, stages)?;
        MemoryRepo::new(self.db).audit(actor, "set", "setting", None, serde_json::json!({"key": STAGES, "value": stages}))
    }

    /// Whether this call points `extraction.base_url` somewhere new while leaving the
    /// stored api key in place. A masked `"***"` is the GUI saying "leave the key
    /// alone", not a key, so it does not count as supplying one.
    fn base_url_moves_away_from_the_stored_key(&self, values: &Map<String, Value>) -> Result<bool> {
        let Some(incoming) = values.get(BASE_URL).and_then(|v| v.as_str()) else { return Ok(false) };
        let sets_key = values.get(API_KEY).and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty() && s != MASKED);
        if sets_key {
            return Ok(false);
        }
        let stored_key = self.get_raw(API_KEY)?.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        if stored_key.is_empty() {
            return Ok(false);
        }
        let stored_url = self.get_raw(BASE_URL)?.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        Ok(!same_endpoint(incoming, &stored_url))
    }

    fn write(&self, key: &str, value: &Value) -> Result<()> {
        let json = value.to_string();
        self.db.with_conn(|c| {
            c.execute("delete from settings where key = ?", params![key])?;
            c.execute("insert into settings (key, value) values (?, ?::json)", params![key, json])?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accelerator_accepts_modifiers_plus_one_key() {
        assert!(validate_accelerator("CmdOrCtrl+Shift+K").is_ok());
        assert!(validate_accelerator("Alt+Space").is_ok());
        assert!(validate_accelerator(" Ctrl + Shift + P ").is_ok(), "surrounding whitespace is trimmed");
        assert!(validate_accelerator("ctrl+shift+k").is_ok(), "modifiers are case-insensitive");
    }

    #[test]
    fn validate_accelerator_rejects_a_bare_key_or_a_key_less_combo() {
        assert!(validate_accelerator("K").is_err(), "no modifier");
        assert!(validate_accelerator("").is_err());
        assert!(validate_accelerator("Cmd+").is_err(), "trailing separator with no key");
        assert!(validate_accelerator("Cmd+Shift").is_err(), "ends on a modifier, not a key");
        assert!(validate_accelerator("Bogus+K").is_err(), "unknown modifier");
    }

    #[test]
    fn unknown_keys_are_rejected_before_any_write() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let values = Map::from_iter([("bogus".to_string(), Value::from(1))]);
        let err = repo.set_many(&values, "t").unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
        assert_eq!(repo.get_raw("bogus").unwrap(), None);
    }

    /// A value of the wrong JSON type is refused before anything is written, so the
    /// extraction worker never reads a shape it cannot use out of the database.
    #[test]
    fn wrong_value_types_are_rejected_before_any_write() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let bad: &[(&str, Value)] = &[
            (API_KEY, Value::from(1)),
            (API_KEY, serde_json::json!({"k": "v"})),
            ("extraction.base_url", Value::from(true)),
            ("extraction.model", Value::from(7)),
            ("embedding.model", serde_json::json!([])),
            ("extraction.enabled", Value::String("yes".into())),
            ("extraction.auto_accept_min_confidence", Value::String("0.5".into())),
            ("extraction.auto_accept_min_confidence", Value::from(1.5)),
            ("extraction.auto_accept_min_confidence", Value::from(-0.1)),
            ("daemon.port", Value::String("7433".into())),
            ("daemon.port", Value::from(0)),
            ("daemon.port", Value::from(70000)),
            ("ui.theme", Value::String("solarized".into())),
            ("ui.theme", Value::from(1)),
            ("ui.theme_pack", Value::from(1)),
            ("ui.theme_pack", Value::String("not json".into())),
            ("ui.theme_pack", Value::String("[]".into())),
            ("ui.theme_pack", Value::String(r#"{"name":"x","base":"purple","tokens":{}}"#.into())),
            ("ui.theme_pack", Value::String(r#"{"name":"","base":"dark","tokens":{}}"#.into())),
            ("ui.theme_pack", Value::String(r#"{"name":"x","base":"dark"}"#.into())),
            (
                "ui.theme_pack",
                Value::String(format!(r#"{{"name":"{}","base":"dark","tokens":{{}}}}"#, "x".repeat(65))),
            ),
            (
                "ui.theme_pack",
                Value::String(format!(r#"{{"name":"x","base":"dark","tokens":{{}},"padding":"{}"}}"#, "x".repeat(17 * 1024))),
            ),
            (
                "ui.theme_pack",
                Value::String(r##"{"name":"x","base":"dark","tokens":{"--not-a-token":"#000"}}"##.into()),
            ),
            (
                "ui.theme_pack",
                Value::String(r#"{"name":"x","base":"dark","tokens":{"--accent":"not-a-colour"}}"#.into()),
            ),
            (
                "ui.theme_pack",
                Value::String(r#"{"name":"x","base":"dark","tokens":{"--radius-sm":"not-a-length"}}"#.into()),
            ),
            (
                "ui.theme_pack",
                Value::String(r#"{"name":"x","base":"dark","tokens":{"--radius-lg":"3px"}}"#.into()),
            ),
            ("ui.font_ui", Value::String("comic-sans".into())),
            ("ui.font_ui", Value::from(1)),
            ("ui.font_mono", Value::String("comic-sans".into())),
            ("ui.font_size", Value::from(14)),
            ("ui.font_size", Value::String("12".into())),
            ("ui.scale", Value::from(101)),
            ("ui.scale", Value::String("125".into())),
            ("mcp.disabled_tools", Value::String("memory_review".into())),
            ("mcp.disabled_tools", serde_json::json!(["memory_review", "no_such_tool"])),
            ("mcp.disabled_tools", serde_json::json!([1])),
            ("access.memory_writers", Value::from(1)),
            ("access.memory_writers", serde_json::json!([])),
            ("access.memory_writers", serde_json::json!([""])),
            ("access.memory_writers", serde_json::json!([1])),
            ("access.memory_writers", serde_json::json!(["claude-code", "claude-code"])),
            ("access.memory_writers", serde_json::json!(["x".repeat(129)])),
            ("access.task_movers", Value::String("claude-code".into())),
            ("access.task_movers", serde_json::json!([""])),
            ("access.task_movers", serde_json::json!(["codex", "codex"])),
            ("access.require_review", Value::String("true".into())),
            ("access.require_review", Value::from(1)),
            ("ui.autostart", Value::String("yes".into())),
            ("ui.notify.review_pending", Value::from(1)),
            ("ui.notify.workflow_runs", Value::String("true".into())),
            ("ui.notify.daemon_errors", serde_json::json!({})),
            ("ui.global_shortcut", Value::from(1)),
            ("ui.global_shortcut", Value::String("K".into())),
            ("ui.global_shortcut", Value::String("Cmd+".into())),
            ("ui.global_shortcut", Value::String("Cmd+Shift".into())),
            ("ui.global_shortcut", Value::String("Bogus+K".into())),
            ("ui.global_shortcut", Value::String("".into())),
        ];
        for (key, value) in bad {
            let values = Map::from_iter([((*key).to_string(), value.clone())]);
            let err = repo.set_many(&values, "t").unwrap_err();
            assert!(matches!(err, AtlasError::Invalid(_)), "{key} = {value}: {err}");
            assert_eq!(repo.get_raw(key).unwrap(), None, "{key} must not have been written");
        }
    }

    /// One bad value fails the whole call, so a partial write cannot leave the settings
    /// half updated.
    #[test]
    fn a_bad_value_rejects_the_whole_call() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let values = Map::from_iter([
            ("extraction.model".to_string(), Value::String("deepseek-chat".into())),
            ("extraction.enabled".to_string(), Value::from(1)),
        ]);
        assert!(matches!(repo.set_many(&values, "t").unwrap_err(), AtlasError::Invalid(_)));
        assert_eq!(repo.get_raw("extraction.model").unwrap(), None);
    }

    #[test]
    fn well_typed_values_and_nulls_are_accepted() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let values = Map::from_iter([
            ("extraction.enabled".to_string(), Value::from(true)),
            ("extraction.base_url".to_string(), Value::String("https://api.deepseek.com".into())),
            ("extraction.auto_accept_min_confidence".to_string(), Value::from(0.8)),
            ("daemon.port".to_string(), Value::from(7433)),
            ("embedding.model".to_string(), Value::Null),
            ("ui.theme".to_string(), Value::String("light".into())),
            (
                "ui.theme_pack".to_string(),
                Value::String(r##"{"name":"Ocean","base":"dark","tokens":{"--accent":"#4C8DF6","--radius-sm":"4px"}}"##.into()),
            ),
            ("ui.font_ui".to_string(), Value::String("inter".into())),
            ("ui.font_mono".to_string(), Value::String("system-mono".into())),
            ("ui.font_size".to_string(), Value::from(13)),
            ("ui.scale".to_string(), Value::from(125)),
            ("mcp.disabled_tools".to_string(), serde_json::json!(["project_connect", "memory_review"])),
            ("access.memory_writers".to_string(), serde_json::json!(["claude-code"])),
            ("access.task_movers".to_string(), Value::Null),
            ("access.require_review".to_string(), Value::from(true)),
            ("ui.autostart".to_string(), Value::from(true)),
            ("ui.notify.review_pending".to_string(), Value::from(true)),
            ("ui.notify.workflow_runs".to_string(), Value::from(false)),
            ("ui.notify.daemon_errors".to_string(), Value::from(true)),
            ("ui.global_shortcut".to_string(), Value::String("CmdOrCtrl+Shift+K".into())),
        ]);
        repo.set_many(&values, "t").unwrap();
        assert_eq!(repo.get_raw("ui.theme").unwrap(), Some(Value::String("light".into())));
        assert_eq!(
            repo.get_raw("ui.theme_pack").unwrap(),
            Some(Value::String(r##"{"name":"Ocean","base":"dark","tokens":{"--accent":"#4C8DF6","--radius-sm":"4px"}}"##.into()))
        );
        assert_eq!(repo.get_raw("ui.font_ui").unwrap(), Some(Value::String("inter".into())));
        assert_eq!(repo.get_raw("ui.font_mono").unwrap(), Some(Value::String("system-mono".into())));
        assert_eq!(repo.get_raw("ui.font_size").unwrap(), Some(Value::from(13)));
        assert_eq!(repo.get_raw("ui.scale").unwrap(), Some(Value::from(125)));
        assert_eq!(repo.get_raw("ui.global_shortcut").unwrap(), Some(Value::String("CmdOrCtrl+Shift+K".into())));
        assert_eq!(repo.get_raw("mcp.disabled_tools").unwrap(), Some(serde_json::json!(["project_connect", "memory_review"])));
        assert_eq!(repo.get_raw("access.memory_writers").unwrap(), Some(serde_json::json!(["claude-code"])));
        assert_eq!(repo.get_raw("access.task_movers").unwrap(), Some(Value::Null));
        assert_eq!(repo.get_raw("access.require_review").unwrap(), Some(Value::from(true)));
        assert_eq!(repo.get_raw("extraction.enabled").unwrap(), Some(Value::from(true)));
        assert_eq!(repo.get_raw("daemon.port").unwrap(), Some(Value::from(7433)));
        // The bounds are inclusive at both ends.
        for edge in [0.0, 1.0] {
            repo.set_many(&Map::from_iter([("extraction.auto_accept_min_confidence".to_string(), Value::from(edge))]), "t").unwrap();
        }
        // A masked api key is skipped before the type check ever sees it.
        repo.set_many(&Map::from_iter([(API_KEY.to_string(), Value::String(MASKED.into()))]), "t").unwrap();
    }

    #[test]
    fn get_all_has_every_key_and_masks_the_api_key() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let all = repo.get_all().unwrap();
        assert_eq!(all.len(), SETTING_KEYS.len());
        for key in SETTING_KEYS {
            assert_eq!(all.get(*key), Some(&Value::Null), "{key}");
        }
        let values = Map::from_iter([(API_KEY.to_string(), Value::String("sk-real".into()))]);
        repo.set_many(&values, "t").unwrap();
        assert_eq!(repo.get_all().unwrap().get(API_KEY), Some(&Value::String(MASKED.into())));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));
    }

    /// A masked value sent back through `set_many` (the shape a GET->edit->PUT round
    /// trip over HTTP produces) must not overwrite the real key underneath it.
    #[test]
    fn masked_api_key_round_trip_leaves_the_real_key_unchanged() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        repo.set_many(&Map::from_iter([(API_KEY.to_string(), Value::String("sk-real".into()))]), "t").unwrap();
        repo.set_many(
            &Map::from_iter([(API_KEY.to_string(), Value::String(MASKED.into())), ("extraction.model".to_string(), Value::String("x".into()))]),
            "t",
        )
        .unwrap();
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));
        assert_eq!(repo.get_raw("extraction.model").unwrap(), Some(Value::String("x".into())));
    }

    /// The daemon is unauthenticated on loopback, so a local process can repoint
    /// `extraction.base_url` and have the daemon send the stored key wherever it likes.
    /// A key belongs to the endpoint it was entered against: moving the endpoint drops
    /// it, and the caller is told so.
    #[test]
    fn changing_the_base_url_clears_the_stored_api_key() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let set = |values: Vec<(&str, Value)>| {
            repo.set_many(&Map::from_iter(values.into_iter().map(|(k, v)| (k.to_string(), v))), "t").unwrap()
        };

        // Configured in one call: the key is for the endpoint named beside it.
        assert!(!set(vec![(BASE_URL, "https://api.deepseek.com".into()), (API_KEY, "sk-real".into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));

        // Re-sending the same endpoint, with or without its trailing slash, is not a move.
        assert!(!set(vec![(BASE_URL, "https://api.deepseek.com/".into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));
        // Nor is a masked key alongside it: `"***"` means "leave the key alone".
        assert!(!set(vec![(BASE_URL, "https://api.deepseek.com".into()), (API_KEY, MASKED.into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));

        // A new endpoint with no new key: the old key does not follow it there.
        assert!(set(vec![(BASE_URL, "http://attacker.example/v1".into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String(String::new())));
        assert_eq!(repo.get_raw(BASE_URL).unwrap(), Some(Value::String("http://attacker.example/v1".into())), "the endpoint itself is still stored");

        // A new endpoint that brings its own key keeps it.
        assert!(!set(vec![(BASE_URL, "http://localhost:1234/v1".into()), (API_KEY, "sk-local".into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-local".into())));

        // With no key stored there is nothing to clear, so nothing is reported.
        repo.set_many(&Map::from_iter([(API_KEY.to_string(), Value::String(String::new()))]), "t").unwrap();
        assert!(!set(vec![(BASE_URL, "http://elsewhere.example/v1".into())]));
    }

    #[test]
    fn set_many_upserts_and_is_audited_without_the_key_value() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        repo.set_many(&Map::from_iter([("daemon.port".to_string(), Value::from(7433))]), "t").unwrap();
        repo.set_many(&Map::from_iter([("daemon.port".to_string(), Value::from(7000))]), "t").unwrap();
        assert_eq!(repo.get_raw("daemon.port").unwrap(), Some(Value::from(7000)));

        repo.set_many(&Map::from_iter([(API_KEY.to_string(), Value::String("sk-secret".into()))]), "t").unwrap();
        let detail: String = db
            .with_conn(|c| Ok(c.query_row("select detail::text from audit where entity = 'setting' and actor = 't' order by \"at\" desc limit 1", [], |r| r.get(0))?))
            .unwrap();
        assert!(!detail.contains("sk-secret"), "the api key value must not appear in the audit log: {detail}");
    }

    // ---- theme pack token allow list and value syntax ----

    #[test]
    fn theme_token_allowlist_has_colours_and_the_two_radius_tokens_but_not_aliases_or_shadows() {
        let allowed = theme_token_allowlist();
        for name in ["--bg-base", "--bg-surface", "--accent", "--accent-muted", "--text-primary", "--border-strong"] {
            assert!(allowed.contains(name), "{name} should be a colour token");
        }
        assert!(allowed.contains("--radius-sm"));
        assert!(allowed.contains("--radius-md"));
        // Not offered: aliases resolve through `var()` rather than a literal colour,
        // shadows are not colours, and radius-lg is a modal radius, not one of the
        // two the design requirements call out.
        assert!(!allowed.contains("--surface-window"), "aliases are var() references, not literal colours");
        assert!(!allowed.contains("--shadow-sm"), "shadows are not colours");
        assert!(!allowed.contains("--radius-lg"), "only radius-sm and radius-md are offered");
    }

    #[test]
    fn is_css_color_accepts_hex_and_function_forms_and_rejects_everything_else() {
        for good in ["#fff", "#ffff", "#4C8DF6", "#4C8DF6AA", "rgb(1,2,3)", "rgba(1,2,3,0.5)", "hsl(1,2%,3%)", "oklch(0.5 0.1 200)"] {
            assert!(is_css_color(good), "{good} should be a valid colour");
        }
        for bad in ["red", "3px", "#gggggg", "#12345", "var(--bg-base)", ""] {
            assert!(!is_css_color(bad), "{bad} should not be a valid colour");
        }
    }

    #[test]
    fn is_css_length_accepts_plain_lengths_and_rejects_everything_else() {
        // Same literal lists as `isCssLength`'s test in `src/lib/shell/theme-pack.test.ts`.
        for good in ["0", "3px", "0.5rem", "12em", "50%"] {
            assert!(is_css_length(good), "{good} should be a valid length");
        }
        for bad in ["px", "3", "3xy", "-3px-", "", ".px", ".5rem", "3."] {
            assert!(!is_css_length(bad), "{bad} should not be a valid length");
        }
    }

    /// The upper boundary of the actor-list cap, from both sides. The `bad` table above
    /// covers an empty list and duplicates; 64 is the last accepted length and 65 the
    /// first refused one, so an off-by-one in the cap fails here rather than quietly
    /// widening what a global access list may hold.
    #[test]
    fn access_lists_accept_exactly_64_actors_and_refuse_65() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        for key in ["access.memory_writers", "access.task_movers"] {
            let at_cap: Vec<String> = (0..64).map(|i| format!("agent-{i}")).collect();
            let values = Map::from_iter([(key.to_string(), serde_json::json!(at_cap))]);
            repo.set_many(&values, "t").unwrap_or_else(|e| panic!("{key} refused 64 actors: {e}"));

            let over_cap: Vec<String> = (0..65).map(|i| format!("agent-{i}")).collect();
            let values = Map::from_iter([(key.to_string(), serde_json::json!(over_cap))]);
            let err = repo.set_many(&values, "t").unwrap_err();
            assert!(matches!(err, AtlasError::Invalid(_)), "{key} accepted 65 actors: {err}");
        }
    }

    /// `is_plugin_id`'s trailing-dash clause on its own, through the shape check
    /// `mcp.disabled_tools` runs. `plugin__ab-__count` splits into the id `ab-` and the
    /// name `count`: the name is legal and the id holds no `--`, so the trailing dash is
    /// the only clause that can refuse it. No registry ever holds that id, so storing the
    /// name would gate nothing. The neighbouring legal name is accepted in the same test
    /// so a blanket refusal cannot pass it.
    #[test]
    fn mcp_disabled_tools_refuses_a_plugin_name_whose_id_ends_in_a_dash() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);

        let values = Map::from_iter([("mcp.disabled_tools".to_string(), serde_json::json!(["plugin__ab-__count"]))]);
        let err = repo.set_many(&values, "t").unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
        assert!(err.to_string().contains("plugin__ab-__count"), "{err}");

        let values = Map::from_iter([("mcp.disabled_tools".to_string(), serde_json::json!(["plugin__ab__count"]))]);
        repo.set_many(&values, "t").expect("a legal plugin tool name must still be storable");
    }

    #[test]
    fn validate_theme_pack_accepts_a_well_formed_pack() {
        let pack = r##"{"name":"Ocean","base":"light","tokens":{"--accent":"#2563EB","--bg-base":"rgba(0,0,0,0.1)","--radius-md":"6px"}}"##;
        assert!(validate_theme_pack(pack).is_ok());
    }

    #[test]
    fn validate_theme_pack_accepts_a_name_at_exactly_64_characters() {
        let name = "x".repeat(64);
        let pack = format!(r#"{{"name":"{name}","base":"dark","tokens":{{}}}}"#);
        assert!(validate_theme_pack(&pack).is_ok());
    }

    #[test]
    fn validate_theme_pack_rejects_a_name_over_64_characters() {
        let name = "x".repeat(65);
        let pack = format!(r#"{{"name":"{name}","base":"dark","tokens":{{}}}}"#);
        assert!(validate_theme_pack(&pack).is_err());
    }

    #[test]
    fn validate_theme_pack_rejects_json_over_16kb() {
        let padding = "x".repeat(17 * 1024);
        let pack = format!(r#"{{"name":"x","base":"dark","tokens":{{}},"padding":"{padding}"}}"#);
        assert!(validate_theme_pack(&pack).is_err());
    }

    fn decl(plugin_id: &str, name: &str) -> PluginToolDecl {
        PluginToolDecl {
            plugin_id: plugin_id.into(),
            name: name.into(),
            description: "Counts something.".into(),
            args: serde_json::json!({"type": "object", "properties": {}}),
            scope: crate::models::PluginToolScope::Read,
        }
    }

    #[test]
    fn validate_plugin_tool_decls_accepts_a_well_formed_set() {
        let decls = vec![decl("hello-world", "count"), decl("hello-world", "greet_twice")];
        assert!(validate_plugin_tool_decls(&decls).is_ok());
        assert!(validate_plugin_tool_decls(&[decl("a1", "z")]).is_ok(), "the shortest legal id and name");
    }

    #[test]
    fn validate_plugin_tool_decls_rejects_a_malformed_plugin_id() {
        for bad in ["", "a", "-lead", "ab-", "hello-world-", "Upper", "has space", "under_score", &"a".repeat(65)] {
            assert!(validate_plugin_tool_decls(&[decl(bad, "count")]).is_err(), "accepted plugin id '{bad}'");
        }
    }

    #[test]
    fn validate_plugin_tool_decls_rejects_a_malformed_tool_name() {
        for bad in ["", "1count", "Count", "with-dash", "has space", &"a".repeat(49)] {
            assert!(validate_plugin_tool_decls(&[decl("hello-world", bad)]).is_err(), "accepted tool name '{bad}'");
        }
    }

    /// The three id and name shapes that would break the MCP name mapping are refused.
    /// A `--` in an id and a `__` in a name would each map two different (plugin, tool)
    /// pairs onto one MCP name: `a--b` + `count` and `a` + `b__count` both read as
    /// `plugin__a__b__count`. A trailing `-` collides with nothing, but its rewritten
    /// `_` runs into the separator, so `ab-` + `count` encodes to `plugin__ab___count`
    /// and parses back as `("ab", "_count")`, a pair no registry holds.
    #[test]
    fn validate_plugin_tool_decls_rejects_the_names_that_would_collide() {
        let bad_id = validate_plugin_tool_decls(&[decl("a--b", "count")]).unwrap_err();
        assert!(bad_id.to_string().contains("plugin id a--b cannot contain \"--\""), "{bad_id}");
        let bad_name = validate_plugin_tool_decls(&[decl("hello-world", "b__count")]).unwrap_err();
        assert!(bad_name.to_string().contains("plugin tool name b__count cannot contain \"__\""), "{bad_name}");
        let trailing = validate_plugin_tool_decls(&[decl("ab-", "count")]).unwrap_err();
        assert!(trailing.to_string().contains("plugin id ab- cannot end with \"-\""), "{trailing}");

        // A single dash and a single underscore stay legal, and so do the shortest and
        // longest legal forms.
        assert!(validate_plugin_tool_decls(&[decl("a-b-c", "greet_twice")]).is_ok());
        assert!(validate_plugin_id("hello-world").is_ok());
        assert!(validate_plugin_id("a--b").is_err(), "the id-only check refuses it too");
        assert!(validate_plugin_id("ab-").is_err(), "and refuses a trailing dash");
    }

    /// The whole point of the three rules above: no two distinct (plugin id, tool name)
    /// pairs share an MCP name, and every pair registration accepts survives the round
    /// trip. The mapping is spelled out here rather than imported, since `atlas-core`
    /// cannot depend on `atlas-mcp`; `atlas-mcp`'s own
    /// `distinct_plugin_tools_never_share_an_mcp_name` checks the same property against
    /// the real functions.
    ///
    /// The lists carry the boundary shapes on purpose: an id ending in `-`, an id with
    /// `--`, a name with `__`, and names whose own underscores sit next to the
    /// separator. Each is asserted invalid with its reason, so loosening a rule turns one
    /// of them valid and fails the round trip below rather than passing quietly.
    #[test]
    fn every_valid_plugin_id_and_tool_name_maps_to_a_distinct_mcp_name() {
        let ids = ["hello-world", "hello", "a1", "x-y-z", "helloworld", "hello-w", "ab-", "hello--world", "a-b-"];
        let names = ["count", "greet_twice", "c", "count_2", "b_count", "world_count", "count_", "b__count"];
        let illegal_ids = ["ab-", "hello--world", "a-b-"];
        let illegal_names = ["b__count"];
        let mut seen: HashMap<String, (&str, &str)> = HashMap::new();
        for id in ids {
            for name in names {
                let valid = validate_plugin_tool_decls(&[decl(id, name)]).is_ok();
                if !valid {
                    assert!(
                        illegal_ids.contains(&id) || illegal_names.contains(&name),
                        "{id} / {name} was refused but is not one of the shapes the rules exclude",
                    );
                    continue;
                }
                assert!(!illegal_ids.contains(&id), "{id} should have been refused as an id");
                assert!(!illegal_names.contains(&name), "{name} should have been refused as a tool name");
                let mcp_name = format!("plugin__{}__{name}", id.replace('-', "_"));
                // Splitting on the first `__` after the prefix recovers the pair,
                // because neither half can hold one and the id cannot end in one either.
                let rest = mcp_name.strip_prefix("plugin__").unwrap();
                let (got_id, got_name) = rest.split_once("__").unwrap();
                assert_eq!(got_id, id.replace('-', "_"), "{mcp_name}");
                assert_eq!(got_name, name, "{mcp_name}");
                if let Some(other) = seen.insert(mcp_name.clone(), (id, name)) {
                    panic!("{mcp_name} is produced by both {other:?} and ({id}, {name})");
                }
            }
        }
        assert!(!seen.is_empty(), "the loop never reached a valid pair");
    }

    /// `mcp.disabled_tools` may only hold a plugin name registration could actually
    /// produce, so a setting that would silently gate nothing cannot be stored.
    #[test]
    fn a_plugin_name_in_disabled_tools_is_held_to_the_registered_shape() {
        for good in ["plugin__hello_world__count", "plugin__a1__c", "plugin__x_y_z__greet_twice"] {
            assert!(validate_mcp_tool_names(&[good.to_string()]).is_ok(), "refused '{good}'");
        }
        for bad in ["plugin__Foo Bar__x", "plugin__A__B", "plugin__x__y-z", "plugin__a__1count", "plugin__x__", "plugin____x", "plugin__hello_world"] {
            assert!(validate_mcp_tool_names(&[bad.to_string()]).is_err(), "accepted '{bad}'");
        }
        assert!(validate_mcp_tool_names(&["memory_remember".to_string()]).is_ok(), "the built-ins still pass");
    }

    #[test]
    fn validate_plugin_tool_decls_rejects_a_bad_description_or_args() {
        let mut blank = decl("hello-world", "count");
        blank.description = "   ".into();
        assert!(validate_plugin_tool_decls(&[blank]).is_err(), "a blank description");

        let mut long = decl("hello-world", "count");
        long.description = "x".repeat(MAX_PLUGIN_TOOL_DESCRIPTION + 1);
        assert!(validate_plugin_tool_decls(&[long]).is_err(), "a description over the cap");

        for args in [serde_json::json!([]), serde_json::json!("object"), serde_json::json!({}), serde_json::json!({"type": "string"})] {
            let mut d = decl("hello-world", "count");
            d.args = args.clone();
            assert!(validate_plugin_tool_decls(&[d]).is_err(), "accepted args {args}");
        }
    }

    #[test]
    fn validate_plugin_tool_decls_rejects_duplicates_and_more_than_the_cap() {
        let dupes = vec![decl("hello-world", "count"), decl("hello-world", "count")];
        assert!(validate_plugin_tool_decls(&dupes).is_err(), "the same name twice under one plugin");

        let across = vec![decl("hello-world", "count"), decl("other-plugin", "count")];
        assert!(validate_plugin_tool_decls(&across).is_ok(), "the same name under two plugins is fine");

        let at_cap: Vec<PluginToolDecl> = (0..MAX_PLUGIN_TOOLS).map(|i| decl("hello-world", &format!("t{i}"))).collect();
        assert!(validate_plugin_tool_decls(&at_cap).is_ok());
        let over_cap: Vec<PluginToolDecl> = (0..=MAX_PLUGIN_TOOLS).map(|i| decl("hello-world", &format!("t{i}"))).collect();
        assert!(validate_plugin_tool_decls(&over_cap).is_err());
    }
}
