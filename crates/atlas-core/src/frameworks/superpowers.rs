use std::collections::HashSet;
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
        let completed = completed_plans(root);
        for path in md_files(&root.join(PLANS_DIR)) {
            // A ledger holds no tasks of its own; it is read once above for the plans
            // it marks complete.
            if is_ledger_filename(&path) {
                continue;
            }
            let Some(text) = read_doc_file(&path) else { continue };
            let path_rel = rel(root, &path);
            // A plan its ledger marks complete is done whatever its checkboxes say:
            // the ledger is written when the work merges, the plan file is not
            // ticked afterwards.
            let plan_done = completed.contains(&path_rel);
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
                    status_hint: Some(if plan_done || items.iter().all(|i| i.checked) { "done".to_string() } else { "todo".to_string() }),
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
                        status_hint: Some(if plan_done || item.checked { "done".to_string() } else { "todo".to_string() }),
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

/// The plans (as paths relative to `root`) that a `*.ledger.md` under `PLANS_DIR`
/// marks complete. Reads every ledger there, which `tasks` is allowed to do.
fn completed_plans(root: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    for path in md_files(&root.join(PLANS_DIR)).into_iter().filter(|p| is_ledger_filename(p)) {
        let Some(text) = read_doc_file(&path) else { continue };
        if ledger_marks_complete(&text) {
            out.insert(rel(root, &plan_of_ledger(root, &path, &text)));
        }
    }
    out
}

/// The plan a ledger belongs to: the `.md` path its first line names after `plan:`
/// (the `# SDD ledger — plan: docs/superpowers/plans/<name>.md` convention, which
/// lets two ledgers share one plan), else the plan sharing its file stem, so
/// `<name>.ledger.md` belongs to `<name>.md` next to it.
fn plan_of_ledger(root: &Path, ledger: &Path, text: &str) -> PathBuf {
    let first = text.lines().next().unwrap_or("");
    if let Some((_, after)) = first.split_once("plan:") {
        let token = after.split_whitespace().next().unwrap_or("").trim_end_matches(['.', ',', ';', ')']);
        if token.ends_with(".md") && !token.starts_with('/') && !token.split('/').any(|part| part == "..") {
            return root.join(token);
        }
    }
    let stem = ledger.file_name().and_then(|f| f.to_str()).unwrap_or("").trim_end_matches(".ledger.md");
    ledger.with_file_name(format!("{stem}.md"))
}

/// Whether a ledger says its plan's work is finished: some line has `phase`
/// followed, in the same clause, by `complete`, `closed` or `shipped`. A clause
/// ends at `.`, `;`, `:`, `(` or `)`, which is what keeps a task-level note such
/// as `Task 1: complete (delivered in Phase 2 commit ...; ...)` from counting.
fn ledger_marks_complete(text: &str) -> bool {
    text.lines().any(line_marks_complete)
}

fn line_marks_complete(line: &str) -> bool {
    const FINISHED: [&str; 3] = ["complete", "closed", "shipped"];
    let lower = line.to_lowercase();
    let mut rest = lower.as_str();
    while let Some(at) = rest.find("phase") {
        let after = &rest[at + "phase".len()..];
        let clause = after.split(['.', ';', ':', '(', ')']).next().unwrap_or("");
        if clause.split(|c: char| !c.is_alphanumeric()).any(|word| FINISHED.contains(&word)) {
            return true;
        }
        rest = after;
    }
    false
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
        assert_eq!(inv.docs, 5, "one spec, three plan-directory files (one a ledger), one sdd ledger");
        assert_eq!(inv.tasks, 3, "detect counts every plan-directory file, not checkbox items");
    }

    #[test]
    fn documents_carry_doc_types_and_titles() {
        let a = SuperpowersAdapter;
        let docs = a.documents(&fixture());
        assert_eq!(docs.len(), 5);
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
        // The first plan file: two "### Task N" headings, each a parent, plus three
        // checkboxes total (two under Task 1, one under Task 2) as their children.
        // Plus the second plan file's own "Task 1" heading and its one checkbox.
        assert_eq!(tasks.len(), 7);
        assert!(tasks.iter().all(|t| t.source_ref.framework == FrameworkKind::Superpowers));

        // Task 1's checkboxes, scoped to the first plan file (the second file has
        // its own "Task 1: Set up the widget" heading, covered separately below),
        // are cleaned of the fixture's bold markup and keep distinct, stable
        // anchors so a re-import can tell them apart.
        let under_task_1: Vec<_> = tasks
            .iter()
            .filter(|t| t.parent_anchor.as_deref() == Some("Task 1: Set up the widget") && t.source_ref.path.ends_with("2026-01-01-example.md"))
            .collect();
        assert_eq!(under_task_1.len(), 2);
        assert_ne!(under_task_1[0].source_ref.anchor, under_task_1[1].source_ref.anchor);
        assert_eq!(under_task_1[0].source_ref.anchor, "Task 1: Set up the widget#1");
        assert_eq!(under_task_1[1].source_ref.anchor, "Task 1: Set up the widget#2");
        assert!(under_task_1.iter().any(|t| t.title == "Step 1: Write the widget module"));
        assert!(under_task_1.iter().all(|t| t.description == "Task 1: Set up the widget"));
    }

    /// Two plan files can share a heading's exact text (a generic step title reused
    /// across phase plans, a real shape the fixture now covers). Each still gets its
    /// own parent, distinguished by `source_ref.path` even though the title, the
    /// anchor and `parent_anchor` text are identical.
    #[test]
    fn two_plan_files_sharing_a_heading_get_their_own_parents() {
        let a = SuperpowersAdapter;
        let tasks = a.tasks(&fixture());

        let parents: Vec<_> = tasks.iter().filter(|t| t.title == "Task 1: Set up the widget" && t.parent_anchor.is_none()).collect();
        assert_eq!(parents.len(), 2, "{parents:?}");
        assert_ne!(parents[0].source_ref.path, parents[1].source_ref.path);
        assert!(parents.iter().all(|p| p.source_ref.anchor == "Task 1: Set up the widget"));

        let second_file_child = tasks
            .iter()
            .find(|t| t.parent_anchor.as_deref() == Some("Task 1: Set up the widget") && t.source_ref.path.ends_with("2026-01-03-example-two.md"))
            .expect("the second file's checkbox is present");
        assert_eq!(second_file_child.title, "Step 1: Write the other widget module");
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
    fn a_line_marks_a_plan_complete_only_where_phase_and_a_finishing_word_share_a_clause() {
        for line in [
            "PHASE 1 COMPLETE: branch feat/phase1-core-daemon ready to merge.",
            "Final fix re-review: all addressed, 0 new (105 cargo tests). Phase 4 closed; ff-merge to main.",
            "Phase 10 complete at 0ee6854.",
            "Re-review 3c: APPROVED. Phase 13b plus Permissions complete at cc785ad; fast-forwarding main.",
            "Phase 15 complete (SKILLS-A dec4dc6 + 25583fc; SKILLS-B 841752b); merged at fc202e7.",
            "Phase 16 shipped.",
        ] {
            assert!(line_marks_complete(line), "{line}");
        }
        for line in [
            "Task 1: complete (delivered in Phase 2 commit 38cd720; test pending_memories_can_be_listed)",
            "T5: implementer DONE (a74e910). Review dispatched. Phase 5 browser pass starting.",
            "Fix wave: complete at eab5e25; scoped re-review dispatched.",
            "Phase 7 final review dispatched (sonnet): open until the fix wave closes.",
            "",
        ] {
            assert!(!line_marks_complete(line), "{line}");
        }
    }

    #[test]
    fn a_ledger_names_its_plan_on_its_first_line_or_shares_its_stem() {
        let root = Path::new("/repo");
        let ledger = root.join("docs/superpowers/plans/2026-01-01-a.ledger.md");
        let named = "# SDD ledger — plan: docs/superpowers/plans/2026-01-01-b.md\n\nScope.";
        assert_eq!(plan_of_ledger(root, &ledger, named), root.join("docs/superpowers/plans/2026-01-01-b.md"));
        let unnamed = "# SDD ledger — plan: (bounded request, no plan file) something\n";
        assert_eq!(plan_of_ledger(root, &ledger, unnamed), root.join("docs/superpowers/plans/2026-01-01-a.md"));
        let escaping = "# plan: ../../etc/passwd.md\n";
        assert_eq!(plan_of_ledger(root, &ledger, escaping), root.join("docs/superpowers/plans/2026-01-01-a.md"));
    }

    /// The fixture's `2026-01-02-example.ledger.md` names no plan and finishes
    /// nothing, so the fixture imports as before; a ledger that names a plan and
    /// closes its phase hints every task of that plan done, ticked or not.
    #[test]
    fn a_completing_ledger_hints_every_task_of_its_plan_done() {
        let dir = tempfile::tempdir().unwrap();
        let plans = dir.path().join(PLANS_DIR);
        std::fs::create_dir_all(&plans).unwrap();
        std::fs::write(plans.join("2026-01-05-finished.md"), "# Finished\n\n### Task 1: Ship it\n\n- [ ] Step 1: Do it\n- [ ] Step 2: Test it\n").unwrap();
        std::fs::write(plans.join("2026-01-06-open.md"), "# Open\n\n### Task 1: Start it\n\n- [ ] Step 1: Do it\n").unwrap();
        std::fs::write(
            plans.join("2026-01-05-finished-b.ledger.md"),
            "# SDD ledger — plan: docs/superpowers/plans/2026-01-05-finished.md\n\n## Progress\nTask 1: complete (delivered in Phase 2 commit abc)\nPhase 5 complete at abc1234; fast-forwarding main.\n",
        )
        .unwrap();
        std::fs::write(plans.join("2026-01-06-open.ledger.md"), "# SDD ledger — plan: docs/superpowers/plans/2026-01-06-open.md\n\n## Progress\nTask 1: dispatched.\n").unwrap();

        let tasks = SuperpowersAdapter.tasks(dir.path());
        let hints: Vec<(&str, &str)> = tasks.iter().map(|t| (t.title.as_str(), t.status_hint.as_deref().unwrap())).collect();
        assert_eq!(
            hints,
            vec![("Task 1: Ship it", "done"), ("Step 1: Do it", "done"), ("Step 2: Test it", "done"), ("Task 1: Start it", "todo"), ("Step 1: Do it", "todo")],
        );
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
