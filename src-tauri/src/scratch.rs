//! A temporary directory that removes itself on drop.
//!
//! Hand-rolled rather than pulling in the `tempfile` crate: extracting a downloaded plugin
//! archive is production code, so `tempfile` would have to become a normal dependency of
//! the desktop app for one call site. The tests use the same type, which is why it lives
//! here rather than inside `plugins::install`.

use std::ops::Deref;
use std::path::{Path, PathBuf};

/// A directory under the OS temp dir, removed on drop. The name carries the process id and
/// a nanosecond stamp so two runs never collide.
pub(crate) struct ScratchDir(PathBuf);

impl ScratchDir {
    pub(crate) fn new(label: &str) -> Result<Self, String> {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("atlas-{label}-{}-{unique}", std::process::id()));
        // `create_dir`, not `create_dir_all`: the path is predictable enough for another
        // local process to pre-create, and `create_dir` fails when it already exists rather
        // than unpacking a downloaded archive into someone else's directory.
        std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
        Ok(Self(dir))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

/// So a `ScratchDir` reads as the `&Path` it stands for at a call site that wants one.
impl Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_directory_and_removes_it_on_drop() {
        let path = {
            let dir = ScratchDir::new("scratch-test").unwrap();
            assert!(dir.path().is_dir());
            dir.path().to_path_buf()
        };
        assert!(!path.exists(), "the directory outlived the guard");
    }

    #[test]
    fn refuses_a_path_that_already_exists() {
        let dir = ScratchDir::new("scratch-exists").unwrap();
        let taken = dir.path().to_path_buf();
        assert!(std::fs::create_dir(&taken).is_err(), "create_dir must not overwrite an existing directory");
    }
}
