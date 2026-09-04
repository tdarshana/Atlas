use std::path::{Path, PathBuf};

use chrono::Utc;

use super::adapter::{mtime, read_doc_file, read_within_root, rel, root_instruction_files, FrameworkAdapter};
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
        // Listing only, no content read: `tasks` counts plan *files* (the task
        // source), not the checkbox items inside them — getting an exact item
        // count would mean reading every plan, which `detect` must not do.
        let plans = md_files(&root.join(PLANS_DIR)).len();
        let docs = md_files(&root.join(SPECS_DIR)).len() + plans + ledger_files(root).len();
        Some(FrameworkInventory { kind: self.kind(), roots, docs, tasks: plans, detected_at: Utc::now() })
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
            let Some(text) = read_doc_file(&path) else { continue };
            let path_rel = rel(root, &path);
            for item in checkboxes(&text) {
                // Only checkbox lines under a "### Task N" heading are plan tasks;
                // a stray checklist elsewhere in the doc (there is none in practice,
                // but the rule is explicit) is not imported.
                let Some(heading) = item.heading.filter(|h| h.starts_with("Task ")) else { continue };
                // "<heading>#<ordinal>": unique among the checkboxes under this
                // heading, since two tasks in the same section (a real case: see
                // the fixture) would otherwise share the same anchor.
                let anchor = format!("{heading}#{}", item.ordinal);
                out.push(ImportedTask {
                    title: item.text,
                    description: heading,
                    status_hint: Some(if item.checked { "done".to_string() } else { "todo".to_string() }),
                    source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor },
                });
            }
        }
        out
    }

    fn decisions(&self, root: &Path) -> Vec<ImportedDecision> {
        let mut out = vec![];
        for path in ledger_files(root) {
            let Some(text) = read_doc_file(&path) else { continue };
            let path_rel = rel(root, &path);
            for (text, _line, ordinal) in ruling_lines(&text) {
                out.push(ImportedDecision {
                    text,
                    source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor: format!("Ruling#{ordinal}") },
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
        let text = read_doc_file(path).unwrap_or_default();
        let title = first_heading(&text).unwrap_or_else(|| file_stem(path));
        FrameworkDoc { kind: self.kind(), path: rel(root, path), title, doc_type, updated_at: mtime(path) }
    }
}

/// `*.md` files directly under `dir` (no recursion). Empty if `dir` doesn't exist.
/// Metadata only (`read_dir` plus `is_file`/extension checks): never opens a file.
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
/// existence check per subdirectory. Metadata only, never opens a file.
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
        assert_eq!(inv.tasks, 1, "detect counts task-bearing files (one plan), not checkbox items");
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
    fn tasks_come_from_checkboxes_under_task_headings_with_unique_anchors() {
        let a = SuperpowersAdapter;
        let tasks = a.tasks(&fixture());
        assert_eq!(tasks.len(), 3);
        let done: Vec<_> = tasks.iter().filter(|t| t.status_hint.as_deref() == Some("done")).collect();
        assert_eq!(done.len(), 1);
        assert!(tasks.iter().any(|t| t.title == "write the widget module"));
        assert!(tasks.iter().all(|t| t.source_ref.framework == FrameworkKind::Superpowers));

        // The fixture's "Task 1" heading covers two checkboxes: their anchors must
        // differ, or a re-import could not tell the two tasks apart.
        let under_task_1: Vec<_> = tasks.iter().filter(|t| t.description == "Task 1: Set up the widget").collect();
        assert_eq!(under_task_1.len(), 2);
        assert_ne!(under_task_1[0].source_ref.anchor, under_task_1[1].source_ref.anchor);
        assert_eq!(under_task_1[0].source_ref.anchor, "Task 1: Set up the widget#1");
        assert_eq!(under_task_1[1].source_ref.anchor, "Task 1: Set up the widget#2");
    }

    #[test]
    fn decisions_come_from_ruling_lines_with_unique_anchors() {
        let a = SuperpowersAdapter;
        let decisions = a.decisions(&fixture());
        assert_eq!(decisions.len(), 2);
        assert!(decisions[0].text.contains("widget module"));
        assert_ne!(decisions[0].source_ref.anchor, decisions[1].source_ref.anchor);
        assert_eq!(decisions[0].source_ref.anchor, "Ruling#1");
        assert_eq!(decisions[1].source_ref.anchor, "Ruling#2");
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
