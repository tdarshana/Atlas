use std::path::{Path, PathBuf};

use chrono::Utc;

use super::adapter::{mtime, read_doc_file, read_doc_prefix, read_within_root, rel, root_instruction_files, FrameworkAdapter};
use super::md::{checkboxes, clean_step_title, first_heading, ruling_lines, Checkbox};
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
                // A plan file named `*.ledger.md` is a ledger despite living under
                // `PLANS_DIR`: the convention some Superpowers projects use for a
                // standalone decision ledger that isn't a `.superpowers/sdd/*/progress.md`.
                let doc_type = if dir == PLANS_DIR && is_ledger_filename(&path) { FrameworkDocType::Ledger } else { doc_type };
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
            // Group the plan's checkboxes by their "### Task N" heading, keeping the
            // order each heading is first seen in the file: a stray checklist with no
            // such heading (there is none in practice, but the rule is explicit) is
            // not imported at all. Each group becomes a parent task, its checkboxes
            // becoming that parent's subtasks.
            let mut groups: Vec<(String, Vec<Checkbox>)> = Vec::new();
            for item in checkboxes(&text) {
                let Some(heading) = item.heading.clone().filter(|h| h.starts_with("Task ")) else { continue };
                match groups.iter_mut().find(|(h, _)| *h == heading) {
                    Some((_, items)) => items.push(item),
                    None => groups.push((heading, vec![item])),
                }
            }
            for (heading, items) in groups {
                // The parent's own anchor is the heading text, with no ordinal: it is
                // unique within the file since two "### Task N" headings never share a
                // title (each names a distinct task number).
                out.push(ImportedTask {
                    title: heading.clone(),
                    description: path_rel.clone(),
                    status_hint: Some(if items.iter().all(|i| i.checked) { "done".to_string() } else { "todo".to_string() }),
                    source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor: heading.clone() },
                    parent_anchor: None,
                });
                for item in items {
                    // "<heading>#<ordinal>": unique among the checkboxes under this
                    // heading, since two tasks in the same section (a real case: see
                    // the fixture) would otherwise share the same anchor.
                    let anchor = format!("{heading}#{}", item.ordinal);
                    out.push(ImportedTask {
                        title: clean_step_title(&item.text),
                        description: heading.clone(),
                        status_hint: Some(if item.checked { "done".to_string() } else { "todo".to_string() }),
                        source_ref: SourceRef { framework: self.kind(), path: path_rel.clone(), anchor },
                        parent_anchor: Some(heading.clone()),
                    });
                }
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
        let text = read_doc_prefix(path).unwrap_or_default();
        let title = first_heading(&text).unwrap_or_else(|| file_stem(path));
        FrameworkDoc { kind: self.kind(), path: rel(root, path), title, doc_type, updated_at: mtime(path) }
    }
}

/// Whether `path`'s file name ends `.ledger.md`, the convention a standalone
/// decision ledger under `PLANS_DIR` uses to mark itself as one rather than a
/// plan.
fn is_ledger_filename(path: &Path) -> bool {
    path.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.ends_with(".ledger.md"))
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
        assert_eq!(inv.docs, 4, "one spec, two plan-directory files (one a ledger), one sdd ledger");
        assert_eq!(inv.tasks, 2, "detect counts every plan-directory file, not checkbox items");
    }

    #[test]
    fn documents_carry_doc_types_and_titles() {
        let a = SuperpowersAdapter;
        let docs = a.documents(&fixture());
        assert_eq!(docs.len(), 4);
        let spec = docs.iter().find(|d| d.doc_type == FrameworkDocType::Spec).unwrap();
        assert_eq!(spec.title, "Example fixture spec");
        let plan = docs.iter().find(|d| d.doc_type == FrameworkDocType::Plan).unwrap();
        assert_eq!(plan.title, "Example fixture plan");
        assert_eq!(docs.iter().filter(|d| d.doc_type == FrameworkDocType::Ledger).count(), 2);
        let sdd_ledger = docs.iter().find(|d| d.doc_type == FrameworkDocType::Ledger && d.path.ends_with("progress.md")).unwrap();
        assert!(sdd_ledger.path.contains(".superpowers/sdd"), "{}", sdd_ledger.path);
    }

    /// A file matching `*.ledger.md` under `PLANS_DIR` is a ledger, not a plan,
    /// even though it lives beside the plan files.
    #[test]
    fn a_ledger_named_plan_directory_file_is_typed_ledger_not_plan() {
        let a = SuperpowersAdapter;
        let docs = a.documents(&fixture());
        let doc = docs.iter().find(|d| d.path.ends_with(".ledger.md")).expect("fixture has a *.ledger.md file");
        assert_eq!(doc.doc_type, FrameworkDocType::Ledger);
        assert_eq!(doc.title, "Example fixture ledger");
    }

    #[test]
    fn tasks_come_from_checkboxes_under_task_headings_with_unique_anchors() {
        let a = SuperpowersAdapter;
        let tasks = a.tasks(&fixture());
        // Two "### Task N" headings, each a parent, plus three checkboxes total
        // (two under Task 1, one under Task 2) as their children.
        assert_eq!(tasks.len(), 5);
        assert!(tasks.iter().all(|t| t.source_ref.framework == FrameworkKind::Superpowers));

        // Task 1's checkboxes are cleaned of the fixture's bold markup and keep
        // distinct, stable anchors so a re-import can tell them apart.
        let under_task_1: Vec<_> =
            tasks.iter().filter(|t| t.parent_anchor.as_deref() == Some("Task 1: Set up the widget")).collect();
        assert_eq!(under_task_1.len(), 2);
        assert_ne!(under_task_1[0].source_ref.anchor, under_task_1[1].source_ref.anchor);
        assert_eq!(under_task_1[0].source_ref.anchor, "Task 1: Set up the widget#1");
        assert_eq!(under_task_1[1].source_ref.anchor, "Task 1: Set up the widget#2");
        assert!(under_task_1.iter().any(|t| t.title == "Step 1: Write the widget module"));
        assert!(under_task_1.iter().all(|t| t.description == "Task 1: Set up the widget"));
    }

    #[test]
    fn a_parent_precedes_its_children_with_its_own_anchor_and_no_parent() {
        let a = SuperpowersAdapter;
        let tasks = a.tasks(&fixture());

        let task_1_pos = tasks.iter().position(|t| t.title == "Task 1: Set up the widget").expect("Task 1 parent present");
        let parent = &tasks[task_1_pos];
        assert_eq!(parent.parent_anchor, None);
        assert_eq!(parent.source_ref.anchor, "Task 1: Set up the widget");
        assert_eq!(parent.description, "docs/superpowers/plans/2026-01-01-example.md");

        // Both of Task 1's children come right after their parent in the list.
        assert_eq!(tasks[task_1_pos + 1].parent_anchor.as_deref(), Some("Task 1: Set up the widget"));
        assert_eq!(tasks[task_1_pos + 2].parent_anchor.as_deref(), Some("Task 1: Set up the widget"));
    }

    #[test]
    fn a_heading_whose_checkboxes_are_all_checked_hints_the_parent_done() {
        let a = SuperpowersAdapter;
        let tasks = a.tasks(&fixture());

        // Task 2's one checkbox is checked, so its parent hints done.
        let task_2 = tasks.iter().find(|t| t.title == "Task 2: Wire the widget in").expect("Task 2 parent present");
        assert_eq!(task_2.status_hint.as_deref(), Some("done"));

        // Task 1 has one unchecked checkbox, so its parent stays todo even though
        // the other checkbox is checked.
        let task_1 = tasks.iter().find(|t| t.title == "Task 1: Set up the widget").expect("Task 1 parent present");
        assert_eq!(task_1.status_hint.as_deref(), Some("todo"));
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
