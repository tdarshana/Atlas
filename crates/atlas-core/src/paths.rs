use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AtlasPaths {
    pub home: PathBuf,
    /// The home directory skill discovery reads its user and plugin roots from
    /// (`<skills_home>/.claude/skills`, `.codex/skills`, `.claude/plugins/cache`).
    ///
    /// Carried here rather than read from the environment at each call so nothing but
    /// [`AtlasPaths::discover`] ever consults `ATLAS_SYNC_HOME` or the real home: a test
    /// building paths with [`AtlasPaths::at`] gets a home of its own by construction and
    /// can never reach the user's `~/.claude`.
    pub skills_home: PathBuf,
}

impl AtlasPaths {
    /// `ATLAS_HOME` if set, else `~/.atlas`, with skills read from `ATLAS_SYNC_HOME` if
    /// set, else the user's own home. The only constructor that reads the environment.
    pub fn discover() -> Self {
        let base = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
        let skills_home = std::env::var_os("ATLAS_SYNC_HOME").map(PathBuf::from).unwrap_or_else(|| base.clone());
        let home = match std::env::var("ATLAS_HOME") {
            Ok(h) => PathBuf::from(h),
            Err(_) => base.join(".atlas"),
        };
        Self { home, skills_home }
    }
    /// [`AtlasPaths::discover`]'s skills home with an explicit data home, for
    /// `atlasd --home` and `atlas --home`: the flag moves where Atlas keeps its own
    /// data, not which skill folders the user has. Every daemon the CLI or the desktop
    /// starts is launched with `--home`, so a daemon must not lose its skills to it.
    pub fn discover_with_home(home: impl AsRef<Path>) -> Self {
        Self { home: home.as_ref().to_path_buf(), skills_home: Self::discover().skills_home }
    }
    /// Paths rooted at `home`, with skill discovery pointed at `home` too. A test's
    /// temp directory holds no `.claude` or `.codex`, so discovery finds nothing there
    /// unless the test puts it there itself; see [`AtlasPaths::with_skills_home`]. This
    /// is the constructor tests use, and it is deliberately the one that cannot reach
    /// the user's real home.
    pub fn at(home: impl AsRef<Path>) -> Self {
        let home = home.as_ref().to_path_buf();
        Self { skills_home: home.clone(), home }
    }
    /// Points skill discovery at a different home, for a test that seeds one.
    pub fn with_skills_home(mut self, skills_home: impl AsRef<Path>) -> Self {
        self.skills_home = skills_home.as_ref().to_path_buf();
        self
    }
    /// The same home under a name that fits every reader of it, not only skills: MCP
    /// server discovery reads `.claude.json`, `.codex/config.toml`, `.cursor/mcp.json`
    /// and the plugin cache from here too. One field, two names, so neither call site
    /// has to read a misleading one.
    pub fn agent_home(&self) -> &Path { &self.skills_home }
    pub fn db_path(&self) -> PathBuf { self.home.join("atlas.duckdb") }
    pub fn models_dir(&self) -> PathBuf { self.home.join("models") }
    pub fn daemon_file(&self) -> PathBuf { self.home.join("daemon.json") }
    pub fn log_file(&self) -> PathBuf { self.home.join("atlasd.log") }
    pub fn ensure(&self) -> std::io::Result<()> { std::fs::create_dir_all(&self.home)?; std::fs::create_dir_all(self.models_dir()) }
}

/// One Claude Code plugin installed under
/// `<home>/.claude/plugins/cache/<marketplace>/<plugin>/<version>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub marketplace: String,
    pub name: String,
    /// The version directory with the newest modification time.
    pub version_dir: PathBuf,
}

impl InstalledPlugin {
    /// `<marketplace>/<plugin>`, the label a plugin's skills and MCP servers carry.
    pub fn label(&self) -> String { format!("{}/{}", self.marketplace, self.name) }
}

/// Every plugin installed under `home`'s Claude Code plugin cache, in marketplace then
/// plugin name order, each at the version directory with the newest modification time.
/// Directories whose name ends in `.clone` or starts with `temp_` are a half-finished
/// install and are skipped; a plugin with no other version is not listed. An absent
/// cache is silent, since most machines have none; anything else that cannot be read
/// is a warning. Skill discovery and MCP server discovery both walk the cache through
/// here, so the version-picking rule lives in one place.
pub fn installed_plugins(home: &Path, warnings: &mut Vec<String>) -> Vec<InstalledPlugin> {
    let cache = home.join(".claude/plugins/cache");
    let mut out = Vec::new();
    let Some(marketplaces) = read_dirs(&cache, warnings) else { return out };
    for marketplace in marketplaces {
        let Some(plugins) = read_dirs(&marketplace, warnings) else { continue };
        for plugin in plugins {
            let Some(versions) = read_dirs(&plugin, warnings) else { continue };
            let newest = versions
                .into_iter()
                .filter(|v| !is_scratch_dir(v))
                .filter_map(|v| std::fs::metadata(&v).ok().and_then(|m| m.modified().ok()).map(|t| (t, v)))
                .max_by_key(|(t, _)| *t)
                .map(|(_, v)| v);
            let Some(version_dir) = newest else { continue };
            out.push(InstalledPlugin { marketplace: name_of(&marketplace), name: name_of(&plugin), version_dir });
        }
    }
    out
}

/// The subdirectories of `path`, sorted, or `None` when it holds none to offer. A path
/// that is simply not there is silent; anything else, a directory the daemon may not
/// traverse in particular, is a warning, so an unreadable root does not look identical
/// to an absent one.
fn read_dirs(path: &Path, warnings: &mut Vec<String>) -> Option<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warnings.push(format!("{}: {e}", path.display()));
            return None;
        }
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) if e.path().is_dir() => out.push(e.path()),
            Ok(_) => {}
            Err(e) => warnings.push(format!("{}: {e}", path.display())),
        }
    }
    out.sort();
    Some(out)
}

fn is_scratch_dir(path: &Path) -> bool {
    let name = name_of(path);
    name.ends_with(".clone") || name.starts_with("temp_")
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_dir_mtime(dir: &Path, when: std::time::SystemTime) {
        std::fs::File::open(dir).unwrap().set_modified(when).unwrap();
    }

    /// The one walk both skill and MCP server discovery use: sorted by marketplace and
    /// plugin, the newest version directory wins, scratch directories are not versions,
    /// and a plugin with only scratch directories is not installed.
    #[test]
    fn installed_plugins_picks_the_newest_real_version_of_each_plugin() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join(".claude/plugins/cache");
        let tools = cache.join("acme/tools");
        for v in ["aaaa1111", "bbbb2222", "cccc3333.clone", "temp_dddd"] {
            std::fs::create_dir_all(tools.join(v)).unwrap();
        }
        let now = std::time::SystemTime::now();
        set_dir_mtime(&tools.join("aaaa1111"), now - std::time::Duration::from_secs(600));
        set_dir_mtime(&tools.join("bbbb2222"), now);
        // A newer scratch directory must not beat the real version.
        set_dir_mtime(&tools.join("cccc3333.clone"), now + std::time::Duration::from_secs(600));
        std::fs::create_dir_all(cache.join("acme/aardvark/v1")).unwrap();
        std::fs::create_dir_all(cache.join("zeta/only-scratch/temp_x")).unwrap();
        // A stray file at the plugin level is not a plugin.
        std::fs::write(cache.join("acme/README"), "not a plugin").unwrap();

        let mut warnings = Vec::new();
        let found = installed_plugins(home.path(), &mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
        let labels: Vec<String> = found.iter().map(InstalledPlugin::label).collect();
        assert_eq!(labels, vec!["acme/aardvark", "acme/tools"], "{labels:?}");
        assert_eq!(found[1].marketplace, "acme");
        assert_eq!(found[1].name, "tools");
        assert_eq!(found[1].version_dir, tools.join("bbbb2222"));
    }

    /// Most machines have no plugin cache, and that is not worth a warning.
    #[test]
    fn a_missing_cache_is_empty_and_silent() {
        let home = tempfile::tempdir().unwrap();
        let mut warnings = Vec::new();
        assert!(installed_plugins(home.path(), &mut warnings).is_empty());
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}
