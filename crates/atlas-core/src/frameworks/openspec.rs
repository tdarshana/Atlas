use std::path::{Path, PathBuf};

use chrono::Utc;

use super::adapter::{mtime, read_doc_file, read_doc_prefix, read_within_root, rel, root_instruction_files, FrameworkAdapter};
use super::md::{checkboxes, first_heading, section_bullets};
use crate::models::{FrameworkDoc, FrameworkDocType, FrameworkInventory, FrameworkKind, ImportedDecision, ImportedTask, SourceRef};
use crate::Result;

const SPECS_DIR: &str = "openspec/specs";
const CHANGES_DIR: &str = "openspec/changes";

pub struct OpenspecAdapter;

impl FrameworkAdapter for OpenspecAdapter {
    fn kind(&self) -> FrameworkKind {
        FrameworkKind::Openspec
    }

    fn detect(&self, root: &Path) -> Option<FrameworkInventory> {
        let roots: Vec<String> = [SPECS_DIR, CHANGES_DIR]
            .iter()
            .filter(|d| root.join(d).is_dir())
            .map(|d| d.to_string())
            .collect();
        if roots.is_empty() {
            return None;
        }
        // Listing and per-file existence checks only, no content read: `tasks`
        // counts `tasks.md` files (the task source), not the checkbox items in
        // them, since counting items exactly would mean reading every one.
        let mut docs = md_files(&root.join(SPECS_DIR)).len();
        let mut tasks = 0usize;
        for change_dir in change_dirs(root) {
            if change_dir.join("proposal.md").is_file() {
                docs += 1;
            }
            if change_dir.join("tasks.md").is_file() {
                docs += 1;
                tasks += 1;
            }
            if change_dir.join("design.md").is_file() {
                docs += 1;
            }
        }
        Some(FrameworkInventory { kind: self.kind(), roots, docs, tasks, detected_at: Utc::now() })
    }

    fn documents(&self, root: &Path) -> Vec<FrameworkDoc> {
        let mut out = vec![];
        for path in md_files(&root.join(SPECS_DIR)) {
            out.push(self.doc_at(root, &path, FrameworkDocType::Spec));
        }
        for change_dir in change_dirs(root) {
            for (file, doc_type) in [
                ("proposal.md", FrameworkDocType::Proposal),
                ("tasks.md", FrameworkDocType::Tasks),
                ("design.md", FrameworkDocType::Plan),
            ] {
                let path = change_dir.join(file);
                if path.is_file() {
                    out.push(self.doc_at(root, &path, doc_type));
                }
            }
        }
        out
    }

    fn read(&self, root: &Path, path: &str) -> Result<String> {
        read_within_root(root, path)
    }

    fn tasks(&self, root: &Path) -> Vec<ImportedTask> {
        let mut out = vec![];
        for change_dir in change_dirs(root) {
            let path = change_dir.join("tasks.md");
            let Some(text) = read_doc_file(&path) else { continue };
            let path_rel = rel(root, &path);
            for item in checkboxes(&text) {
                // "<heading>#<ordinal>" when there is a heading, else the line
                // number: both are unique within the file, so two items sharing a
                // heading (or none) never collide.
                let anchor = match &item.heading {
                    Some(h) => format!("{h}#{}", item.ordinal),
                    None => format!("L{}", item.line),
                };
                out.push(ImportedTask {
                    title: item.text,
                    description: item.heading.clone().unwrap_or_default(),
                    status_hint: Some(if item.checked { "done".to_string() } else { "todo".to_string() }),
                    source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor },
                    parent_anchor: None,
                });
            }
        }
        out
    }

    fn decisions(&self, root: &Path) -> Vec<ImportedDecision> {
        let mut out = vec![];
        for change_dir in change_dirs(root) {
            for file in ["proposal.md", "design.md"] {
                let path = change_dir.join(file);
                let Some(text) = read_doc_file(&path) else { continue };
                let path_rel = rel(root, &path);
                for (text, _line, ordinal) in section_bullets(&text, "Decisions") {
                    out.push(ImportedDecision {
                        text,
                        source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor: format!("Decisions#{ordinal}") },
                    });
                }
            }
        }
        out
    }

    fn instruction_targets(&self, root: &Path) -> Vec<PathBuf> {
        root_instruction_files(root)
    }
}

impl OpenspecAdapter {
    fn doc_at(&self, root: &Path, path: &Path, doc_type: FrameworkDocType) -> FrameworkDoc {
        let text = read_doc_prefix(path).unwrap_or_default();
        let title = first_heading(&text).unwrap_or_else(|| file_stem(path));
        FrameworkDoc { kind: self.kind(), path: rel(root, path), title, doc_type, updated_at: mtime(path) }
    }
}

/// `*.md` files directly under `dir` (no recursion). Empty if `dir` doesn't exist.
/// Metadata only: never opens a file.
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

/// `openspec/changes/*`: one listing of the `changes` directory. Metadata only.
fn change_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join(CHANGES_DIR)) else { return vec![] };
    let mut out: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frameworks/openspec")
    }

    #[test]
    fn detects_roots_and_counts() {
        let a = OpenspecAdapter;
        let inv = a.detect(&fixture()).expect("openspec fixture should be detected");
        assert_eq!(inv.kind, FrameworkKind::Openspec);
        assert_eq!(inv.roots.len(), 2);
        assert_eq!(inv.docs, 4, "one spec, one proposal, one tasks, one design");
        assert_eq!(inv.tasks, 1, "detect counts task-bearing files (one tasks.md), not checkbox items");
    }

    #[test]
    fn documents_carry_doc_types_and_titles() {
        let a = OpenspecAdapter;
        let docs = a.documents(&fixture());
        assert_eq!(docs.len(), 4);
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Spec && d.title == "Widget spec"));
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Proposal));
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Tasks));
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Plan), "design.md maps to plan");
    }

    #[test]
    fn tasks_come_from_checkboxes() {
        let a = OpenspecAdapter;
        let tasks = a.tasks(&fixture());
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks.iter().filter(|t| t.status_hint.as_deref() == Some("done")).count(), 1);
    }

    #[test]
    fn decisions_come_from_the_decisions_section_with_unique_anchors() {
        let a = OpenspecAdapter;
        let decisions = a.decisions(&fixture());
        assert_eq!(decisions.len(), 2);
        assert!(decisions.iter().all(|d| d.source_ref.framework == FrameworkKind::Openspec));
        assert_ne!(decisions[0].source_ref.anchor, decisions[1].source_ref.anchor);
        assert_eq!(decisions[0].source_ref.anchor, "Decisions#1");
        assert_eq!(decisions[1].source_ref.anchor, "Decisions#2");
    }

    #[test]
    fn read_refuses_paths_that_escape_the_root() {
        let a = OpenspecAdapter;
        assert!(a.read(&fixture(), "../CLAUDE.md").is_err());
        assert!(a.read(&fixture(), "/etc/passwd").is_err());
        assert!(a.read(&fixture(), "openspec/specs/widget.md").is_ok());
    }
}
