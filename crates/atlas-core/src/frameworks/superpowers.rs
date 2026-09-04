use std::path::{Path, PathBuf};

use chrono::Utc;

use super::adapter::{mtime, read_within_root, rel, root_instruction_files, FrameworkAdapter};
use super::md::{checkboxes, first_heading, ruling_lines};
use crate::models::{FrameworkDoc, FrameworkDocType, FrameworkInventory, FrameworkKind, ImportedDecision, ImportedTask, SourceRef};
use crate::Result;

const SPECS_DIR: &str = "docs/superpowers/specs";
const PLANS_DIR: &str = "docs/superpowers/plans";
const SDD_DIR: &str = ".superpowers/sdd";

pub struct SuperpowersAdapter;

impl FrameworkAdapter for SuperpowersAdapter {
    fn kind(&self) -> FrameworkKind {
        FrameworkKind::Superpowers
    }

    fn detect(&self, root: &Path) -> Option<FrameworkInventory> {
        let roots: Vec<String> = [SPECS_DIR, PLANS_DIR, SDD_DIR]
            .iter()
            .filter(|d| root.join(d).is_dir())
            .map(|d| d.to_string())
            .collect();
        if roots.is_empty() {
            return None;
        }
        let docs = self.documents(root).len();
        let tasks = self.tasks(root).len();
        Some(FrameworkInventory { kind: self.kind(), roots, docs, tasks, detected_at: Utc::now() })
    }

    fn documents(&self, root: &Path) -> Vec<FrameworkDoc> {
        let mut out = vec![];
        for (dir, doc_type) in [(SPECS_DIR, FrameworkDocType::Spec), (PLANS_DIR, FrameworkDocType::Plan)] {
            for path in md_files(&root.join(dir)) {
                out.push(self.doc_at(root, &path, doc_type));
            }
        }
        for path in ledger_files(root) {
            out.push(self.doc_at(root, &path, FrameworkDocType::Ledger));
        }
        out
    }

    fn read(&self, root: &Path, path: &str) -> Result<String> {
        read_within_root(root, path)
    }

    fn tasks(&self, root: &Path) -> Vec<ImportedTask> {
        let mut out = vec![];
        for path in md_files(&root.join(PLANS_DIR)) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let path_rel = rel(root, &path);
            for item in checkboxes(&text) {
                // Only checkbox lines under a "### Task N" heading are plan tasks;
                // a stray checklist elsewhere in the doc (there is none in practice,
                // but the rule is explicit) is not imported.
                let Some(heading) = item.heading.filter(|h| h.starts_with("Task ")) else { continue };
                out.push(ImportedTask {
                    title: item.text,
                    description: heading.clone(),
                    status_hint: Some(if item.checked { "done".to_string() } else { "todo".to_string() }),
                    source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor: heading },
                });
            }
        }
        out
    }

    fn decisions(&self, root: &Path) -> Vec<ImportedDecision> {
        let mut out = vec![];
        for path in ledger_files(root) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let path_rel = rel(root, &path);
            for (text, _line) in ruling_lines(&text) {
                out.push(ImportedDecision {
                    text,
                    source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor: "Ruling".to_string() },
                });
            }
        }
        out
    }

    fn instruction_targets(&self, root: &Path) -> Vec<PathBuf> {
        root_instruction_files(root)
    }
}

impl SuperpowersAdapter {
    fn doc_at(&self, root: &Path, path: &Path, doc_type: FrameworkDocType) -> FrameworkDoc {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let title = first_heading(&text).unwrap_or_else(|| file_stem(path));
        FrameworkDoc { kind: self.kind(), path: rel(root, path), title, doc_type, updated_at: mtime(path) }
    }
}

/// `*.md` files directly under `dir` (no recursion). Empty if `dir` doesn't exist.
fn md_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "md"))
        .collect();
    out.sort();
    out
}

/// `.superpowers/sdd/*/progress.md`: one listing of the `sdd` directory, then one
/// existence check per subdirectory.
fn ledger_files(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join(SDD_DIR)) else { return vec![] };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .map(|d| d.join("progress.md"))
        .filter(|p| p.is_file())
        .collect();
    out.sort();
    out
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frameworks/superpowers")
    }

    #[test]
    fn detects_roots_and_counts() {
        let a = SuperpowersAdapter;
        let inv = a.detect(&fixture()).expect("superpowers fixture should be detected");
        assert_eq!(inv.kind, FrameworkKind::Superpowers);
        assert_eq!(inv.roots.len(), 3);
        assert_eq!(inv.docs, 3, "one spec, one plan, one ledger");
        assert_eq!(inv.tasks, 3);
    }

    #[test]
    fn documents_carry_doc_types_and_titles() {
        let a = SuperpowersAdapter;
        let docs = a.documents(&fixture());
        assert_eq!(docs.len(), 3);
        let spec = docs.iter().find(|d| d.doc_type == FrameworkDocType::Spec).unwrap();
        assert_eq!(spec.title, "Example fixture spec");
        let plan = docs.iter().find(|d| d.doc_type == FrameworkDocType::Plan).unwrap();
        assert_eq!(plan.title, "Example fixture plan");
        let ledger = docs.iter().find(|d| d.doc_type == FrameworkDocType::Ledger).unwrap();
        assert!(ledger.path.ends_with("progress.md"), "{}", ledger.path);
    }

    #[test]
    fn tasks_come_from_checkboxes_under_task_headings() {
        let a = SuperpowersAdapter;
        let tasks = a.tasks(&fixture());
        assert_eq!(tasks.len(), 3);
        let done: Vec<_> = tasks.iter().filter(|t| t.status_hint.as_deref() == Some("done")).collect();
        assert_eq!(done.len(), 1);
        assert!(tasks.iter().any(|t| t.title == "write the widget module"));
        assert!(tasks.iter().all(|t| t.source_ref.framework == FrameworkKind::Superpowers));
    }

    #[test]
    fn decisions_come_from_ruling_lines() {
        let a = SuperpowersAdapter;
        let decisions = a.decisions(&fixture());
        assert_eq!(decisions.len(), 2);
        assert!(decisions[0].text.contains("widget module"));
    }

    #[test]
    fn instruction_targets_finds_claude_md() {
        let a = SuperpowersAdapter;
        let targets = a.instruction_targets(&fixture());
        assert_eq!(targets, vec![fixture().join("CLAUDE.md")]);
    }

    #[test]
    fn read_refuses_paths_that_escape_the_root() {
        let a = SuperpowersAdapter;
        assert!(a.read(&fixture(), "../CLAUDE.md").is_err());
        assert!(a.read(&fixture(), "/etc/passwd").is_err());
        assert!(a.read(&fixture(), "docs/superpowers/specs/2026-01-01-example.md").is_ok());
    }
}
