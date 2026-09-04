// `atlas-plugin.json`: the manifest every installed plugin carries, and the checks that
// decide whether it is well formed and safe to load.

use std::path::{Component as PathComponent, Path};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

/// The API version this app implements. A plugin's `api` field is a semver range
/// matched against this, the same way `cargo` matches a dependency requirement.
pub const ATLAS_API_VERSION: &str = "1.0.0";

/// Whether `req` matches [`ATLAS_API_VERSION`].
pub fn compatible(req: &VersionReq) -> bool {
    Version::parse(ATLAS_API_VERSION).is_ok_and(|v| req.matches(&v))
}

/// Parses an `api` range such as `">=1.0 <2"`. `semver::VersionReq::parse` itself only
/// accepts comparators separated by a comma (`">=1.0, <2"`); the plan's own manifest
/// example uses a bare space, so a comma is inserted between comparators before handing
/// the string to the real parser.
fn parse_api_range(raw: &str) -> Result<VersionReq, String> {
    VersionReq::parse(&comma_separate_comparators(raw)).map_err(|e| e.to_string())
}

/// Turns whitespace that separates two comparators (a run of whitespace immediately
/// followed by a comparator operator) into `", "`. Whitespace that is not followed by an
/// operator, and any whitespace that already follows a comma, is left as a single space.
fn comma_separate_comparators(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 4);
    let mut chars = raw.trim().chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
            let starts_new_comparator = chars.peek().is_some_and(|c| matches!(c, '>' | '<' | '=' | '^' | '~' | '*'));
            if starts_new_comparator && !out.ends_with(',') {
                out.push(',');
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

fn deserialize_api<'de, D>(deserializer: D) -> Result<VersionReq, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    parse_api_range(&raw).map_err(serde::de::Error::custom)
}

/// One capability a plugin asks for. The wire names use dots, matching the plan's
/// examples (`memories.read`, `ui.sections`, ...), so they are spelled out rather than
/// derived from a `rename_all` rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    #[serde(rename = "memories.read")]
    MemoriesRead,
    #[serde(rename = "memories.write")]
    MemoriesWrite,
    #[serde(rename = "tasks.read")]
    TasksRead,
    #[serde(rename = "tasks.write")]
    TasksWrite,
    #[serde(rename = "settings.read")]
    SettingsRead,
    #[serde(rename = "ui.sections")]
    UiSections,
    #[serde(rename = "ui.components")]
    UiComponents,
    #[serde(rename = "mcp.tools")]
    McpTools,
}

/// Where a contributed component renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentSlot {
    #[serde(rename = "dashboard.card")]
    DashboardCard,
    #[serde(rename = "board.card.badge")]
    BoardCardBadge,
    #[serde(rename = "task.detail.panel")]
    TaskDetailPanel,
    #[serde(rename = "table.column")]
    TableColumn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub view: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub slot: ComponentSlot,
    pub id: String,
    pub view: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandContribution {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolContribution {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub args: serde_json::Value,
    pub scope: String,
}

/// Everything a plugin contributes to the app. Every field defaults to empty, so a
/// manifest that contributes nothing at all can omit `contributes` entirely.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contributes {
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub themes: Vec<Theme>,
    #[serde(default)]
    pub components: Vec<Component>,
    #[serde(default)]
    pub commands: Vec<CommandContribution>,
    #[serde(default)]
    pub tools: Vec<ToolContribution>,
}

/// The parsed contents of `atlas-plugin.json`. `version` and `api` are typed as semver
/// values directly, so a malformed one is rejected at deserialization; everything else
/// [`Manifest::validate`] checks needs the plugin's folder on disk, so it runs
/// afterwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: Version,
    pub description: String,
    pub author: String,
    #[serde(deserialize_with = "deserialize_api")]
    pub api: VersionReq,
    pub main: String,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub contributes: Contributes,
}

/// `^[a-z0-9][a-z0-9-]{1,63}$`, spelled out rather than pulled in through a regex
/// dependency this task has no other use for.
fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if !(2..=64).contains(&bytes.len()) {
        return false;
    }
    let first_ok = matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9');
    first_ok && bytes[1..].iter().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

impl Manifest {
    /// Parses `atlas-plugin.json`'s text. Everything that can be checked without the
    /// plugin's folder (field shapes, `version`, `api`, unknown fields) is caught here,
    /// as a plain serde error; call [`Manifest::validate`] afterwards for the rest.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    /// Checks everything [`Manifest::parse`] could not: `id`'s shape, that `main` is a
    /// relative path with no `..` segment that exists inside `plugin_dir`, and that
    /// every contribution has the permission it needs.
    pub fn validate(&self, plugin_dir: &Path) -> Result<(), String> {
        if !valid_id(&self.id) {
            return Err(format!("'{}' is not a valid plugin id.", self.id));
        }

        let main_path = Path::new(&self.main);
        let escapes = main_path.is_absolute()
            || main_path.components().any(|c| matches!(c, PathComponent::ParentDir));
        if escapes {
            return Err(format!("'{}' is not a relative path inside the plugin.", self.main));
        }
        if !plugin_dir.join(&self.main).is_file() {
            return Err(format!("'{}' does not exist in the plugin folder.", self.main));
        }

        let has = |p: Permission| self.permissions.contains(&p);
        if !self.contributes.sections.is_empty() && !has(Permission::UiSections) {
            return Err("`contributes.sections` needs the `ui.sections` permission.".to_string());
        }
        if !self.contributes.components.is_empty() && !has(Permission::UiComponents) {
            return Err("`contributes.components` needs the `ui.components` permission.".to_string());
        }
        if !self.contributes.tools.is_empty() && !has(Permission::McpTools) {
            return Err("`contributes.tools` needs the `mcp.tools` permission.".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO_WORLD: &str = include_str!("../../tests/fixtures/hello-plugin/atlas-plugin.json");

    fn fixture_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello-plugin")
    }

    #[test]
    fn accepts_the_hello_world_shape() {
        let manifest = Manifest::parse(HELLO_WORLD).unwrap();
        assert_eq!(manifest.id, "hello-world");
        manifest.validate(&fixture_dir()).unwrap();
    }

    #[test]
    fn rejects_a_bad_id() {
        let json = HELLO_WORLD.replace("\"hello-world\"", "\"Hello_World\"");
        let manifest = Manifest::parse(&json).unwrap();
        let err = manifest.validate(&fixture_dir()).unwrap_err();
        assert_eq!(err, "'Hello_World' is not a valid plugin id.");
    }

    #[test]
    fn rejects_a_bad_version() {
        let json = HELLO_WORLD.replace("\"1.0.0\"", "\"not-a-version\"");
        let err = Manifest::parse(&json).unwrap_err();
        assert!(err.contains("version"), "expected a version error, got: {err}");
    }

    #[test]
    fn rejects_a_bad_api() {
        let json = HELLO_WORLD.replace("\">=1.0 <2\"", "\"not a version range\"");
        let err = Manifest::parse(&json).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn rejects_a_main_with_dot_dot() {
        let json = HELLO_WORLD.replace("\"main.js\"", "\"../main.js\"");
        let manifest = Manifest::parse(&json).unwrap();
        let err = manifest.validate(&fixture_dir()).unwrap_err();
        assert_eq!(err, "'../main.js' is not a relative path inside the plugin.");
    }

    #[test]
    fn rejects_an_unknown_top_level_field() {
        let json = HELLO_WORLD.replace("\"main\": \"main.js\",", "\"main\": \"main.js\", \"extra\": true,");
        let err = Manifest::parse(&json).unwrap_err();
        assert!(err.contains("extra"), "expected an unknown-field error, got: {err}");
    }

    #[test]
    fn rejects_a_contribution_without_its_permission() {
        let json = HELLO_WORLD.replace(
            "\"permissions\": [\"ui.sections\", \"ui.components\", \"tasks.read\"]",
            "\"permissions\": []",
        );
        assert_ne!(json, HELLO_WORLD, "the fixture's permissions line moved; update this replacement");
        let manifest = Manifest::parse(&json).unwrap();
        let err = manifest.validate(&fixture_dir()).unwrap_err();
        assert_eq!(err, "`contributes.sections` needs the `ui.sections` permission.");
    }

    #[test]
    fn compatible_accepts_a_matching_range() {
        assert!(compatible(&parse_api_range(">=1.0 <2").unwrap()));
    }

    #[test]
    fn compatible_rejects_a_range_that_excludes_the_current_version() {
        assert!(!compatible(&parse_api_range(">=2").unwrap()));
    }
}
