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
    let counts = repo.counts_by_stage(Some(p.id)).unwrap();
    assert_eq!(
        counts,
        vec![("Backlog".to_string(), 1), ("In Progress".to_string(), 0), ("Testing".to_string(), 1), ("Done".to_string(), 0)]
    );
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

    repo.move_stage(&child.key, "Done", None, "t").unwrap();
    let after = repo.get(&parent.key).unwrap().task;
    assert!(after.ready, "{:?}", after.blocked_reason);
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
