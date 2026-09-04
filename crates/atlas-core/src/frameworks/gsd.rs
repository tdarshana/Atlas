use std::path::{Path, PathBuf};

use chrono::Utc;

use super::adapter::{mtime, read_doc_file, read_within_root, rel, FrameworkAdapter};
use super::md::{checkboxes, first_heading, section_bullets};
use crate::models::{FrameworkDoc, FrameworkDocType, FrameworkInventory, FrameworkKind, ImportedDecision, ImportedTask, SourceRef};
use crate::Result;

const PLANNING_DIR: &str = ".planning";

pub struct GsdAdapter;

impl FrameworkAdapter for GsdAdapter {
    fn kind(&self) -> FrameworkKind {
        FrameworkKind::Gsd
    }

    fn detect(&self, root: &Path) -> Option<FrameworkInventory> {
        if !root.join(PLANNING_DIR).is_dir() {
            return None;
        }
        // Listing and per-file existence checks only, no content read: `tasks`
        // counts `PLAN.md` files (the task source), not the checkbox items in
        // them.
        let planning = root.join(PLANNING_DIR);
        let mut docs = 0usize;
        if planning.join("PROJECT.md").is_file() {
            docs += 1;
        }
        if planning.join("ROADMAP.md").is_file() {
            docs += 1;
        }
        let mut tasks = 0usize;
        for phase_dir in phase_dirs(root) {
            if phase_dir.join("PLAN.md").is_file() {
                docs += 1;
                tasks += 1;
            }
            if phase_dir.join("SUMMARY.md").is_file() {
                docs += 1;
            }
        }
        docs += todo_files(root).len();
        Some(FrameworkInventory { kind: self.kind(), roots: vec![PLANNING_DIR.to_string()], docs, tasks, detected_at: Utc::now() })
    }

    fn documents(&self, root: &Path) -> Vec<FrameworkDoc> {
        let mut out = vec![];
        let planning = root.join(PLANNING_DIR);
        for (file, doc_type) in [("PROJECT.md", FrameworkDocType::Summary), ("ROADMAP.md", FrameworkDocType::Roadmap)] {
            let path = planning.join(file);
            if path.is_file() {
                out.push(self.doc_at(root, &path, doc_type));
            }
        }
        for phase_dir in phase_dirs(root) {
            for (file, doc_type) in [("PLAN.md", FrameworkDocType::Plan), ("SUMMARY.md", FrameworkDocType::Summary)] {
                let path = phase_dir.join(file);
                if path.is_file() {
                    out.push(self.doc_at(root, &path, doc_type));
                }
            }
        }
        for path in todo_files(root) {
            out.push(self.doc_at(root, &path, FrameworkDocType::Todo));
        }
        out
    }

    fn read(&self, root: &Path, path: &str) -> Result<String> {
        read_within_root(root, path)
    }

    fn tasks(&self, root: &Path) -> Vec<ImportedTask> {
        let mut out = vec![];
        for phase_dir in phase_dirs(root) {
            let path = phase_dir.join("PLAN.md");
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
                });
            }
        }
        out
    }

    fn decisions(&self, root: &Path) -> Vec<ImportedDecision> {
        let mut out = vec![];
        for phase_dir in phase_dirs(root) {
            let path = phase_dir.join("SUMMARY.md");
            let Some(text) = read_doc_file(&path) else { continue };
            let path_rel = rel(root, &path);
            for (text, _line, ordinal) in section_bullets(&text, "Decisions") {
                out.push(ImportedDecision {
                    text,
                    source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor: format!("Decisions#{ordinal}") },
                });
            }
        }
        out
    }

    /// GSD has no agent-instruction-file convention of its own: `.planning/` holds
    /// only planning documents, so there is nowhere for `atlas sync` to target.
    fn instruction_targets(&self, _root: &Path) -> Vec<PathBuf> {
        vec![]
    }
}

impl GsdAdapter {
    fn doc_at(&self, root: &Path, path: &Path, doc_type: FrameworkDocType) -> FrameworkDoc {
        let text = read_doc_file(path).unwrap_or_default();
        let title = first_heading(&text).unwrap_or_else(|| file_stem(path));
        FrameworkDoc { kind: self.kind(), path: rel(root, path), title, doc_type, updated_at: mtime(path) }
    }
}

/// `.planning/phases/*`: one listing of the `phases` directory. Metadata only.
fn phase_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join(PLANNING_DIR).join("phases")) else { return vec![] };
    let mut out: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    out.sort();
    out
}

/// `.planning/todos/{pending,done}/*.md`: one listing per subdirectory. Metadata
/// only, never opens a file.
fn todo_files(root: &Path) -> Vec<PathBuf> {
    let todos = root.join(PLANNING_DIR).join("todos");
    let mut out = vec![];
    for state in ["pending", "done"] {
        let Ok(entries) = std::fs::read_dir(todos.join(state)) else { continue };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "md"))
            .collect();
        files.sort();
        out.extend(files);
    }
    out
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frameworks/gsd")
    }

    #[test]
    fn detects_root_and_counts() {
        let a = GsdAdapter;
        let inv = a.detect(&fixture()).expect("gsd fixture should be detected");
        assert_eq!(inv.kind, FrameworkKind::Gsd);
        assert_eq!(inv.roots, vec![".planning".to_string()]);
        assert_eq!(inv.docs, 6, "project, roadmap, plan, summary, and two todos");
        assert_eq!(inv.tasks, 1, "detect counts task-bearing files (one PLAN.md), not checkbox items");
    }

    #[test]
    fn documents_carry_doc_types_including_todos() {
        let a = GsdAdapter;
        let docs = a.documents(&fixture());
        assert_eq!(docs.len(), 6);
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Roadmap));
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Plan));
        assert!(docs.iter().any(|d| d.doc_type == FrameworkDocType::Summary && d.title == "Widget project"));
        assert_eq!(docs.iter().filter(|d| d.doc_type == FrameworkDocType::Todo).count(), 2);
    }

    #[test]
    fn tasks_come_from_plan_checkboxes() {
        let a = GsdAdapter;
        let tasks = a.tasks(&fixture());
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks.iter().filter(|t| t.status_hint.as_deref() == Some("done")).count(), 1);
    }

    #[test]
    fn decisions_come_from_summary_decisions_section_with_unique_anchors() {
        let a = GsdAdapter;
        let decisions = a.decisions(&fixture());
        assert_eq!(decisions.len(), 2);
        assert!(decisions.iter().all(|d| d.source_ref.framework == FrameworkKind::Gsd));
        assert_eq!(decisions[0].source_ref.anchor, "Decisions#1");
        assert_eq!(decisions[1].source_ref.anchor, "Decisions#2");
    }

    #[test]
    fn instruction_targets_is_always_empty() {
        let a = GsdAdapter;
        assert!(a.instruction_targets(&fixture()).is_empty());
    }

    #[test]
    fn read_refuses_paths_that_escape_the_root() {
        let a = GsdAdapter;
        assert!(a.read(&fixture(), "../CLAUDE.md").is_err());
        assert!(a.read(&fixture(), "/etc/passwd").is_err());
        assert!(a.read(&fixture(), ".planning/PROJECT.md").is_ok());
    }
}
