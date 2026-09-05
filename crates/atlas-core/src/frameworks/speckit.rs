use std::path::{Path, PathBuf};

use chrono::Utc;

use super::adapter::{doc_at, read_doc_file, read_within_root, rel, root_instruction_files, FrameworkAdapter};
use super::md::{checkboxes, section_bullets};
use crate::models::{FrameworkDoc, FrameworkDocType, FrameworkInventory, FrameworkKind, ImportedDecision, ImportedTask, SourceRef};
use crate::Result;

const SPECIFY_DIR: &str = ".specify";
const SPECS_DIR: &str = "specs";

pub struct SpeckitAdapter;

impl FrameworkAdapter for SpeckitAdapter {
    fn kind(&self) -> FrameworkKind {
        FrameworkKind::Speckit
    }

    fn detect(&self, root: &Path) -> Option<FrameworkInventory> {
        let roots: Vec<String> = [SPECIFY_DIR, SPECS_DIR]
            .iter()
            .filter(|d| root.join(d).is_dir())
            .map(|d| d.to_string())
            .collect();
        if roots.is_empty() {
            return None;
        }
        // Listing and per-file existence checks only, no content read: `tasks`
        // counts `tasks.md` files (the task source), not the checkbox items in
        // them.
        let mut docs = 0usize;
        let mut tasks = 0usize;
        for feature_dir in feature_dirs(root) {
            if feature_dir.join("spec.md").is_file() {
                docs += 1;
            }
            if feature_dir.join("plan.md").is_file() {
                docs += 1;
            }
            if feature_dir.join("tasks.md").is_file() {
                docs += 1;
                tasks += 1;
            }
        }
        Some(FrameworkInventory { kind: self.kind(), roots, docs, tasks, detected_at: Utc::now() })
    }

    fn documents(&self, root: &Path) -> Vec<FrameworkDoc> {
        let mut out = vec![];
        for feature_dir in feature_dirs(root) {
            for (file, doc_type) in [("spec.md", FrameworkDocType::Spec), ("plan.md", FrameworkDocType::Plan), ("tasks.md", FrameworkDocType::Tasks)] {
                let path = feature_dir.join(file);
                if path.is_file() {
                    out.push(doc_at(self.kind(), root, &path, doc_type));
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
        for feature_dir in feature_dirs(root) {
            let path = feature_dir.join("tasks.md");
            let Some(text) = read_doc_file(&path) else { continue };
            let path_rel = rel(root, &path);
            for item in checkboxes(&text) {
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
        for feature_dir in feature_dirs(root) {
            let path = feature_dir.join("plan.md");
            let Some(text) = read_doc_file(&path) else { continue };
            let path_rel = rel(root, &path);
            // A plan's own "Decisions" section wins; a SpecKit plan without one
            // still carries its choices in "Technical Context", so fall back to it.
            let mut bullets = section_bullets(&text, "Decisions");
            let anchor_name = if bullets.is_empty() {
                bullets = section_bullets(&text, "Technical Context");
                "Technical Context"
            } else {
                "Decisions"
            };
            for (text, _line, ordinal) in bullets {
                out.push(ImportedDecision {
                    text,
                    source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor: format!("{anchor_name}#{ordinal}") },
                });
            }
        }
        out
    }

    fn instruction_targets(&self, root: &Path) -> Vec<PathBuf> {
        // `.specify/memory/constitution.md` is SpecKit's own read-only convention:
        // Atlas never targets it, only the generic `CLAUDE.md`/`AGENTS.md` at root.
        root_instruction_files(root)
    }
}

/// `specs/*`: one listing of the `specs` directory. Metadata only, never opens a
/// file.
fn feature_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join(SPECS_DIR)) else { return vec![] };
    let mut out: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::adapter::fixtures_dir;

    fn fixture() -> PathBuf {
        fixtures_dir("speckit")
    }

    #[test]
    fn detects_roots_and_counts() {
        let a = SpeckitAdapter;
        let inv = a.detect(&fixture()).expect("speckit fixture should be detected");
        assert_eq!(inv.kind, FrameworkKind::Speckit);
        assert_eq!(inv.roots.len(), 2);
        assert_eq!(inv.docs, 3, "one spec, one plan, one tasks");
        assert_eq!(inv.tasks, 1, "detect counts task-bearing files (one tasks.md), not checkbox items");
    }

    #[test]
    fn documents_carry_doc_types_and_titles() {
        let a = SpeckitAdapter;
        let docs = a.documents(&fixture());
        assert_eq!(docs.len(), 3);
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Spec && d.title == "Widget feature spec"));
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Plan));
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Tasks));
    }

    #[test]
    fn tasks_come_from_checkboxes() {
        let a = SpeckitAdapter;
        let tasks = a.tasks(&fixture());
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks.iter().filter(|t| t.status_hint.as_deref() == Some("done")).count(), 1);
    }

    #[test]
    fn decisions_fall_back_to_technical_context_with_unique_anchors() {
        let a = SpeckitAdapter;
        let decisions = a.decisions(&fixture());
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[0].source_ref.anchor, "Technical Context#1");
        assert_eq!(decisions[1].source_ref.anchor, "Technical Context#2");
    }

    #[test]
    fn instruction_targets_excludes_the_readonly_constitution() {
        let a = SpeckitAdapter;
        let targets = a.instruction_targets(&fixture());
        assert_eq!(targets, vec![fixture().join("AGENTS.md")]);
        assert!(!targets.iter().any(|p| p.ends_with("constitution.md")));
    }

    #[test]
    fn read_refuses_paths_that_escape_the_root() {
        let a = SpeckitAdapter;
        assert!(a.read(&fixture(), "../CLAUDE.md").is_err());
        assert!(a.read(&fixture(), "/etc/passwd").is_err());
        assert!(a.read(&fixture(), "specs/001-widget/spec.md").is_ok());
    }
}
