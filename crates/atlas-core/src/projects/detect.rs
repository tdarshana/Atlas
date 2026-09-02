use std::path::{Path, PathBuf};
use crate::{AtlasError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub root: PathBuf,
    pub remote: Option<String>,
}

/// Finds the git toplevel for `dir` (via `git2`) and its `origin` remote URL
/// (or the first remote if there is no `origin`). Falls back to `dir`
/// canonicalized with `remote: None` when `dir` is not inside a git repo.
pub fn detect_root(dir: &Path) -> Result<Detected> {
    detect_root_with_ceiling(dir, None)
}

/// [`detect_root`] with a ceiling on the upward search: the walk stops at
/// `ceiling` instead of climbing to the filesystem root. Tests need it because a
/// temporary directory may itself sit inside a git repository, which would make
/// the fallback path untestable.
pub(crate) fn detect_root_with_ceiling(dir: &Path, ceiling: Option<&Path>) -> Result<Detected> {
    let dir = dir
        .canonicalize()
        .map_err(|e| AtlasError::Invalid(format!("{}: {e}", dir.display())))?;
    // A file resolves and would otherwise be recorded as a project root, whose profile
    // and sync paths would then be built underneath something that cannot hold them.
    if !dir.is_dir() {
        return Err(AtlasError::Invalid(format!("{} is not a directory", dir.display())));
    }
    // CROSS_FS is what `Repository::discover` passes, so an uncapped call keeps
    // searching across mount points exactly as before.
    let found = git2::Repository::open_ext(&dir, git2::RepositoryOpenFlags::CROSS_FS, ceiling).ok().map(|repo| {
        let root = repo.workdir().map(|p| p.to_path_buf()).unwrap_or_else(|| dir.clone());
        let root = root.canonicalize().unwrap_or(root);
        let remote = repo.remotes().ok().and_then(|names| {
            // `StringArray::iter()` yields `Result<Option<&str>, Error>` (non-utf8
            // names surface as `Ok(None)`); the double `.flatten()` drops both
            // lookup errors and non-utf8 entries.
            let names: Vec<&str> = names.iter().flatten().flatten().collect();
            let first = names.iter().find(|n| **n == "origin").or_else(|| names.first()).copied()?;
            repo.find_remote(first).ok().and_then(|r| r.url().ok().map(String::from))
        });
        Detected { root, remote }
    });
    // libgit2 derives its own stop point from the *parent* of the starting directory,
    // so a ceiling equal to that directory does not stop the walk. Enforce the ceiling
    // on the result too: a root found outside it is not this directory's repository.
    let found = found.filter(|d| ceiling.is_none_or(|c| d.root.starts_with(c)));
    Ok(found.unwrap_or(Detected { root: dir, remote: None }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_common as common;

    #[test]
    fn finds_root_and_remote_from_subdir() {
        let d = tempfile::tempdir().unwrap();
        common::fixture_repo(d.path());
        let det = detect_root(&d.path().join("src")).unwrap();
        assert_eq!(det.root.canonicalize().unwrap(), d.path().canonicalize().unwrap());
        assert_eq!(det.remote.as_deref(), Some("https://github.com/example/fixture.git"));
    }

    #[test]
    fn file_is_rejected() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("not-a-dir.txt");
        std::fs::write(&file, "x").unwrap();
        let err = detect_root(&file).unwrap_err();
        assert!(matches!(err, crate::AtlasError::Invalid(ref m) if m.ends_with("is not a directory")), "{err}");
    }

    #[test]
    fn non_repo_falls_back_to_dir() {
        let d = tempfile::tempdir().unwrap();
        // The ceiling is the tempdir itself: without it the search climbs out of TMPDIR
        // and finds whatever repository happens to contain it, this one included.
        let det = detect_root_with_ceiling(d.path(), Some(&d.path().canonicalize().unwrap())).unwrap();
        assert_eq!(det.root, d.path().canonicalize().unwrap());
        assert!(det.remote.is_none());
    }
}
