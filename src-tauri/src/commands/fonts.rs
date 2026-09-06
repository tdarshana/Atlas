//! The font families installed on this machine, for the Appearance card's Interface
//! font and Monospace font pickers. The webview cannot enumerate fonts itself (WebKit
//! has no Local Font Access API), so the host reads the system font directories with
//! `fontdb` and answers with one row per family, monospace families flagged.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FontFamily {
    pub family: String,
    /// True when every face of the family is monospaced.
    pub monospace: bool,
}

/// Families from a loaded database, sorted by name (case-insensitively), one row per
/// family name, `monospace` only when every face agrees.
pub(crate) fn families(db: &fontdb::Database) -> Vec<FontFamily> {
    let mut seen: BTreeMap<String, bool> = BTreeMap::new();
    for face in db.faces() {
        let Some((name, _)) = face.families.first() else { continue };
        let name = name.trim();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        let mono = seen.entry(name.to_string()).or_insert(true);
        *mono = *mono && face.monospaced;
    }
    let mut out: Vec<FontFamily> = seen
        .into_iter()
        .map(|(family, monospace)| FontFamily { family, monospace })
        .collect();
    out.sort_by_key(|f| f.family.to_lowercase());
    out
}

/// Every installed font family. Reading and parsing the system fonts takes a moment, so
/// it runs off the async runtime's blocking pool.
#[tauri::command]
pub async fn list_fonts() -> Vec<FontFamily> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        families(&db)
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_installed_families_once_each_and_sorted() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let list = families(&db);
        assert!(!list.is_empty(), "a machine with no fonts is not a real machine");
        let names: Vec<&str> = list.iter().map(|f| f.family.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(names, sorted);
        let mut dedup = names.clone();
        dedup.dedup();
        assert_eq!(names.len(), dedup.len());
        assert!(names.iter().all(|n| !n.starts_with('.')), "hidden system faces are left out");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn flags_a_monospace_family_on_macos() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let list = families(&db);
        let menlo = list.iter().find(|f| f.family == "Menlo").expect("Menlo ships with macOS");
        assert!(menlo.monospace);
    }
}
