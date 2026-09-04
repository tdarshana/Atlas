use std::path::{Path, PathBuf};

use chrono::Utc;

use super::adapter::{mtime, read_within_root, rel, root_instruction_files, FrameworkAdapter};
use super::md::{checkboxes, first_heading, section_bullets};
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
        let docs = self.documents(root).len();
        let tasks = self.tasks(root).len();
        Some(FrameworkInventory { kind: self.kind(), roots, docs, tasks, detected_at: Utc::now() })
    }

    fn documents(&self, root: &Path) -> Vec<FrameworkDoc> {
        let mut out = vec![];
        for feature_dir in feature_dirs(root) {
            for (file, doc_type) in [("spec.md", FrameworkDocType::Spec), ("plan.md", FrameworkDocType::Plan), ("tasks.md", FrameworkDocType::Tasks)] {
                let path = feature_dir.join(file);
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
        for feature_dir in feature_dirs(root) {
            let path = feature_dir.join("tasks.md");
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let path_rel = rel(root, &path);
            for item in checkboxes(&text) {
                out.push(ImportedTask {
                    title: item.text,
                    description: item.heading.clone().unwrap_or_default(),
                    status_hint: Some(if item.checked { "done".to_string() } else { "todo".to_string() }),
                    source_ref: SourceRef {
                        framework: self.kind(),
                        path: path_rel.clone(),
                        anchor: item.heading.unwrap_or_else(|| format!("L{}", item.line)),
                    },
                });
            }
        }
        out
    }

    fn decisions(&self, root: &Path) -> Vec<ImportedDecision> {
        let mut out = vec![];
        for feature_dir in feature_dirs(root) {
            let path = feature_dir.join("plan.md");
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let path_rel = rel(root, &path);
            // A plan's own "Decisions" section wins; a SpecKit plan without one
            // still carries its choices in "Technical Context", so fall back to it.
            let mut bullets = section_bullets(&text, "Decisions");
            let anchor = if bullets.is_empty() {
                bullets = section_bullets(&text, "Technical Context");
                "Technical Context"
            } else {
                "Decisions"
            };
            for (text, _line) in bullets {
                out.push(ImportedDecision {
                    text,
                    source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor: anchor.to_string() },
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

impl SpeckitAdapter {
    fn doc_at(&self, root: &Path, path: &Path, doc_type: FrameworkDocType) -> FrameworkDoc {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let title = first_heading(&text).unwrap_or_else(|| file_stem(path));
        FrameworkDoc { kind: self.kind(), path: rel(root, path), title, doc_type, updated_at: mtime(path) }
    }
}

/// `specs/*`: one listing of the `specs` directory.
fn feature_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join(SPECS_DIR)) else { return vec![] };
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frameworks/speckit")
    }

    #[test]
    fn detects_roots_and_counts() {
        let a = SpeckitAdapter;
        let inv = a.detect(&fixture()).expect("speckit fixture should be detected");
        assert_eq!(inv.kind, FrameworkKind::Speckit);
        assert_eq!(inv.roots.len(), 2);
        assert_eq!(inv.docs, 3, "one spec, one plan, one tasks");
        assert_eq!(inv.tasks, 3);
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
    fn decisions_fall_back_to_technical_context() {
        let a = SpeckitAdapter;
        let decisions = a.decisions(&fixture());
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[0].source_ref.anchor, "Technical Context");
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
