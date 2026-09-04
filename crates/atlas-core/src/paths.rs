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
