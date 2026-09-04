use super::*;
use crate::projects::detect::Detected;
use crate::projects::ProjectRepo;

fn repo() -> (Arc<Db>, TaskRepo) {
    let db = Arc::new(Db::open_in_memory().unwrap());
    let repo = TaskRepo::new(db.clone(), Arc::new(Mutex::new(())));
    (db, repo)
}

/// A project whose directory name becomes its board key: `/tmp/atlas` -> `ATL`.
fn project(db: &Db, root: &str) -> Project {
    ProjectRepo::new(db).upsert(&Detected { root: root.into(), remote: None }, None, "t").unwrap()
}

fn new_task(project_id: Option<Uuid>, title: &str) -> NewTask {
    NewTask { project_id, title: title.into(), ..Default::default() }
}

fn stage(name: &str, done: bool) -> Stage {
    Stage { name: name.into(), done }
}

fn events(db: &Db, task: Uuid) -> Vec<(String, String)> {
    db.with_conn(|c| {
        let mut st = c.prepare("select kind, actor from task_events where task_id = ? order by created_at, id")?;
        let rows = st.query_map(params![task.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
    })
    .unwrap()
}

// -- keys -------------------------------------------------------------------

#[test]
fn keys_count_up_per_project_and_global_tasks_use_atlas() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    assert_eq!(p.board_key.as_deref(), Some("ATL"));

    let a = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    let b = repo.create(&new_task(Some(p.id), "second"), "t").unwrap();
    assert_eq!(a.key, "ATL-1");
    assert_eq!(b.key, "ATL-2");
    assert_eq!(b.seq, 2);

    let g = repo.create(&new_task(None, "global"), "t").unwrap();
    assert_eq!(g.key, "ATLAS-1");
}

#[test]
fn two_projects_with_the_same_three_letters_get_a_numeric_suffix() {
    let (db, _repo) = repo();
    let a = project(&db, "/tmp/atlas");
    let b = project(&db, "/tmp/atlantic");
    assert_eq!(a.board_key.as_deref(), Some("ATL"));
    assert_eq!(b.board_key.as_deref(), Some("ATL2"));
}

#[test]
fn short_and_punctuated_names_are_padded_to_three_characters() {
    assert_eq!(board_key_base("atlas"), "ATL");
    assert_eq!(board_key_base("ab"), "ABX");
    assert_eq!(board_key_base("-/-"), "XXX");
    assert_eq!(board_key_base("my-app"), "MYA");
    assert_eq!(board_key_base("3d"), "3DX");
}

/// A project row written before migration 3 has no board key; the first task
/// created for it fills one in rather than failing.
#[test]
fn a_missing_board_key_is_backfilled_on_first_use() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    db.with_conn(|c| {
        c.execute("update projects set board_key = null where id = ?", params![p.id.to_string()])?;
        Ok(())
    })
    .unwrap();
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    assert_eq!(t.key, "ATL-1");
    assert_eq!(ProjectRepo::new(&db).get(p.id).unwrap().board_key.as_deref(), Some("ATL"));
}

// -- stages -----------------------------------------------------------------

#[test]
fn a_new_task_lands_in_the_first_stage() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    assert_eq!(t.stage, "Backlog");
    assert!(t.closed_at.is_none());
    assert_eq!(t.kind, TaskKind::Task);
    assert_eq!(t.priority, TaskPriority::Medium);
}

#[test]
fn moving_to_an_unknown_stage_names_the_valid_ones() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    let err = repo.move_stage(&t.key, "Nowhere", None, "t").unwrap_err();
    assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("Backlog") && msg.contains("Done"), "{msg}");
}

#[test]
fn a_done_stage_stamps_closed_at_and_leaving_one_clears_it() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    let done = repo.move_stage(&t.key, "Done", None, "t").unwrap();
    assert_eq!(done.stage, "Done");
    assert!(done.closed_at.is_some());
    let back = repo.move_stage(&t.key, "Testing", None, "t").unwrap();
    assert!(back.closed_at.is_none(), "moving out of a done stage clears closed_at");
}

#[test]
fn counts_by_stage_covers_every_column_in_board_order() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    let b = repo.create(&new_task(Some(p.id), "b"), "t").unwrap();
    repo.move_stage(&b.key, "Testing", None, "t").unwrap();
    let counts = repo.counts_by_stage(Some(p.id), false, None).unwrap();
    assert_eq!(
        counts,
        vec![("Backlog".to_string(), 1), ("In Progress".to_string(), 0), ("Testing".to_string(), 1), ("Done".to_string(), 0)]
    );
}

/// `counts_by_stage` follows the same three-way scoping `list` does: neither
/// `project_id` nor `global_only` counts every project's tasks, `global_only`
/// narrows to just the project-less ones.
#[test]
fn counts_by_stage_with_neither_scope_counts_every_project() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    repo.create(&new_task(Some(p.id), "in a project"), "t").unwrap();
    repo.create(&new_task(None, "no project"), "t").unwrap();

    let every = repo.counts_by_stage(None, false, None).unwrap();
    let backlog = every.iter().find(|(s, _)| s == "Backlog").unwrap().1;
    assert_eq!(backlog, 2, "{every:?}");

    let global = repo.counts_by_stage(None, true, None).unwrap();
    let backlog = global.iter().find(|(s, _)| s == "Backlog").unwrap().1;
    assert_eq!(backlog, 1, "{global:?}");
}

/// `top_level` narrows the count to parent-less tasks (`Some(true)`) or subtasks
/// (`Some(false)`), the same `parent_id` rule `list`'s `top_level` filter applies.
#[test]
fn counts_by_stage_top_level_narrows_by_parent_id() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let parent = repo.create(&new_task(Some(p.id), "parent"), "t").unwrap();
    repo.create(&new_task(Some(p.id), "standalone"), "t").unwrap();
    repo.update(&parent.key, &TaskUpdate::default(), "t").unwrap(); // no-op, keeps parent stable
    let child = repo.create(&new_task(Some(p.id), "child"), "t").unwrap();
    repo.update(&child.key, &TaskUpdate { parent: Some(Some(parent.key.clone())), ..Default::default() }, "t").unwrap();

    let top = repo.counts_by_stage(Some(p.id), false, Some(true)).unwrap();
    let backlog = top.iter().find(|(s, _)| s == "Backlog").unwrap().1;
    assert_eq!(backlog, 2, "the parent and the standalone task, not the child: {top:?}");

    let subtasks = repo.counts_by_stage(Some(p.id), false, Some(false)).unwrap();
    let backlog = subtasks.iter().find(|(s, _)| s == "Backlog").unwrap().1;
    assert_eq!(backlog, 1, "just the child: {subtasks:?}");
}

// -- blockers, subtasks, ready ---------------------------------------------

#[test]
fn a_blocker_cycle_is_refused() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let a = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    let b = repo.create(&new_task(Some(p.id), "b"), "t").unwrap();
    repo.set_blockers(&a.key, vec![b.key.clone()], "t").unwrap();
    let err = repo.set_blockers(&b.key, vec![a.key.clone()], "t").unwrap_err();
    assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
    assert!(err.to_string().contains("cycle"), "{err}");
    // The refused call left the graph alone.
    assert!(repo.get(&b.key).unwrap().task.blocked_by.is_empty());
    // A task cannot block itself either.
    assert!(repo.set_blockers(&a.key, vec![a.key.clone()], "t").is_err());
}

#[test]
fn a_task_with_an_open_blocker_is_not_ready_until_the_blocker_is_done() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let a = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    let b = repo.create(&new_task(Some(p.id), "b"), "t").unwrap();
    let a = repo.set_blockers(&a.key, vec![b.key.clone()], "t").unwrap();
    assert_eq!(a.blocked_by, vec![b.key.clone()]);
    assert!(!a.ready);
    assert_eq!(a.blocked_reason.as_deref(), Some(format!("blocked by {}", b.key).as_str()));

    let ready: Vec<String> = repo
        .list(&TaskFilter { ready: true, ..Default::default() })
        .unwrap()
        .into_iter()
        .map(|t| t.key)
        .collect();
    assert_eq!(ready, vec![b.key.clone()]);

    repo.move_stage(&b.key, "Done", None, "t").unwrap();
    let a = repo.get(&a.key).unwrap().task;
    assert!(a.ready, "{:?}", a.blocked_reason);
    assert!(a.blocked_reason.is_none());
    let ready: Vec<String> = repo
        .list(&TaskFilter { ready: true, ..Default::default() })
        .unwrap()
        .into_iter()
        .map(|t| t.key)
        .collect();
    assert_eq!(ready, vec![a.key.clone()], "the done blocker drops out of the ready list");
}

/// `open_blockers` counts what the ready rule counts. A blocker in a done stage is
/// still in `blocked_by`, which is the full link list, but drops out of the count a
/// board card draws its badge from, so the badge and `ready` never disagree.
#[test]
fn open_blockers_leaves_out_a_blocker_that_is_done() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let a = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    let done = repo.create(&new_task(Some(p.id), "done blocker"), "t").unwrap();
    let open = repo.create(&new_task(Some(p.id), "open blocker"), "t").unwrap();
    let a = repo.set_blockers(&a.key, vec![done.key.clone(), open.key.clone()], "t").unwrap();
    assert_eq!(a.open_blockers, 2);

    repo.move_stage(&done.key, "Done", None, "t").unwrap();
    let a = repo.get(&a.key).unwrap().task;
    assert_eq!(a.blocked_by.len(), 2, "both links are still on the task");
    assert_eq!(a.open_blockers, 1);
    assert!(!a.ready);
    assert_eq!(a.blocked_reason.as_deref(), Some(format!("blocked by {}", open.key).as_str()));

    repo.move_stage(&open.key, "Done", None, "t").unwrap();
    let a = repo.get(&a.key).unwrap().task;
    assert_eq!(a.open_blockers, 0);
    assert!(a.ready, "no open blocker left, so no badge and no reason");
}

/// The claim holder is compared the way the assignee filter matches it, so the same
/// agent under a different capitalisation is not a conflict, and a different one
/// still is.
#[test]
fn claim_matches_the_holder_case_insensitively() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    repo.claim(&t.key, false, "codex").unwrap();
    let again = repo.claim(&t.key, false, "Codex").unwrap();
    assert_eq!(again.assignee.as_deref(), Some("Codex"));
    assert!(matches!(repo.claim(&t.key, false, "claude").unwrap_err(), AtlasError::Conflict(_)));
}

/// Clearing the assignee is an `assigned` event whose body reads forwards.
#[test]
fn clearing_the_assignee_records_an_unassigned_body() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    repo.update(&t.key, &TaskUpdate { assignee: Some(Some("codex".into())), ..Default::default() }, "t").unwrap();
    repo.update(&t.key, &TaskUpdate { assignee: Some(None), ..Default::default() }, "t").unwrap();

    let bodies: Vec<String> = repo
        .get(&t.key)
        .unwrap()
        .events
        .into_iter()
        .filter(|e| e.kind == "assigned")
        .map(|e| e.body)
        .collect();
    assert_eq!(bodies, vec![format!("assigned {}", t.key), format!("unassigned {}", t.key)]);
}

/// Moving a task to the stage it is already in, spelled differently, is a no-op:
/// `find_stage` matches case-insensitively, so the no-op check does too.
#[test]
fn moving_to_the_same_stage_under_another_spelling_writes_no_event() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    let before = repo.get(&t.key).unwrap().events.len();
    let moved = repo.move_stage(&t.key, " backlog ", None, "t").unwrap();
    assert_eq!(moved.stage, "Backlog");
    assert_eq!(repo.get(&t.key).unwrap().events.len(), before, "no moved event for a stage the task is in");
}

#[test]
fn a_parent_with_an_open_child_is_not_ready_and_says_why() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let parent = repo.create(&new_task(Some(p.id), "parent"), "t").unwrap();
    let child = repo
        .create(&NewTask { project_id: Some(p.id), title: "child".into(), parent: Some(parent.key.clone()), ..Default::default() }, "t")
        .unwrap();

    let detail = repo.get(&parent.key).unwrap();
    assert!(!detail.task.ready);
    let reason = detail.task.blocked_reason.clone().unwrap();
    assert!(reason.contains("open subtask") && reason.contains(&child.key), "{reason}");
    assert_eq!(detail.children.len(), 1);
    // The child is ready on its own terms.
    assert!(detail.children[0].ready);
    assert_eq!(detail.task.subtasks_total, 1);
    assert_eq!(detail.task.subtasks_done, 0);

    repo.move_stage(&child.key, "Done", None, "t").unwrap();
    let after = repo.get(&parent.key).unwrap().task;
    assert!(after.ready, "{:?}", after.blocked_reason);
    assert_eq!(after.subtasks_total, 1);
    assert_eq!(after.subtasks_done, 1, "the child moved to a done stage");
}

/// `subtasks_total`/`subtasks_done` count only direct children, are 0 for a task
/// with none, and count a done child even while another sibling is still open.
#[test]
fn subtasks_total_and_done_count_direct_children_in_a_done_stage() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let parent = repo.create(&new_task(Some(p.id), "parent"), "t").unwrap();
    assert_eq!(parent.subtasks_total, 0);
    assert_eq!(parent.subtasks_done, 0);

    let a = repo.create(&NewTask { project_id: Some(p.id), title: "a".into(), parent: Some(parent.key.clone()), ..Default::default() }, "t").unwrap();
    let b = repo.create(&NewTask { project_id: Some(p.id), title: "b".into(), parent: Some(parent.key.clone()), ..Default::default() }, "t").unwrap();
    repo.move_stage(&a.key, "Done", None, "t").unwrap();

    let parent = repo.get(&parent.key).unwrap().task;
    assert_eq!(parent.subtasks_total, 2);
    assert_eq!(parent.subtasks_done, 1);

    // The child itself has no subtasks of its own.
    let a = repo.get(&a.key).unwrap().task;
    assert_eq!(a.subtasks_total, 0);
    assert_eq!(a.subtasks_done, 0);
    let _ = b;
}

/// `list`'s `top_level` filter keeps only parent-less tasks (`Some(true)`) or only
/// subtasks (`Some(false)`); `None` (the default) applies no filter either way.
#[test]
fn list_top_level_filter_keeps_parents_or_subtasks() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let parent = repo.create(&new_task(Some(p.id), "parent"), "t").unwrap();
    let child = repo.create(&NewTask { project_id: Some(p.id), title: "child".into(), parent: Some(parent.key.clone()), ..Default::default() }, "t").unwrap();

    let keys = |f: TaskFilter| -> Vec<String> { repo.list(&f).unwrap().into_iter().map(|t| t.key).collect() };

    let top = keys(TaskFilter { project_id: Some(p.id), top_level: Some(true), ..Default::default() });
    assert_eq!(top, vec![parent.key.clone()], "{top:?}");

    let subtasks = keys(TaskFilter { project_id: Some(p.id), top_level: Some(false), ..Default::default() });
    assert_eq!(subtasks, vec![child.key.clone()], "{subtasks:?}");

    let mut both = keys(TaskFilter { project_id: Some(p.id), ..Default::default() });
    both.sort();
    let mut expected = vec![parent.key, child.key];
    expected.sort();
    assert_eq!(both, expected, "top_level left unset filters neither out");
}

// -- claim ------------------------------------------------------------------

#[test]
fn claim_takes_an_unassigned_task_and_starts_it() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    let claimed = repo.claim(&t.key, false, "codex").unwrap();
    assert_eq!(claimed.assignee.as_deref(), Some("codex"));
    assert_eq!(claimed.stage, "In Progress");
    let kinds: Vec<String> = events(&db, t.id).into_iter().map(|(k, _)| k).collect();
    assert_eq!(kinds, vec!["created", "assigned", "moved"]);
}

#[test]
fn claim_on_someone_elses_task_needs_force() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    repo.claim(&t.key, false, "codex").unwrap();
    let err = repo.claim(&t.key, false, "claude-code").unwrap_err();
    assert!(matches!(err, AtlasError::Conflict(_)), "{err}");
    let taken = repo.claim(&t.key, true, "claude-code").unwrap();
    assert_eq!(taken.assignee.as_deref(), Some("claude-code"));
    // The second claim does not move it again; it was already out of the first stage.
    assert_eq!(taken.stage, "In Progress");
}

// -- conflicts and events ---------------------------------------------------

#[test]
fn a_stale_expected_updated_at_is_a_conflict() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    let stale = t.updated_at - chrono::Duration::seconds(1);
    let patch = TaskUpdate { title: Some("renamed".into()), expected_updated_at: Some(stale), ..Default::default() };
    let err = repo.update(&t.key, &patch, "t").unwrap_err();
    assert!(matches!(err, AtlasError::Conflict(_)), "{err}");
    assert!(matches!(repo.move_stage(&t.key, "Testing", Some(stale), "t"), Err(AtlasError::Conflict(_))));

    // The value the caller actually read is accepted.
    let patch = TaskUpdate { title: Some("renamed".into()), expected_updated_at: Some(t.updated_at), ..Default::default() };
    assert_eq!(repo.update(&t.key, &patch, "t").unwrap().title, "renamed");
}

#[test]
fn every_write_appends_an_event_carrying_the_actor() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let other = repo.create(&new_task(Some(p.id), "blocker"), "t").unwrap();
    let t = repo.create(&new_task(Some(p.id), "first"), "maker").unwrap();

    repo.update(&t.key, &TaskUpdate { title: Some("renamed".into()), ..Default::default() }, "editor").unwrap();
    repo.update(&t.key, &TaskUpdate { assignee: Some(Some("ann".into())), ..Default::default() }, "editor").unwrap();
    repo.move_stage(&t.key, "Testing", None, "mover").unwrap();
    repo.comment(&t.key, "looks good", "reviewer").unwrap();
    repo.set_blockers(&t.key, vec![other.key.clone()], "linker").unwrap();
    repo.set_blockers(&t.key, vec![], "linker").unwrap();
    repo.claim(&t.key, true, "codex").unwrap();
    repo.delete(&t.key, "remover").unwrap();

    let got = events(&db, t.id);
    let kinds: Vec<&str> = got.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds, vec!["created", "edited", "assigned", "moved", "commented", "blocked", "unblocked", "assigned", "deleted"]);
    let actors: Vec<&str> = got.iter().map(|(_, a)| a.as_str()).collect();
    assert_eq!(actors, vec!["maker", "editor", "editor", "mover", "reviewer", "linker", "linker", "codex", "remover"]);

    // Deletion is also an audit row, like every other hard delete in Atlas.
    let audits: i64 = db
        .with_conn(|c| Ok(c.query_row("select count(*) from audit where action = 'task_delete'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(audits, 1);
}

#[test]
fn a_comment_comes_back_as_an_event() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    let e = repo.comment(&t.key, "  looks good  ", "reviewer").unwrap();
    assert_eq!(e.kind, "commented");
    assert_eq!(e.body, "looks good");
    assert_eq!(e.actor, "reviewer");
    assert!(repo.comment(&t.key, "   ", "reviewer").is_err());
    assert_eq!(repo.get(&t.key).unwrap().events.len(), 2);
}

// -- update and delete ------------------------------------------------------

#[test]
fn an_explicit_null_clears_the_assignee_and_the_parent() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let parent = repo.create(&new_task(Some(p.id), "parent"), "t").unwrap();
    let t = repo
        .create(
            &NewTask {
                project_id: Some(p.id),
                title: "child".into(),
                assignee: Some("ann".into()),
                parent: Some(parent.key.clone()),
                ..Default::default()
            },
            "t",
        )
        .unwrap();
    assert_eq!(t.parent_id, Some(parent.id));

    // Absent leaves both alone.
    let same = repo.update(&t.key, &TaskUpdate { title: Some("child 2".into()), ..Default::default() }, "t").unwrap();
    assert_eq!(same.assignee.as_deref(), Some("ann"));
    assert_eq!(same.parent_id, Some(parent.id));

    let cleared = repo
        .update(&t.key, &TaskUpdate { assignee: Some(None), parent: Some(None), ..Default::default() }, "t")
        .unwrap();
    assert!(cleared.assignee.is_none());
    assert!(cleared.parent_id.is_none());
}

#[test]
fn delete_detaches_children_and_removes_blocker_links() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let target = repo.create(&new_task(Some(p.id), "target"), "t").unwrap();
    let child = repo
        .create(&NewTask { project_id: Some(p.id), title: "child".into(), parent: Some(target.key.clone()), ..Default::default() }, "t")
        .unwrap();
    let waiter = repo
        .create(&NewTask { project_id: Some(p.id), title: "waiter".into(), blocked_by: Some(vec![target.key.clone()]), ..Default::default() }, "t")
        .unwrap();
    repo.set_blockers(&target.key, vec![child.key.clone()], "t").unwrap();

    repo.delete(&target.key, "t").unwrap();

    assert!(matches!(repo.get(&target.key), Err(AtlasError::NotFound(_))));
    assert!(repo.get(&child.key).unwrap().task.parent_id.is_none(), "children are detached, not deleted");
    let waiter = repo.get(&waiter.key).unwrap().task;
    assert!(waiter.blocked_by.is_empty(), "links pointing at the deleted task are gone");
    assert!(waiter.ready);
    let links: i64 = db.with_conn(|c| Ok(c.query_row("select count(*) from task_blockers", [], |r| r.get(0))?)).unwrap();
    assert_eq!(links, 0);
}

// -- listing ----------------------------------------------------------------

#[test]
fn list_filters_by_project_stage_assignee_text_and_done() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let q = project(&db, "/tmp/other");
    let a = repo
        .create(&NewTask { project_id: Some(p.id), title: "write the parser".into(), assignee: Some("ann".into()), ..Default::default() }, "t")
        .unwrap();
    let b = repo
        .create(&NewTask { project_id: Some(p.id), title: "ship it".into(), description: Some("needs the parser".into()), ..Default::default() }, "t")
        .unwrap();
    repo.create(&new_task(Some(q.id), "elsewhere"), "t").unwrap();

    let keys = |f: TaskFilter| repo.list(&f).unwrap().into_iter().map(|t| t.key).collect::<Vec<_>>();

    assert_eq!(keys(TaskFilter { project_id: Some(p.id), ..Default::default() }).len(), 2);
    assert_eq!(keys(TaskFilter { assignee: Some("ann".into()), ..Default::default() }), vec![a.key.clone()]);
    assert_eq!(keys(TaskFilter { stage: Some("backlog".into()), ..Default::default() }).len(), 3);
    // The query matches key, title and description.
    let mut hits = keys(TaskFilter { query: Some("PARSER".into()), ..Default::default() });
    hits.sort();
    let mut want = vec![a.key.clone(), b.key.clone()];
    want.sort();
    assert_eq!(hits, want);
    assert_eq!(keys(TaskFilter { query: Some(a.key.to_lowercase(), ), ..Default::default() }), vec![a.key.clone()]);

    repo.move_stage(&b.key, "Done", None, "t").unwrap();
    assert!(!keys(TaskFilter { project_id: Some(p.id), ..Default::default() }).contains(&b.key));
    assert!(keys(TaskFilter { project_id: Some(p.id), include_done: true, ..Default::default() }).contains(&b.key));
}

/// `global_only` is the literal global board: tasks with no project at all, not a
/// bare `project_id: None`, which leaves every project's tasks in.
#[test]
fn list_global_only_keeps_just_the_project_less_tasks() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let g = repo.create(&new_task(None, "no project"), "t").unwrap();
    repo.create(&new_task(Some(p.id), "has a project"), "t").unwrap();

    let keys = |f: TaskFilter| repo.list(&f).unwrap().into_iter().map(|t| t.key).collect::<Vec<_>>();
    assert_eq!(keys(TaskFilter { global_only: true, ..Default::default() }), vec![g.key.clone()]);
    assert_eq!(keys(TaskFilter::default()).len(), 2, "no filter still shows every task");
}

// -- stage administration ---------------------------------------------------

#[test]
fn a_project_override_replaces_the_effective_list_and_clearing_it_restores_the_global_one() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    assert!(!repo.effective_stages(Some(p.id)).unwrap().overridden);

    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    let renames = HashMap::from([("Backlog".to_string(), "Design".to_string())]);
    let list = repo
        .set_project_stages(p.id, Some(vec![stage("Design", false), stage("Shipped", true)]), &renames, "t")
        .unwrap();
    assert!(list.overridden);
    assert_eq!(list.stages, vec![stage("Design", false), stage("Shipped", true)]);
    assert_eq!(repo.get(&t.key).unwrap().task.stage, "Design");
    // The global list is untouched.
    assert_eq!(repo.effective_stages(None).unwrap().stages, default_stages());

    let back = HashMap::from([("Design".to_string(), "Backlog".to_string())]);
    let list = repo.set_project_stages(p.id, None, &back, "t").unwrap();
    assert!(!list.overridden);
    assert_eq!(list.stages, default_stages());
    assert_eq!(repo.get(&t.key).unwrap().task.stage, "Backlog");
}

#[test]
fn removing_a_stage_that_still_holds_tasks_is_refused_with_the_count() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    repo.create(&new_task(Some(p.id), "b"), "t").unwrap();

    let err = repo
        .set_global_stages(vec![stage("In Progress", false), stage("Done", true)], &HashMap::new(), "t")
        .unwrap_err();
    assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("Backlog") && msg.contains("2 tasks"), "{msg}");
    // Nothing was written.
    assert_eq!(repo.effective_stages(None).unwrap().stages, default_stages());
}

#[test]
fn a_rename_carries_the_tasks_and_records_the_move() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    let g = repo.create(&new_task(None, "global"), "t").unwrap();

    let renames = HashMap::from([("Backlog".to_string(), "Todo".to_string())]);
    let stages = vec![stage("Todo", false), stage("In Progress", false), stage("Testing", false), stage("Done", true)];
    repo.set_global_stages(stages.clone(), &renames, "board-admin").unwrap();

    assert_eq!(repo.effective_stages(None).unwrap().stages, stages);
    assert_eq!(repo.get(&t.key).unwrap().task.stage, "Todo");
    assert_eq!(repo.get(&g.key).unwrap().task.stage, "Todo", "global tasks follow the global list too");
    let kinds: Vec<String> = events(&db, t.id).into_iter().map(|(k, _)| k).collect();
    assert_eq!(kinds, vec!["created", "moved"]);
    // Moving to the new name works; the old one is gone.
    repo.move_stage(&t.key, "Todo", None, "t").unwrap();
    assert!(repo.move_stage(&t.key, "Backlog", None, "t").is_err());
}

#[test]
fn an_invalid_stage_list_never_reaches_the_settings_table() {
    let (db, repo) = repo();
    assert!(repo.set_global_stages(vec![stage("Only", true)], &HashMap::new(), "t").is_err());
    let stored: i64 = db
        .with_conn(|c| Ok(c.query_row("select count(*) from settings where key = 'board.stages'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(stored, 0);
}

#[test]
fn a_project_override_governs_its_own_tasks_only() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let q = project(&db, "/tmp/other");
    let renames = HashMap::from([("Backlog".to_string(), "Design".to_string())]);
    repo.set_project_stages(p.id, Some(vec![stage("Design", false), stage("Shipped", true)]), &renames, "t").unwrap();

    let a = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    assert_eq!(a.stage, "Design");
    let b = repo.create(&new_task(Some(q.id), "b"), "t").unwrap();
    assert_eq!(b.stage, "Backlog");
    // A global rename leaves the overriding project alone.
    let stages = vec![stage("Todo", false), stage("Done", true)];
    repo.set_global_stages(stages, &HashMap::from([("Backlog".to_string(), "Todo".to_string())]), "t").unwrap();
    assert_eq!(repo.get(&a.key).unwrap().task.stage, "Design");
    assert_eq!(repo.get(&b.key).unwrap().task.stage, "Todo");
}

// -- fix round 1 ------------------------------------------------------------

/// A key names one piece of work for good. Recycling the highest sequence number
/// after a delete would point old commit messages and memories at new work.
#[test]
fn a_deleted_key_is_never_issued_again() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    for _ in 0..3 {
        repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    }
    repo.delete("ATL-3", "t").unwrap();
    assert_eq!(repo.create(&new_task(Some(p.id), "next"), "t").unwrap().key, "ATL-4");
    // Deleting from the middle does not shuffle the run either.
    repo.delete("ATL-2", "t").unwrap();
    assert_eq!(repo.create(&new_task(Some(p.id), "later"), "t").unwrap().key, "ATL-5");
    // The global board keeps its own counter.
    repo.create(&new_task(None, "g"), "t").unwrap();
    repo.delete("ATLAS-1", "t").unwrap();
    assert_eq!(repo.create(&new_task(None, "g2"), "t").unwrap().key, "ATLAS-2");
}

/// A board written before `board_counters` existed carries on from its live rows.
#[test]
fn a_missing_counter_row_is_seeded_from_the_live_tasks() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    repo.create(&new_task(Some(p.id), "b"), "t").unwrap();
    db.with_conn(|c| {
        c.execute("delete from board_counters", [])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(repo.create(&new_task(Some(p.id), "c"), "t").unwrap().key, "ATL-3");
}

/// A chained map is applied once against each task's original stage, so a task in
/// `Testing` lands in `Done` and stops there rather than falling on to `Archive`.
#[test]
fn a_chained_rename_moves_each_task_exactly_one_hop() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let backlog = repo.create(&new_task(Some(p.id), "still queued"), "t").unwrap();
    let testing = repo.create(&new_task(Some(p.id), "under test"), "t").unwrap();
    repo.move_stage(&testing.key, "Testing", None, "t").unwrap();
    let done = repo.create(&new_task(Some(p.id), "finished"), "t").unwrap();
    repo.move_stage(&done.key, "Done", None, "t").unwrap();

    let renames = HashMap::from([("Testing".to_string(), "Done".to_string()), ("Done".to_string(), "Archive".to_string())]);
    let stages = vec![stage("Backlog", false), stage("Done", false), stage("Archive", true)];
    repo.set_global_stages(stages, &renames, "t").unwrap();

    assert_eq!(repo.get(&backlog.key).unwrap().task.stage, "Backlog");
    assert_eq!(repo.get(&testing.key).unwrap().task.stage, "Done", "one hop, not two");
    assert_eq!(repo.get(&done.key).unwrap().task.stage, "Archive");
}

/// A swap exchanges two columns instead of collapsing both into one.
#[test]
fn a_swapped_rename_exchanges_the_two_columns() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let a = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    let b = repo.create(&new_task(Some(p.id), "b"), "t").unwrap();
    repo.move_stage(&b.key, "Testing", None, "t").unwrap();

    let renames = HashMap::from([("Backlog".to_string(), "Testing".to_string()), ("Testing".to_string(), "Backlog".to_string())]);
    repo.set_global_stages(default_stages(), &renames, "t").unwrap();

    assert_eq!(repo.get(&a.key).unwrap().task.stage, "Testing");
    assert_eq!(repo.get(&b.key).unwrap().task.stage, "Backlog");
}

#[test]
fn a_rename_map_that_cannot_be_applied_in_one_pass_is_refused() {
    let (db, repo) = repo();
    let _p = project(&db, "/tmp/atlas");
    // The same source twice, differing only in case: which one wins would depend on
    // HashMap order.
    let twice = HashMap::from([("Backlog".to_string(), "Todo".to_string()), ("backlog".to_string(), "Later".to_string())]);
    let stages = vec![stage("Todo", false), stage("Later", false), stage("Done", true)];
    let err = repo.set_global_stages(stages, &twice, "t").unwrap_err();
    assert!(err.to_string().contains("renamed twice"), "{err}");

    // A target that is not on the new board would park tasks off the board.
    let stray = HashMap::from([("Backlog".to_string(), "Nowhere".to_string())]);
    let err = repo.set_global_stages(default_stages(), &stray, "t").unwrap_err();
    assert!(err.to_string().contains("unknown stage 'Nowhere'"), "{err}");
    assert_eq!(repo.effective_stages(None).unwrap().stages, default_stages());
}

/// A JavaScript `Date` truncates to milliseconds, so a value the GUI read back and
/// sent straight on must still match.
#[test]
fn expected_updated_at_is_compared_at_millisecond_resolution() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();

    let truncated = DateTime::from_timestamp_millis(t.updated_at.timestamp_millis()).unwrap();
    assert_ne!(truncated, t.updated_at, "the fixture needs sub-millisecond precision to be a real test");
    let patch = TaskUpdate { title: Some("renamed".into()), expected_updated_at: Some(truncated), ..Default::default() };
    assert_eq!(repo.update(&t.key, &patch, "t").unwrap().title, "renamed");

    let stale = truncated - chrono::Duration::seconds(1);
    let patch = TaskUpdate { title: Some("again".into()), expected_updated_at: Some(stale), ..Default::default() };
    assert!(matches!(repo.update(&t.key, &patch, "t"), Err(AtlasError::Conflict(_))));
}

/// Deleting a project takes its board with it: an orphaned task would be judged
/// against the global stage list on read but skipped by a global rename.
#[test]
fn deleting_a_project_cascades_to_its_board() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let q = project(&db, "/tmp/other");
    let a = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    repo.create(&new_task(Some(p.id), "b"), "t").unwrap();
    repo.comment(&a.key, "note", "t").unwrap();
    let kept = repo.create(&new_task(Some(q.id), "kept"), "t").unwrap();
    let global = repo.create(&new_task(None, "global"), "t").unwrap();
    // A task in another project waits on one that is about to be cascaded away.
    repo.set_blockers(&kept.key, vec![a.key.clone()], "t").unwrap();

    ProjectRepo::new(&db).delete(p.id, "remover").unwrap();

    let count = |sql: &str| -> i64 { db.with_conn(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap() };
    assert_eq!(count("select count(*) from tasks where project_id is not null"), 1);
    assert_eq!(count("select count(*) from task_blockers"), 0, "links in both directions go");
    // kept: created + blocked; global: created. The cascaded board's events are gone.
    assert_eq!(count("select count(*) from task_events"), 3, "only the surviving tasks keep their history");
    assert_eq!(count("select count(*) from audit where action = 'task_delete_cascade'"), 1);

    // The survivors are untouched and no longer blocked by a row that is gone.
    let kept = repo.get(&kept.key).unwrap().task;
    assert!(kept.blocked_by.is_empty());
    assert!(kept.ready);
    assert_eq!(repo.get(&global.key).unwrap().task.stage, "Backlog");
    // A global rename now sees every task there is.
    repo.set_global_stages(
        vec![stage("Todo", false), stage("Done", true)],
        &HashMap::from([("Backlog".to_string(), "Todo".to_string())]),
        "t",
    )
    .unwrap();
    assert_eq!(repo.get(&global.key).unwrap().task.stage, "Todo");
}

#[test]
fn a_project_with_no_tasks_records_no_cascade() {
    let (db, _repo) = repo();
    let p = project(&db, "/tmp/atlas");
    ProjectRepo::new(&db).delete(p.id, "t").unwrap();
    let n: i64 = db
        .with_conn(|c| Ok(c.query_row("select count(*) from audit where action = 'task_delete_cascade'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 0);
}

/// A link pointing at a row that is gone must not wedge the waiting task as
/// never-ready, and the next write cleans it up.
#[test]
fn a_dangling_blocker_link_is_ignored_and_then_swept() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "waiter"), "t").unwrap();
    db.with_conn(|c| {
        c.execute(
            "insert into task_blockers (task_id, blocked_by) values (?, ?)",
            params![t.id.to_string(), Uuid::new_v4().to_string()],
        )?;
        Ok(())
    })
    .unwrap();

    let seen = repo.get(&t.key).unwrap().task;
    assert!(seen.blocked_by.is_empty());
    assert!(seen.ready, "a broken link must not hold a task open forever");

    repo.set_blockers(&t.key, vec![], "t").unwrap();
    let left: i64 = db.with_conn(|c| Ok(c.query_row("select count(*) from task_blockers", [], |r| r.get(0))?)).unwrap();
    assert_eq!(left, 0);
}

#[test]
fn a_transitive_cycle_is_refused() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let a = repo.create(&new_task(Some(p.id), "a"), "t").unwrap();
    let b = repo.create(&new_task(Some(p.id), "b"), "t").unwrap();
    let c = repo.create(&new_task(Some(p.id), "c"), "t").unwrap();
    repo.set_blockers(&a.key, vec![b.key.clone()], "t").unwrap();
    repo.set_blockers(&b.key, vec![c.key.clone()], "t").unwrap();
    // A -> B -> C, so C waiting on A closes the loop.
    let err = repo.set_blockers(&c.key, vec![a.key.clone()], "t").unwrap_err();
    assert!(err.to_string().contains("cycle"), "{err}");
    assert!(repo.get(&c.key).unwrap().task.blocked_by.is_empty());
}

/// Claim moves a task to the second stage of the list its own project uses, not
/// the second stage of the global one.
#[test]
fn claim_follows_a_project_override() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    repo.set_project_stages(
        p.id,
        Some(vec![stage("Ideas", false), stage("Building", false), stage("Shipped", true)]),
        &HashMap::from([("Backlog".to_string(), "Ideas".to_string())]),
        "t",
    )
    .unwrap();
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    assert_eq!(t.stage, "Ideas");
    assert_eq!(repo.claim(&t.key, false, "codex").unwrap().stage, "Building");

    // A global task still follows the global list.
    let g = repo.create(&new_task(None, "global"), "t").unwrap();
    assert_eq!(repo.claim(&g.key, false, "codex").unwrap().stage, "In Progress");
}

#[test]
fn list_orders_by_project_then_sequence() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let mut made = Vec::new();
    for i in 0..11 {
        made.push(repo.create(&new_task(Some(p.id), &format!("task {i}")), "t").unwrap().key);
    }
    let got: Vec<String> = repo.list(&TaskFilter { project_id: Some(p.id), ..Default::default() }).unwrap().into_iter().map(|t| t.key).collect();
    assert_eq!(got, made, "ATL-10 sorts after ATL-9, not between ATL-1 and ATL-2");
}

#[test]
fn the_assignee_filter_ignores_case() {
    let (db, repo) = repo();
    let p = project(&db, "/tmp/atlas");
    let t = repo.create(&new_task(Some(p.id), "first"), "t").unwrap();
    repo.claim(&t.key, false, "codex").unwrap();
    let hits = repo.list(&TaskFilter { assignee: Some("Codex".into()), ..Default::default() }).unwrap();
    assert_eq!(hits.len(), 1);
}

/// The settings route cannot touch the stage list at all: not a new list, which
/// would skip the removal checks and the renames, and not `null`, which would leave
/// every task in a stage the default board does not have. Going back to the default
/// board is the board route with the default list.
#[test]
fn the_settings_route_refuses_the_stage_list() {
    let (db, repo) = repo();
    let stages = vec![stage("Todo", false), stage("Done", true)];
    repo.set_global_stages(stages.clone(), &HashMap::new(), "t").unwrap();
    let p = project(&db, "/tmp/atlas");
    repo.create(&new_task(Some(p.id), "parked"), "t").unwrap();

    let settings = SettingsRepo::new(&db);
    for value in [serde_json::Value::Null, serde_json::json!([{"name": "A", "done": false}, {"name": "B", "done": true}])] {
        let values = serde_json::Map::from_iter([(STAGES_SETTING.to_string(), value)]);
        let err = settings.set_many(&values, "t").unwrap_err();
        assert!(
            matches!(&err, AtlasError::Invalid(m) if m == "set board stages through PUT /api/v1/board/stages"),
            "{err}"
        );
    }
    // Nothing was written, so the list the board wrote still stands.
    assert_eq!(repo.effective_stages(None).unwrap().stages, stages);

    // The board route is the way back to the default, and it still refuses to strand
    // the task sitting in `Todo`.
    let err = repo.set_global_stages(default_stages(), &HashMap::new(), "t").unwrap_err();
    assert!(matches!(&err, AtlasError::Invalid(m) if m.contains("Todo")), "{err}");
    let renames = HashMap::from([("Todo".to_string(), "Backlog".to_string())]);
    repo.set_global_stages(default_stages(), &renames, "t").unwrap();
    assert_eq!(repo.effective_stages(None).unwrap().stages, default_stages());
}
