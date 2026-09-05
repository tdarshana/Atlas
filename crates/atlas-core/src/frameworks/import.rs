//! Imports a framework's tasks and decisions into the board and into memory.
//!
//! An imported task stays linked to the file it came from by `SourceRef`, so
//! running an import twice updates the tasks it already created rather than
//! filing duplicates, and never moves a task's stage back: the board owns
//! progress once a task exists, and the one stage an import writes to an existing
//! task is the done stage, when the framework now says the item is done. An
//! imported decision becomes a pending memory,
//! deduplicated by an exact text match against this project's memories already
//! tagged for the framework.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::board::TaskRepo;
use crate::frameworks;
use crate::memories::MemoryRepo;
use crate::models::*;
use crate::{AtlasError, Result};
use uuid::Uuid;

/// Free-text status hints (as a framework spells "done" in its own checkboxes or
/// status lines) that map an imported task onto the board's first *done* stage
/// instead of its first stage.
fn is_done_hint(hint: &str) -> bool {
    matches!(hint.trim().to_lowercase().as_str(), "done" | "x" | "complete" | "checked")
}

/// The stage a newly imported task lands in: the first done stage for a hint that
/// reads done, else the board's first stage.
fn stage_for_hint(stages: &[Stage], hint: Option<&str>) -> Result<String> {
    let done = hint.map(is_done_hint).unwrap_or(false);
    if done {
        if let Some(s) = stages.iter().find(|s| s.done) {
            return Ok(s.name.clone());
        }
    }
    stages.first().map(|s| s.name.clone()).ok_or_else(|| AtlasError::Invalid("the board has no stages".into()))
}

fn adapter_for(kind: FrameworkKind) -> Result<Box<dyn frameworks::FrameworkAdapter>> {
    frameworks::adapters().into_iter().find(|a| a.kind() == kind).ok_or_else(|| AtlasError::Invalid(format!("unknown framework: {kind}")))
}

/// Creates or updates board tasks from `kind`'s adapter under `project`, keyed by
/// `source_ref` so a re-import never duplicates a task. An existing task's title
/// and description are refreshed when the source changed and left alone when they
/// match; its stage is touched only to move it from the board's first stage into
/// its done stage when the item now hints done (a plan whose ledger closed after
/// the first import landed its tasks in Backlog). A task anywhere else was moved
/// by hand and stays where the board put it. An item whose `parent_anchor` names
/// another item from the same adapter run becomes (or is moved to be) that item's
/// subtask; the adapter emits a parent immediately before its children, so this
/// resolves without a second pass over `items`.
///
/// Every task write is attributed to `import/<kind>` (both `created_by` and the
/// event actor), so the board history shows the item came from the framework
/// rather than from whoever triggered the import; `actor`, the caller that
/// triggered it, is recorded on one summary audit row instead.
pub fn import_tasks(tasks: &TaskRepo, memories: &MemoryRepo, project: &Project, kind: FrameworkKind, actor: &str) -> Result<ImportReport> {
    let adapter = adapter_for(kind)?;
    let root = Path::new(&project.root_path);
    let items = adapter.tasks(root);
    let stages = tasks.effective_stages(Some(project.id))?.stages;
    let import_actor = format!("import/{}", kind.as_str());

    let mut report = ImportReport::default();
    // `(source_ref.path, source_ref.anchor)` to task id, filled in as each item's
    // task is created or matched below. A child's `parent_anchor` looks itself up
    // here first, scoped to its own file: two different plan files can otherwise
    // share a heading's exact text (a generic step title reused across phase
    // plans), and without the path a second file's children would resolve against
    // the first file's parent instead of falling through to the path-scoped
    // `find_by_source_ref` below.
    let mut anchor_ids: HashMap<(String, String), Uuid> = HashMap::new();

    for item in items {
        let parent_id: Option<Uuid> = match &item.parent_anchor {
            None => None,
            Some(anchor) => match anchor_ids.get(&(item.source_ref.path.clone(), anchor.clone())) {
                Some(id) => Some(*id),
                // The parent wasn't created earlier in *this* run (an import of just
                // the children, say, or a run order the adapter doesn't guarantee),
                // so fall back to the parent's own SourceRef, the same lookup a
                // child uses for itself.
                None => {
                    let parent_ref = SourceRef { framework: kind, path: item.source_ref.path.clone(), anchor: anchor.clone() };
                    tasks.find_by_source_ref(Some(project.id), &parent_ref)?.map(|t| t.id)
                }
            },
        };

        let task_id = match tasks.find_by_source_ref(Some(project.id), &item.source_ref)? {
            Some(existing) => {
                let mut upd = TaskUpdate::default();
                let content_changed = existing.title != item.title || existing.description != item.description;
                if content_changed {
                    upd.title = Some(item.title.clone());
                    upd.description = Some(item.description.clone());
                }
                let reparent = parent_id != existing.parent_id;
                if reparent {
                    upd.parent = Some(parent_id.map(|id| id.to_string()));
                }
                if content_changed || reparent {
                    tasks.update(&existing.key, &upd, &import_actor)?;
                    if reparent {
                        report.reparented += 1;
                    }
                }
                let now_done = item.status_hint.as_deref().is_some_and(is_done_hint);
                let untouched = stages.first().is_some_and(|s| s.name == existing.stage);
                let finish = match stages.iter().find(|s| s.done) {
                    Some(done) if now_done && untouched => Some(done.name.clone()),
                    _ => None,
                };
                if let Some(done) = &finish {
                    tasks.move_stage(&existing.key, done, None, &import_actor)?;
                }
                if content_changed || finish.is_some() {
                    report.updated += 1;
                } else if !reparent {
                    report.skipped += 1;
                }
                existing.id
            }
            None => {
                let stage = stage_for_hint(&stages, item.status_hint.as_deref())?;
                let new = NewTask {
                    project_id: Some(project.id),
                    title: item.title.clone(),
                    description: Some(item.description.clone()),
                    stage: Some(stage),
                    source_ref: Some(item.source_ref.clone()),
                    parent: parent_id.map(|id| id.to_string()),
                    ..Default::default()
                };
                let created = tasks.create(&new, &import_actor)?;
                report.created += 1;
                created.id
            }
        };
        anchor_ids.insert((item.source_ref.path.clone(), item.source_ref.anchor.clone()), task_id);
    }
    memories.audit(
        actor,
        "import_tasks",
        "project",
        Some(project.id),
        serde_json::json!({
            "framework": kind.as_str(),
            "created": report.created,
            "updated": report.updated,
            "skipped": report.skipped,
            "reparented": report.reparented,
        }),
    )?;
    Ok(report)
}

/// Creates pending memories from `kind`'s decisions under `project`, one per
/// ruling the adapter found, skipping any whose text already exists among this
/// project's memories tagged for the framework (across every status, not just
/// active, so a decision already reviewed or rejected is not re-proposed either).
pub fn import_decisions(memories: &MemoryRepo, project: &Project, kind: FrameworkKind, actor: &str) -> Result<ImportReport> {
    let adapter = adapter_for(kind)?;
    let root = Path::new(&project.root_path);
    let items = adapter.decisions(root);

    let mut existing_texts: HashSet<String> = HashSet::new();
    for status in [MemoryStatus::Active, MemoryStatus::Pending, MemoryStatus::Rejected, MemoryStatus::Superseded] {
        for m in memories.list_by_status_scoped(status, None, Some(project.id), MemoryScopeFilter::ProjectOnly, MemoryPage::default())? {
            if m.tags.iter().any(|t| t == kind.as_str()) {
                existing_texts.insert(m.text);
            }
        }
    }

    let import_actor = format!("import/{}", kind.as_str());
    let mut report = ImportReport::default();
    for item in items {
        if existing_texts.contains(&item.text) {
            report.skipped += 1;
            continue;
        }
        let new = NewMemory {
            scope: MemoryScope::Project,
            project_id: Some(project.id),
            kind: MemoryKind::Decision,
            text: item.text.clone(),
            tags: vec![kind.as_str().to_string(), "import".to_string()],
            source_agent: None,
            source_tool: Some(import_actor.clone()),
            confidence: 1.0,
            status: MemoryStatus::Pending,
        };
        memories.insert(&new, &import_actor)?;
        existing_texts.insert(item.text);
        report.created += 1;
    }
    memories.audit(
        actor,
        "import_decisions",
        "project",
        Some(project.id),
        serde_json::json!({"framework": kind.as_str(), "created": report.created, "updated": report.updated, "skipped": report.skipped}),
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::TaskRepo;
    use crate::db::Db;
    use crate::projects::ProjectRepo;
    use std::sync::{Arc, Mutex};

    fn fixtures_dir(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frameworks").join(name)
    }

    /// A project row pointed at one of the frameworks fixtures under `tests/fixtures`,
    /// with a copy of the fixture in a tempdir so a test can freely add/read files
    /// without mutating the checked-in fixture.
    fn project_for_fixture(db: &Arc<Db>, name: &str) -> (Project, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        copy_dir(&fixtures_dir(name), dir.path());
        let repo = ProjectRepo::new(db);
        let detected = crate::projects::detect_root(dir.path()).unwrap();
        let project = repo.upsert(&detected, None, "test").unwrap();
        (project, dir)
    }

    fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let target = dst.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                std::fs::create_dir_all(&target).unwrap();
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    fn setup() -> (Arc<Db>, TaskRepo) {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let tasks = TaskRepo::new(db.clone(), Arc::new(Mutex::new(())));
        (db, tasks)
    }

    #[test]
    fn importing_tasks_twice_updates_rather_than_duplicates_and_keeps_a_moved_stage() {
        let (db, tasks) = setup();
        let memories = MemoryRepo::new(&db);
        let (project, _dir) = project_for_fixture(&db, "superpowers");

        let first = import_tasks(&tasks, &memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        assert!(first.created > 0, "the fixture should have importable tasks: {first:?}");
        assert_eq!(first.updated, 0);

        let all = tasks.list(&TaskFilter { project_id: Some(project.id), include_done: true, ..Default::default() }).unwrap();
        assert_eq!(all.len(), first.created);
        assert!(all.iter().all(|t| t.created_by == "import/superpowers"));
        assert!(all.iter().all(|t| t.source_ref.is_some()));

        // Move one task forward by hand, the way a human working the board would.
        let moved = &all[0];
        let stages = tasks.effective_stages(Some(project.id)).unwrap().stages;
        let target = stages.iter().find(|s| s.name != moved.stage).expect("more than one stage").name.clone();
        let after_move = tasks.move_stage(&moved.key, &target, None, "human").unwrap();
        assert_eq!(after_move.stage, target);

        // Re-importing must not duplicate anything, and must leave the moved task's
        // stage exactly where the human left it: the board owns progress.
        let second = import_tasks(&tasks, &memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        assert_eq!(second.created, 0, "a re-import must not create new tasks: {second:?}");

        let again = tasks.list(&TaskFilter { project_id: Some(project.id), include_done: true, ..Default::default() }).unwrap();
        assert_eq!(again.len(), first.created, "no duplicates after a second import");
        let still_moved = again.iter().find(|t| t.key == moved.key).unwrap();
        assert_eq!(still_moved.stage, target, "re-import must not move a task's stage back");
    }

    #[test]
    fn imports_superpowers_plan_tasks_as_parents_with_linked_children() {
        let (db, tasks) = setup();
        let memories = MemoryRepo::new(&db);
        let (project, _dir) = project_for_fixture(&db, "superpowers");

        let report = import_tasks(&tasks, &memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        // The first plan file: two parents, two children under Task 1, one under
        // Task 2 (five items). Plus the second plan file's own parent and child.
        assert_eq!(report.created, 7, "{report:?}");
        assert_eq!(report.reparented, 0);

        let all = tasks.list(&TaskFilter { project_id: Some(project.id), include_done: true, ..Default::default() }).unwrap();
        // The second plan file has its own "Task 1: Set up the widget" heading too
        // (see `children_across_two_plan_files_attach_to_their_own_files_parent`);
        // scope to the first file's parent here.
        let parent = all
            .iter()
            .find(|t| t.title == "Task 1: Set up the widget" && t.source_ref.as_ref().is_some_and(|s| s.path.ends_with("2026-01-01-example.md")))
            .expect("parent task created");
        assert!(parent.parent_id.is_none(), "a parent task has no parent of its own");

        let children: Vec<_> = all.iter().filter(|t| t.parent_id == Some(parent.id)).collect();
        assert_eq!(children.len(), 2, "{all:?}");
        assert!(children.iter().any(|t| t.title == "Step 1: Write the widget module"), "the child's title is cleaned: {children:?}");

        // Task 2's single checkbox is checked, so its parent lands in a done stage.
        let task_2 = all.iter().find(|t| t.title == "Task 2: Wire the widget in").unwrap();
        let stages = tasks.effective_stages(Some(project.id)).unwrap().stages;
        assert!(stages.iter().find(|s| s.name == task_2.stage).unwrap().done, "{task_2:?}");
    }

    fn write_plan(dir: &std::path::Path, name: &str, body: &str) {
        let plans = dir.join("docs/superpowers/plans");
        std::fs::create_dir_all(&plans).unwrap();
        std::fs::write(plans.join(name), body).unwrap();
    }

    const FINISHED_PLAN: &str = "# Finished\n\n### Task 9: Ship it\n\n- [ ] Step 1: Do it\n- [ ] Step 2: Test it\n";
    const CLOSING_LEDGER: &str = "# SDD ledger — plan: docs/superpowers/plans/2026-01-05-finished.md\n\n## Progress\nPhase 9 complete at abc1234; fast-forwarding main.\n";

    fn finished_plan_tasks(tasks: &TaskRepo, project: &Project) -> Vec<Task> {
        let all = tasks.list(&TaskFilter { project_id: Some(project.id), include_done: true, ..Default::default() }).unwrap();
        let mut mine: Vec<Task> = all.into_iter().filter(|t| t.source_ref.as_ref().is_some_and(|s| s.path.ends_with("2026-01-05-finished.md"))).collect();
        mine.sort_by_key(|t| t.seq);
        assert_eq!(mine.len(), 3, "{mine:?}");
        mine
    }

    /// A plan whose ledger closes its phase imports straight into the done stage,
    /// parent and steps alike, even though none of its checkboxes is ticked.
    #[test]
    fn a_plan_whose_ledger_marks_it_complete_imports_as_done() {
        let (db, tasks) = setup();
        let memories = MemoryRepo::new(&db);
        let (project, dir) = project_for_fixture(&db, "superpowers");
        write_plan(dir.path(), "2026-01-05-finished.md", FINISHED_PLAN);
        write_plan(dir.path(), "2026-01-05-finished.ledger.md", CLOSING_LEDGER);

        let report = import_tasks(&tasks, &memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        assert_eq!(report.created, 10, "the fixture's seven plus the finished plan's three: {report:?}");

        let stages = tasks.effective_stages(Some(project.id)).unwrap().stages;
        let done = stages.iter().find(|s| s.done).unwrap();
        for task in finished_plan_tasks(&tasks, &project) {
            assert_eq!(task.stage, done.name, "{task:?}");
            assert!(task.closed_at.is_some(), "{task:?}");
        }
        // The fixture's own ledger names no plan and closes nothing, so Task 1 of the
        // example plan still lands in the first stage.
        let all = tasks.list(&TaskFilter { project_id: Some(project.id), include_done: true, ..Default::default() }).unwrap();
        let task_1 = all.iter().find(|t| t.title == "Task 1: Set up the widget").unwrap();
        assert_eq!(task_1.stage, stages[0].name);
    }

    /// The ledger usually arrives after the plan was first imported: the re-import
    /// moves the tasks still in the first stage to done and counts them as updated,
    /// leaves the one a human had moved to another stage alone, and a third import
    /// skips them all.
    #[test]
    fn a_ledger_written_after_the_first_import_moves_its_open_tasks_to_done() {
        let (db, tasks) = setup();
        let memories = MemoryRepo::new(&db);
        let (project, dir) = project_for_fixture(&db, "superpowers");
        write_plan(dir.path(), "2026-01-05-finished.md", FINISHED_PLAN);

        import_tasks(&tasks, &memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        let stages = tasks.effective_stages(Some(project.id)).unwrap().stages;
        let done = stages.iter().find(|s| s.done).unwrap();
        for task in finished_plan_tasks(&tasks, &project) {
            assert_eq!(task.stage, stages[0].name, "{task:?}");
        }

        // A human takes one step in hand before the ledger closes.
        let in_hand = stages.iter().find(|s| !s.done && s.name != stages[0].name).expect("a middle stage").name.clone();
        let step_2 = finished_plan_tasks(&tasks, &project).into_iter().find(|t| t.title == "Step 2: Test it").unwrap();
        tasks.move_stage(&step_2.key, &in_hand, None, "human").unwrap();

        write_plan(dir.path(), "2026-01-05-finished.ledger.md", CLOSING_LEDGER);
        let second = import_tasks(&tasks, &memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        assert_eq!((second.created, second.updated, second.skipped), (0, 2, 8), "{second:?}");
        for task in finished_plan_tasks(&tasks, &project) {
            let moved_by = tasks.get(&task.key).unwrap().events.into_iter().filter(|e| e.kind == "moved").map(|e| e.actor).collect::<Vec<_>>();
            if task.key == step_2.key {
                assert_eq!(task.stage, in_hand, "the board owns a task a human moved: {task:?}");
                assert_eq!(moved_by, vec!["human".to_string()]);
            } else {
                assert_eq!(task.stage, done.name, "{task:?}");
                assert_eq!(moved_by, vec!["import/superpowers".to_string()], "{task:?}");
            }
        }

        let third = import_tasks(&tasks, &memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        assert_eq!((third.created, third.updated, third.skipped), (0, 0, 10), "{third:?}");
    }

    /// The fixture's second plan file repeats the first file's "Task 1: Set up the
    /// widget" heading verbatim, so this is the regression test for the anchor map
    /// being scoped by `(path, anchor)`: without that scoping, the second file's
    /// checkbox would resolve its parent from the first file's map entry and land
    /// as a subtask of the wrong plan's task.
    #[test]
    fn children_across_two_plan_files_attach_to_their_own_files_parent() {
        let (db, tasks) = setup();
        let memories = MemoryRepo::new(&db);
        let (project, _dir) = project_for_fixture(&db, "superpowers");

        import_tasks(&tasks, &memories, &project, FrameworkKind::Superpowers, "test").unwrap();

        let all = tasks.list(&TaskFilter { project_id: Some(project.id), include_done: true, ..Default::default() }).unwrap();
        let parents: Vec<_> = all.iter().filter(|t| t.title == "Task 1: Set up the widget").collect();
        assert_eq!(parents.len(), 2, "each plan file gets its own parent task: {all:?}");
        let (path_of, path_of_first) = (
            |t: &Task| t.source_ref.as_ref().map(|s| s.path.clone()).unwrap(),
            parents[0].source_ref.as_ref().map(|s| s.path.clone()).unwrap(),
        );
        assert_ne!(path_of_first, path_of(parents[1]), "the two parents come from different files: {parents:?}");

        for parent in &parents {
            let parent_path = path_of(parent);
            let children: Vec<_> = all.iter().filter(|t| t.parent_id == Some(parent.id)).collect();
            assert!(!children.is_empty(), "{parent:?}");
            for child in &children {
                assert_eq!(path_of(child), parent_path, "a child must attach to its own file's parent, not the other file's: {child:?}");
            }
        }
    }

    /// A task an earlier, parent-less importer created for a checkbox, titled with
    /// the raw checkbox markdown, must be picked up by a re-import: reparented under
    /// the new parent task and renamed to the cleaned title, both counted.
    #[test]
    fn reimporting_over_tasks_created_flat_reparents_and_renames_them() {
        let (db, tasks) = setup();
        let memories = MemoryRepo::new(&db);
        let (project, _dir) = project_for_fixture(&db, "superpowers");

        let flat = NewTask {
            project_id: Some(project.id),
            title: "**Step 1: Write the widget module**".to_string(),
            description: Some("Task 1: Set up the widget".to_string()),
            source_ref: Some(SourceRef {
                framework: FrameworkKind::Superpowers,
                path: "docs/superpowers/plans/2026-01-01-example.md".to_string(),
                anchor: "Task 1: Set up the widget#1".to_string(),
            }),
            ..Default::default()
        };
        let created = tasks.create(&flat, "test").unwrap();
        assert!(created.parent_id.is_none());

        let report = import_tasks(&tasks, &memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        assert_eq!(report.reparented, 1, "{report:?}");
        assert_eq!(report.updated, 1, "the title changed too: {report:?}");
        assert_eq!(report.created, 6, "everything but the pre-existing child: {report:?}");

        let again = tasks.get(&created.key).unwrap().task;
        assert_eq!(again.title, "Step 1: Write the widget module");
        let parent = tasks
            .list(&TaskFilter { project_id: Some(project.id), include_done: true, ..Default::default() })
            .unwrap()
            .into_iter()
            .find(|t| t.title == "Task 1: Set up the widget" && t.source_ref.as_ref().is_some_and(|s| s.path.ends_with("2026-01-01-example.md")))
            .expect("parent created");
        assert_eq!(again.parent_id, Some(parent.id));
    }

    #[test]
    fn importing_decisions_lands_as_pending_and_is_deduplicated() {
        let (db, _tasks) = setup();
        let memories = MemoryRepo::new(&db);
        let (project, _dir) = project_for_fixture(&db, "superpowers");

        let first = import_decisions(&memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        assert!(first.created > 0, "the fixture should have importable decisions: {first:?}");

        let pending = memories.list_by_status_scoped(MemoryStatus::Pending, None, Some(project.id), MemoryScopeFilter::ProjectOnly, MemoryPage::default()).unwrap();
        assert_eq!(pending.len(), first.created);
        assert!(pending.iter().all(|m| m.kind == MemoryKind::Decision));
        assert!(pending.iter().all(|m| m.tags.contains(&"superpowers".to_string()) && m.tags.contains(&"import".to_string())));

        let second = import_decisions(&memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        assert_eq!(second.created, 0, "a re-import must not duplicate decisions: {second:?}");
        assert_eq!(second.skipped, first.created);
    }
}
