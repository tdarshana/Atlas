use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::models::{FrameworkDoc, FrameworkInventory, FrameworkKind, ImportedDecision, ImportedTask};
use crate::{AtlasError, Result};

/// Reads and detects one planning framework's files inside a connected project.
/// Every method takes `root`, the project's canonicalised root: an adapter never
/// reads outside it, and `read` refuses a path that resolves outside it.
pub trait FrameworkAdapter: Send + Sync {
    fn kind(&self) -> FrameworkKind;
    /// Cheap existence checks and shallow directory listings (no recursion beyond
    /// two levels) for this framework's roots: `docs`/`tasks` on the returned
    /// inventory are file counts by name and extension, not document content.
    /// `detect` never opens a document — `documents`, `tasks` and `decisions`
    /// are the readers. `None` when nothing was found.
    fn detect(&self, root: &Path) -> Option<FrameworkInventory>;
    /// Every document this framework's roots hold, one directory listing per root.
    fn documents(&self, root: &Path) -> Vec<FrameworkDoc>;
    /// The text of one document, addressed by the path `documents` handed back.
    /// Refuses a path that canonicalises outside `root`.
    fn read(&self, root: &Path, path: &str) -> Result<String>;
    fn tasks(&self, root: &Path) -> Vec<ImportedTask>;
    fn decisions(&self, root: &Path) -> Vec<ImportedDecision>;
    /// Where this framework's agent instruction files live, for `atlas sync` to
    /// write its managed block into. Only files that already exist are returned:
    /// Atlas never creates a framework file, only edits inside one that is there.
    fn instruction_targets(&self, root: &Path) -> Vec<PathBuf>;
}

/// Reads `rel` (a path relative to `root`, as recorded on a `FrameworkDoc`) after
/// canonicalising both sides and checking the result did not resolve outside
/// `root`. Shared by every adapter's `read` so the escape check can't be skipped
/// in one of them; also catches a symlink inside `root` that resolves outside it,
/// since canonicalising follows symlinks.
pub(crate) fn read_within_root(root: &Path, rel: &str) -> Result<String> {
    let root = root
        .canonicalize()
        .map_err(|e| AtlasError::Invalid(format!("{}: {e}", root.display())))?;
    let candidate = root.join(rel);
    // A plain sentence either way: whether `rel` simply doesn't exist or resolves
    // outside `root`, the caller gets a message it can show as-is, never the raw
    // OS error text (`No such file or directory (os error 2)`, and so on).
    let canon = candidate
        .canonicalize()
        .map_err(|_| AtlasError::Invalid(format!("{rel} does not exist under the project root")))?;
    if !canon.starts_with(&root) {
        return Err(AtlasError::Invalid(format!("{rel} escapes the project root")));
    }
    std::fs::read_to_string(&canon).map_err(AtlasError::from)
}

/// Reads a document's content for `documents`/`tasks`/`decisions`. The only place
/// any adapter opens a file for its content: `detect` counts by name and
/// extension only and never calls this. In tests, every call is tallied so a
/// test can assert `detect` made none.
pub(crate) fn read_doc_file(path: &Path) -> Option<String> {
    #[cfg(test)]
    OPEN_COUNT.with(|c| c.set(c.get() + 1));
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
thread_local! {
    /// Number of `read_doc_file` calls since the last `reset_open_count`. Lets a
    /// test prove `detect` never opened a document, without depending on OS
    /// permission quirks (which behave differently when tests run as root).
    pub(crate) static OPEN_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_open_count() {
    OPEN_COUNT.with(|c| c.set(0));
}

#[cfg(test)]
pub(crate) fn open_count() -> usize {
    OPEN_COUNT.with(|c| c.get())
}

/// `path` relative to `root`, as a forward-slash-free-of-surprises string (native
/// separator, matching `ProjectProfile.tree`'s convention).
pub(crate) fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().to_string()
}

/// A file's last-modified time, or now if the filesystem can't say.
pub(crate) fn mtime(path: &Path) -> DateTime<Utc> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now())
}

/// The instruction files a framework may target: `CLAUDE.md` and `AGENTS.md` at
/// the project root, whichever exist. Shared by every adapter except GSD, whose
/// `.planning/` convention has no such file.
pub(crate) fn root_instruction_files(root: &Path) -> Vec<PathBuf> {
    ["CLAUDE.md", "AGENTS.md"].iter().map(|f| root.join(f)).filter(|p| p.is_file()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_file_inside_the_root() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("doc.md"), "hello").unwrap();
        assert_eq!(read_within_root(d.path(), "doc.md").unwrap(), "hello");
    }

    #[test]
    fn refuses_a_relative_escape() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("project")).unwrap();
        std::fs::write(d.path().join("secret.txt"), "nope").unwrap();
        let err = read_within_root(&d.path().join("project"), "../secret.txt").unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
    }

    #[test]
    fn refuses_an_absolute_escape() {
        let d = tempfile::tempdir().unwrap();
        let err = read_within_root(d.path(), "/etc/passwd").unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
    }

    #[test]
    fn a_missing_path_refuses_with_a_plain_sentence_too() {
        let d = tempfile::tempdir().unwrap();
        let err = read_within_root(d.path(), "does/not/exist.md").unwrap_err().to_string();
        assert!(!err.contains("os error"), "raw OS error text leaked into the message: {err}");
        assert_eq!(err, "invalid input: does/not/exist.md does not exist under the project root");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlink_pointing_outside_the_root() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "nope").unwrap();
        std::fs::create_dir(d.path().join("project")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), d.path().join("project/link.txt")).unwrap();
        let err = read_within_root(&d.path().join("project"), "link.txt").unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
    }
}
