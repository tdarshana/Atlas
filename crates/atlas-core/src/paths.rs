use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AtlasPaths { pub home: PathBuf }

impl AtlasPaths {
    /// `ATLAS_HOME` if set, else `~/.atlas`.
    pub fn discover() -> Self {
        if let Ok(h) = std::env::var("ATLAS_HOME") { return Self { home: PathBuf::from(h) }; }
        let base = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
        Self { home: base.join(".atlas") }
    }
    pub fn at(home: impl AsRef<Path>) -> Self { Self { home: home.as_ref().to_path_buf() } }
    pub fn db_path(&self) -> PathBuf { self.home.join("atlas.duckdb") }
    pub fn models_dir(&self) -> PathBuf { self.home.join("models") }
    pub fn daemon_file(&self) -> PathBuf { self.home.join("daemon.json") }
    pub fn log_file(&self) -> PathBuf { self.home.join("atlasd.log") }
    pub fn ensure(&self) -> std::io::Result<()> { std::fs::create_dir_all(&self.home)?; std::fs::create_dir_all(self.models_dir()) }
}
