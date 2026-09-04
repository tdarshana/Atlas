//! Imports a framework's tasks and decisions into the board and into memory.
//!
//! An imported task stays linked to the file it came from by `SourceRef`, so
//! running an import twice updates the tasks it already created rather than
//! filing duplicates, and never moves a task's stage back: the board owns
//! progress once a task exists. An imported decision becomes a pending memory,
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
/// match; its stage is never touched either way. An item whose `parent_anchor` names
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
    // `source_ref.anchor` to task id, filled in as each item's task is created or
    // matched below. A child's `parent_anchor` looks itself up here first.
    let mut anchor_ids: HashMap<String, Uuid> = HashMap::new();

    for item in items {
        let parent_id: Option<Uuid> = match &item.parent_anchor {
            None => None,
            Some(anchor) => match anchor_ids.get(anchor) {
                Some(id) => Some(*id),
                // The parent wasn't created earlier in *this* run (an import of just
                // the children, say, or a run order the adapter doesn't guarantee) —
                // fall back to the parent's own SourceRef, the same lookup a child
                // uses for itself.
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
                    if content_changed {
                        report.updated += 1;
                    }
                    if reparent {
                        report.reparented += 1;
                    }
                } else {
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
        anchor_ids.insert(item.source_ref.anchor.clone(), task_id);
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
        for m in memories.list_by_status_scoped(status, None, Some(project.id), MemoryScopeFilter::ProjectOnly)? {
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
        assert_eq!(report.created, 5, "two parents, two children under Task 1, one under Task 2: {report:?}");
        assert_eq!(report.reparented, 0);

        let all = tasks.list(&TaskFilter { project_id: Some(project.id), include_done: true, ..Default::default() }).unwrap();
        let parent = all.iter().find(|t| t.title == "Task 1: Set up the widget").expect("parent task created");
        assert!(parent.parent_id.is_none(), "a parent task has no parent of its own");

        let children: Vec<_> = all.iter().filter(|t| t.parent_id == Some(parent.id)).collect();
        assert_eq!(children.len(), 2, "{all:?}");
        assert!(children.iter().any(|t| t.title == "Step 1: Write the widget module"), "the child's title is cleaned: {children:?}");

        // Task 2's single checkbox is checked, so its parent lands in a done stage.
        let task_2 = all.iter().find(|t| t.title == "Task 2: Wire the widget in").unwrap();
        let stages = tasks.effective_stages(Some(project.id)).unwrap().stages;
        assert!(stages.iter().find(|s| s.name == task_2.stage).unwrap().done, "{task_2:?}");
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
        assert_eq!(report.created, 4, "everything but the pre-existing child: {report:?}");

        let again = tasks.get(&created.key).unwrap().task;
        assert_eq!(again.title, "Step 1: Write the widget module");
        let parent = tasks
            .list(&TaskFilter { project_id: Some(project.id), include_done: true, ..Default::default() })
            .unwrap()
            .into_iter()
            .find(|t| t.title == "Task 1: Set up the widget")
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

        let pending = memories.list_by_status_scoped(MemoryStatus::Pending, None, Some(project.id), MemoryScopeFilter::ProjectOnly).unwrap();
        assert_eq!(pending.len(), first.created);
        assert!(pending.iter().all(|m| m.kind == MemoryKind::Decision));
        assert!(pending.iter().all(|m| m.tags.contains(&"superpowers".to_string()) && m.tags.contains(&"import".to_string())));

        let second = import_decisions(&memories, &project, FrameworkKind::Superpowers, "test").unwrap();
        assert_eq!(second.created, 0, "a re-import must not duplicate decisions: {second:?}");
        assert_eq!(second.skipped, first.created);
    }
}
