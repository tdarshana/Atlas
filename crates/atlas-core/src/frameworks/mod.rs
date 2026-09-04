//! Detects and reads the planning frameworks (Superpowers, OpenSpec, SpecKit,
//! GSD) a connected project may use, one adapter per framework behind the
//! [`adapter::FrameworkAdapter`] trait.

pub mod adapter;
mod gsd;
pub mod import;
mod md;
mod openspec;
mod speckit;
mod superpowers;

pub use adapter::FrameworkAdapter;

use std::path::Path;

use crate::models::FrameworkInventory;

/// One adapter per known framework, in a stable order (`detect_all`'s output
/// order follows it).
pub fn adapters() -> Vec<Box<dyn FrameworkAdapter>> {
    vec![
        Box::new(superpowers::SuperpowersAdapter),
        Box::new(openspec::OpenspecAdapter),
        Box::new(speckit::SpeckitAdapter),
        Box::new(gsd::GsdAdapter),
    ]
}

/// Runs every adapter's cheap `detect` against `root`, keeping only the
/// frameworks actually found. Used to populate `ProjectProfile.planning_frameworks`
/// on connect and refresh.
pub fn detect_all(root: &Path) -> Vec<FrameworkInventory> {
    adapters().iter().filter_map(|a| a.detect(root)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::FrameworkKind;
    use std::path::PathBuf;

    fn fixtures_dir(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frameworks").join(name)
    }

    #[test]
    fn detects_all_four_fixtures() {
        let kinds: Vec<FrameworkKind> = ["superpowers", "openspec", "speckit", "gsd"]
            .iter()
            .map(|name| detect_all(&fixtures_dir(name)))
            .map(|found| {
                assert_eq!(found.len(), 1, "expected exactly one framework detected in the {found:?} fixture");
                found[0].kind
            })
            .collect();
        assert_eq!(kinds, vec![FrameworkKind::Superpowers, FrameworkKind::Openspec, FrameworkKind::Speckit, FrameworkKind::Gsd]);
    }

    #[test]
    fn detect_is_idempotent() {
        let root = fixtures_dir("superpowers");
        let first = detect_all(&root);
        let second = detect_all(&root);
        assert_eq!(first.len(), second.len());
        assert_eq!(first[0].roots, second[0].roots);
        assert_eq!(first[0].docs, second[0].docs);
        assert_eq!(first[0].tasks, second[0].tasks);
    }

    #[test]
    fn unrelated_directory_detects_nothing() {
        let d = tempfile::tempdir().unwrap();
        assert!(detect_all(d.path()).is_empty());
    }

    /// `detect` must complete without ever opening a document, even one that is
    /// large and (on Unix, where permissions are meaningful) unreadable — proven
    /// by a call counter in `adapter::read_doc_file`, the one function every
    /// adapter's `documents`/`tasks`/`decisions` reads a file through.
    #[test]
    fn detect_never_opens_a_document() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("docs/superpowers/plans")).unwrap();
        let huge = d.path().join("docs/superpowers/plans/huge.md");
        // 8 MiB of content that isn't a real plan: large enough that a full read
        // and parse would be noticeable if `detect` ever did it.
        std::fs::write(&huge, vec![b'x'; 8 * 1024 * 1024]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // No read permission: if anything in `detect`'s path tried to open
            // this file, it would fail loudly rather than the failure being
            // swallowed by a lenient `Ok(_) = ... else` further down the line.
            std::fs::set_permissions(&huge, std::fs::Permissions::from_mode(0o000)).unwrap();
        }

        adapter::reset_open_count();
        let inv = detect_all(d.path());
        assert_eq!(adapter::open_count(), 0, "detect must not open any document");
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].tasks, 1, "the huge file still counts as one task-bearing file by listing alone");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Restore permissions so the tempdir can be cleaned up.
            std::fs::set_permissions(&huge, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        // Sanity check the counter itself: reading the same tree's documents
        // does open files, so a regression that made `detect` cheap by accident
        // (e.g. an adapter that never reads anything) wouldn't pass silently.
        adapter::reset_open_count();
        let _ = adapters()[0].documents(d.path());
        assert!(adapter::open_count() > 0, "documents() is expected to open files");
    }
}
