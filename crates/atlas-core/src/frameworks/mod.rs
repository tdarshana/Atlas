//! Detects and reads the planning frameworks (Superpowers, OpenSpec, SpecKit,
//! GSD) a connected project may use, one adapter per framework behind the
//! [`adapter::FrameworkAdapter`] trait.

pub mod adapter;
mod gsd;
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
}
