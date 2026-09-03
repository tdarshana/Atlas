use super::migrate_docs::migrate_workflow_docs;
use super::*;
use crate::library::DocRepo;
use crate::models::{DocKind, NewDoc};
use crate::settings::SettingsRepo;
use chrono::TimeZone;

fn repo() -> (Arc<Db>, WorkflowRepo) {
    let db = Arc::new(Db::open_in_memory().unwrap());
    let repo = WorkflowRepo::new(db.clone(), Arc::new(Mutex::new(())));
    (db, repo)
}

fn at(x: f64, y: f64) -> Position {
    Position { x, y }
}

fn trigger_node(id: &str) -> Node {
    Node { id: id.into(), kind: NodeKind::Trigger, position: at(0.0, 0.0), data: NodeData::Trigger(Trigger::manual()) }
}

fn action_node(id: &str, name: &str, p: Position) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Action,
        position: p,
        data: NodeData::Action {
            name: name.into(),
            instructions: format!("do {name}"),
            agent: "desktop".into(),
            practices: vec![],
            memories: None,
        },
    }
}

fn output_node(id: &str) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Output,
        position: at(600.0, 0.0),
        data: NodeData::Output { propose_memories: false, file_tasks: false },
    }
}

fn edge(source: &str, target: &str) -> Edge {
    Edge { id: format!("{source}->{target}"), source: source.into(), target: target.into() }
}

/// trigger -> a -> b -> output.
fn linear() -> Graph {
    Graph {
        nodes: vec![trigger_node("t"), action_node("a", "first", at(200.0, 0.0)), action_node("b", "second", at(400.0, 0.0)), output_node("o")],
        edges: vec![edge("t", "a"), edge("a", "b"), edge("b", "o")],
    }
}

fn new_workflow(name: &str) -> NewWorkflow {
    NewWorkflow {
        name: name.into(),
        project_id: None,
        description: String::new(),
        trigger: Trigger::manual(),
        graph: linear(),
        enabled: true,
    }
}

fn invalid(g: &Graph) -> String {
    match graph::validate(g) {
        Err(AtlasError::Invalid(m)) => m,
        other => panic!("expected an Invalid error, got {other:?}"),
    }
}

// -- graph validation -------------------------------------------------------

#[test]
fn a_linear_graph_orders_its_actions_from_the_trigger() {
    assert_eq!(graph::validate(&linear()).unwrap(), vec!["a".to_string(), "b".to_string()]);
}

/// Two branches that meet again: both must run before the action they feed.
#[test]
fn a_fan_in_runs_both_branches_before_the_join() {
    let g = Graph {
        nodes: vec![
            trigger_node("t"),
            action_node("a", "left", at(200.0, 0.0)),
            action_node("b", "right", at(200.0, 100.0)),
            action_node("c", "join", at(400.0, 50.0)),
            output_node("o"),
        ],
        edges: vec![edge("t", "a"), edge("t", "b"), edge("a", "c"), edge("b", "c"), edge("c", "o")],
    };
    assert_eq!(graph::validate(&g).unwrap(), vec!["a".to_string(), "b".to_string(), "c".to_string()]);
}

/// Two actions ready at the same time are ordered by position, top first and then
/// leftmost, not by the order they happen to sit in the `nodes` array.
#[test]
fn ties_in_the_order_are_broken_by_position() {
    let lower = action_node("lower", "lower", at(200.0, 300.0));
    let upper = action_node("upper", "upper", at(200.0, 10.0));
    let left = action_node("left", "left", at(50.0, 10.0));
    let g = Graph {
        nodes: vec![trigger_node("t"), lower, upper, left, output_node("o")],
        edges: vec![
            edge("t", "lower"),
            edge("t", "upper"),
            edge("t", "left"),
            edge("lower", "o"),
            edge("upper", "o"),
            edge("left", "o"),
        ],
    };
    // `left` and `upper` share a y, so x decides; `lower` sits below both.
    assert_eq!(graph::validate(&g).unwrap(), vec!["left".to_string(), "upper".to_string(), "lower".to_string()]);
}

#[test]
fn a_cycle_is_rejected() {
    let mut g = linear();
    g.edges.push(edge("b", "a"));
    assert!(invalid(&g).contains("cycle"), "{}", invalid(&g));
}

#[test]
fn a_second_trigger_is_rejected() {
    let mut g = linear();
    g.nodes.push(trigger_node("t2"));
    g.edges.push(edge("t2", "a"));
    assert!(invalid(&g).contains("exactly one trigger"), "{}", invalid(&g));
}

#[test]
fn a_missing_output_is_rejected() {
    let mut g = linear();
    g.nodes.retain(|n| n.kind != NodeKind::Output);
    g.edges.retain(|e| e.target != "o");
    assert!(invalid(&g).contains("exactly one output"), "{}", invalid(&g));
}

#[test]
fn a_graph_with_no_actions_is_rejected() {
    let g = Graph { nodes: vec![trigger_node("t"), output_node("o")], edges: vec![edge("t", "o")] };
    assert!(invalid(&g).contains("at least one action"), "{}", invalid(&g));
}

#[test]
fn an_action_the_trigger_cannot_reach_is_rejected() {
    let mut g = linear();
    g.nodes.push(action_node("x", "orphan", at(200.0, 400.0)));
    g.edges.push(edge("x", "o"));
    assert!(invalid(&g).contains("cannot be reached from the trigger"), "{}", invalid(&g));
}

#[test]
fn an_action_that_never_reaches_the_output_is_rejected() {
    let mut g = linear();
    g.nodes.push(action_node("x", "dead end", at(200.0, 400.0)));
    g.edges.push(edge("t", "x"));
    assert!(invalid(&g).contains("does not lead to the output"), "{}", invalid(&g));
}

/// Names are how a step is identified in the run log, so two actions cannot share one
/// even when only the case or the padding differs.
#[test]
fn duplicate_action_names_are_rejected_ignoring_case_and_padding() {
    let mut g = linear();
    g.nodes[2] = action_node("b", "  FIRST ", at(400.0, 0.0));
    assert!(invalid(&g).contains("two actions are named"), "{}", invalid(&g));
}

#[test]
fn a_data_object_that_does_not_match_its_node_kind_is_rejected() {
    let mut g = linear();
    g.nodes[0] = Node { id: "t".into(), kind: NodeKind::Trigger, position: at(0.0, 0.0), data: NodeData::Output { propose_memories: false, file_tasks: false } };
    assert!(invalid(&g).contains("carries output data"), "{}", invalid(&g));
}

#[test]
fn an_edge_naming_an_absent_node_is_rejected() {
    let mut g = linear();
    g.edges.push(edge("a", "ghost"));
    assert!(invalid(&g).contains("not in the graph"), "{}", invalid(&g));
}

// -- triggers ---------------------------------------------------------------

#[test]
fn a_five_field_cron_is_read_as_at_second_zero() {
    let schedule = parse_cron("*/5 * * * *").unwrap();
    let from = Utc.with_ymd_and_hms(2026, 9, 3, 10, 1, 0).unwrap();
    let next = schedule.after(&from).next().unwrap();
    assert_eq!(next, Utc.with_ymd_and_hms(2026, 9, 3, 10, 5, 0).unwrap());
}

#[test]
fn a_schedule_trigger_needs_a_cron_expression_that_parses() {
    let missing = validate_trigger(&Trigger { kind: TriggerKind::Schedule, cron: None, prompt: None }).unwrap_err();
    assert!(matches!(missing, AtlasError::Invalid(_)), "{missing}");
    let nonsense = validate_trigger(&Trigger { kind: TriggerKind::Schedule, cron: Some("every tuesday".into()), prompt: None }).unwrap_err();
    assert!(matches!(nonsense, AtlasError::Invalid(_)), "{nonsense}");
    validate_trigger(&Trigger { kind: TriggerKind::Schedule, cron: Some("0 9 * * 1".into()), prompt: None }).unwrap();
}

#[test]
fn a_prompt_trigger_needs_a_prompt() {
    let blank = validate_trigger(&Trigger { kind: TriggerKind::Prompt, cron: None, prompt: Some("  ".into()) }).unwrap_err();
    assert!(matches!(blank, AtlasError::Invalid(_)), "{blank}");
    validate_trigger(&Trigger { kind: TriggerKind::Prompt, cron: None, prompt: Some("summarise the day".into()) }).unwrap();
}

// -- the repository ---------------------------------------------------------

#[test]
fn a_workflow_round_trips_through_the_database() {
    let (_db, repo) = repo();
    let made = repo.create(&new_workflow("release"), "t").unwrap();
    assert_eq!(made.name, "release");
    assert!(made.enabled);
    assert_eq!(made.last_run_at, None);
    assert_eq!(made.last_status, None);

    let read = repo.get("release").unwrap();
    assert_eq!(read.id, made.id);
    assert_eq!(read.graph, made.graph);
    assert_eq!(repo.get(&made.id.to_string()).unwrap().name, "release");
    assert_eq!(repo.list(None).unwrap().len(), 1);
}

#[test]
fn a_workflow_with_a_broken_graph_is_never_stored() {
    let (_db, repo) = repo();
    let mut new = new_workflow("broken");
    new.graph.edges.push(edge("b", "a"));
    let err = repo.create(&new, "t").unwrap_err();
    assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
    assert_eq!(repo.list(None).unwrap().len(), 0);
}

#[test]
fn two_workflows_cannot_share_a_name() {
    let (_db, repo) = repo();
    repo.create(&new_workflow("release"), "t").unwrap();
    let err = repo.create(&new_workflow("Release"), "t").unwrap_err();
    assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
}

#[test]
fn a_patch_touches_only_the_fields_it_names() {
    let (_db, repo) = repo();
    let made = repo.create(&new_workflow("release"), "t").unwrap();
    let patched = repo
        .update("release", &WorkflowPatch { description: Some("ship it".into()), enabled: Some(false), ..Default::default() }, "t")
        .unwrap();
    assert_eq!(patched.description, "ship it");
    assert!(!patched.enabled);
    assert_eq!(patched.name, "release");
    assert_eq!(patched.graph, made.graph);

    // A double option tells "leave the project alone" from "make this global".
    let global = repo.update("release", &WorkflowPatch { project_id: Some(None), ..Default::default() }, "t").unwrap();
    assert_eq!(global.project_id, None);
}

#[test]
fn deleting_a_workflow_takes_its_runs_and_steps_with_it() {
    let (db, repo) = repo();
    let w = repo.create(&new_workflow("release"), "t").unwrap();
    let run = repo.create_run(w.id, TriggerKind::Manual).unwrap();
    repo.append_step(run.id, 0, "a", "first", "desktop").unwrap();

    repo.delete("release", "t").unwrap();
    assert!(matches!(repo.get("release"), Err(AtlasError::NotFound(_))));
    let left: i64 = db
        .with_conn(|c| Ok(c.query_row("select (select count(*) from workflow_runs) + (select count(*) from workflow_steps)", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn run_numbers_count_up_within_a_workflow_and_start_again_in_the_next() {
    let (_db, repo) = repo();
    let a = repo.create(&new_workflow("alpha"), "t").unwrap();
    let b = repo.create(&new_workflow("beta"), "t").unwrap();
    assert_eq!(repo.create_run(a.id, TriggerKind::Manual).unwrap().number, 1);
    assert_eq!(repo.create_run(a.id, TriggerKind::Manual).unwrap().number, 2);
    assert_eq!(repo.create_run(b.id, TriggerKind::Prompt).unwrap().number, 1);
    assert_eq!(repo.create_run(a.id, TriggerKind::Schedule).unwrap().number, 3);

    // Newest first, and the workflow now records that it has run.
    let runs = repo.list_runs(a.id, 10).unwrap();
    assert_eq!(runs.iter().map(|r| r.number).collect::<Vec<_>>(), vec![3, 2, 1]);
    assert!(repo.get("alpha").unwrap().last_run_at.is_some());
    assert_eq!(repo.get("alpha").unwrap().last_status, Some(RunStatus::Queued));
}

#[test]
fn a_run_carries_its_steps_and_their_logs() {
    let (_db, repo) = repo();
    let w = repo.create(&new_workflow("release"), "t").unwrap();
    let run = repo.create_run(w.id, TriggerKind::Manual).unwrap();
    repo.set_run_status(run.id, RunStatus::Running, None).unwrap();

    let step = repo.append_step(run.id, 0, "a", "first", "desktop").unwrap();
    assert_eq!(step.status, StepStatus::Running);
    assert!(step.log.is_empty());
    repo.append_log(step.id, &LogLine::now(LogLevel::Info, "calling the model")).unwrap();
    repo.append_log(step.id, &LogLine::now(LogLevel::Warn, "slow reply")).unwrap();
    let done = repo.finish_step(step.id, StepStatus::Success, Some("the answer"), &[LogLine::now(LogLevel::Info, "done")]).unwrap();
    assert_eq!(done.output.as_deref(), Some("the answer"));
    // `finish_step` writes the log whole, replacing what `append_log` had built up.
    assert_eq!(done.log.len(), 1);
    assert!(done.finished_at.is_some());

    let finished = repo.set_run_status(run.id, RunStatus::Success, Some(json!({"steps": 1}))).unwrap();
    assert!(finished.finished_at.is_some());
    assert_eq!(finished.summary, Some(json!({"steps": 1})));
    assert_eq!(repo.get("release").unwrap().last_status, Some(RunStatus::Success));

    let (read, steps) = repo.get_run(run.id).unwrap();
    assert_eq!(read.number, 1);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].name, "first");
}

#[test]
fn a_status_change_without_a_summary_keeps_the_one_the_run_has() {
    let (_db, repo) = repo();
    let w = repo.create(&new_workflow("release"), "t").unwrap();
    let run = repo.create_run(w.id, TriggerKind::Manual).unwrap();
    repo.set_run_status(run.id, RunStatus::Running, Some(json!({"note": "kept"}))).unwrap();
    let done = repo.set_run_status(run.id, RunStatus::Failed, None).unwrap();
    assert_eq!(done.summary, Some(json!({"note": "kept"})));
}

#[test]
fn a_queued_or_running_run_cancels_and_a_finished_one_conflicts() {
    let (_db, repo) = repo();
    let w = repo.create(&new_workflow("release"), "t").unwrap();

    let queued = repo.create_run(w.id, TriggerKind::Manual).unwrap();
    let cancelled = repo.cancel_run(queued.id).unwrap();
    assert_eq!(cancelled.status, RunStatus::Cancelled);
    assert!(cancelled.finished_at.is_some());
    // Cancelling twice is a conflict, not a second cancellation.
    assert!(matches!(repo.cancel_run(queued.id), Err(AtlasError::Conflict(_))));

    let running = repo.create_run(w.id, TriggerKind::Manual).unwrap();
    repo.set_run_status(running.id, RunStatus::Running, None).unwrap();
    assert_eq!(repo.cancel_run(running.id).unwrap().status, RunStatus::Cancelled);

    let done = repo.create_run(w.id, TriggerKind::Manual).unwrap();
    repo.set_run_status(done.id, RunStatus::Success, None).unwrap();
    let err = repo.cancel_run(done.id).unwrap_err();
    assert!(matches!(err, AtlasError::Conflict(_)), "{err}");
    assert_eq!(repo.get_run(done.id).unwrap().0.status, RunStatus::Success);
}

/// `set_run_status` is a compare-and-swap against terminal statuses: once a run is
/// `cancelled` (or `success` or `failed`), a later call asking for a different status
/// is a silent no-op rather than an overwrite. This is what protects a run cancelled
/// while its last step's model call was still in flight: `run_workflow` cannot see the
/// cancellation land in time and always tries to close such a run as `success`, and it
/// must not win that race.
#[test]
fn set_run_status_never_overwrites_a_terminal_status() {
    let (_db, repo) = repo();
    let w = repo.create(&new_workflow("release"), "t").unwrap();
    let run = repo.create_run(w.id, TriggerKind::Manual).unwrap();
    repo.set_run_status(run.id, RunStatus::Running, None).unwrap();
    let cancelled = repo.cancel_run(run.id).unwrap();
    assert_eq!(cancelled.status, RunStatus::Cancelled);
    let cancelled_finished_at = cancelled.finished_at;

    // A late "success" (or "failed") loses the race: the row stays cancelled, with the
    // `finished_at` cancellation itself stamped, not overwritten.
    let after = repo.set_run_status(run.id, RunStatus::Success, Some(json!({"steps": 2}))).unwrap();
    assert_eq!(after.status, RunStatus::Cancelled, "a terminal status must not move");
    assert_eq!(after.finished_at, cancelled_finished_at);
    assert_eq!(after.summary, None, "a swallowed status change must not smuggle in its summary either");
    assert_eq!(repo.get_run(run.id).unwrap().0.status, RunStatus::Cancelled);
}

#[test]
fn a_pending_run_is_one_that_is_queued_or_running() {
    let (_db, repo) = repo();
    let w = repo.create(&new_workflow("release"), "t").unwrap();
    assert!(!repo.has_pending_run(w.id).unwrap());
    let run = repo.create_run(w.id, TriggerKind::Manual).unwrap();
    assert!(repo.has_pending_run(w.id).unwrap());
    repo.set_run_status(run.id, RunStatus::Success, None).unwrap();
    assert!(!repo.has_pending_run(w.id).unwrap());
}

// -- the scheduler's question ----------------------------------------------

fn scheduled(name: &str, cron: &str) -> NewWorkflow {
    NewWorkflow { trigger: Trigger { kind: TriggerKind::Schedule, cron: Some(cron.into()), prompt: None }, ..new_workflow(name) }
}

#[test]
fn a_schedule_fires_once_for_an_occurrence_and_not_again() {
    let (_db, repo) = repo();
    let w = repo.create(&scheduled("nightly", "0 3 * * *"), "t").unwrap();
    let since = Utc.with_ymd_and_hms(2026, 9, 3, 2, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 3, 5, 0).unwrap();

    let due = repo.due_scheduled(since, now).unwrap();
    assert_eq!(due.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), vec!["nightly"]);

    // Starting the run stamps `last_run_at`, which is what moves the window forward.
    repo.create_run(w.id, TriggerKind::Schedule).unwrap();
    assert!(repo.due_scheduled(since, Utc::now()).unwrap().is_empty());
}

#[test]
fn only_enabled_scheduled_workflows_are_ever_due() {
    let (_db, repo) = repo();
    repo.create(&new_workflow("manual-one"), "t").unwrap();
    repo.create(&NewWorkflow { enabled: false, ..scheduled("off", "0 3 * * *") }, "t").unwrap();
    repo.create(&scheduled("on", "0 3 * * *"), "t").unwrap();
    let since = Utc.with_ymd_and_hms(2026, 9, 3, 2, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 3, 5, 0).unwrap();
    let due = repo.due_scheduled(since, now).unwrap();
    assert_eq!(due.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), vec!["on"]);
}

/// The window starts at `since` for a workflow that has never run, so a daemon coming
/// up at noon does not replay every occurrence since the epoch.
#[test]
fn a_workflow_that_never_ran_is_measured_from_the_time_passed_in() {
    let (_db, repo) = repo();
    repo.create(&scheduled("nightly", "0 3 * * *"), "t").unwrap();
    let since = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 30).unwrap();
    assert!(repo.due_scheduled(since, now).unwrap().is_empty());
}

// -- the document migration -------------------------------------------------

fn doc(db: &Db, name: &str, body: &str) {
    DocRepo::new(db, DocKind::Workflow)
        .save(&NewDoc { name: name.into(), body: body.into(), tags: vec![], project_id: None }, "t")
        .unwrap();
}

#[test]
fn each_workflow_document_becomes_a_manual_single_action_workflow() {
    let (db, repo) = repo();
    doc(&db, "release", "cut a tag and publish");
    doc(&db, "triage", "read the inbox");
    let docs = DocRepo::new(&db, DocKind::Workflow);
    let settings = SettingsRepo::new(&db);

    assert_eq!(migrate_workflow_docs(&docs, &repo, &settings, "t").unwrap(), 2);

    let made = repo.get("release").unwrap();
    assert_eq!(made.trigger.kind, TriggerKind::Manual);
    let order = graph::validate(&made.graph).unwrap();
    assert_eq!(order.len(), 1);
    let action = made.graph.nodes.iter().find(|n| n.kind == NodeKind::Action).unwrap();
    let NodeData::Action { name, instructions, agent, .. } = &action.data else { panic!("not an action") };
    assert_eq!(name, "main");
    assert_eq!(agent, "desktop");
    assert_eq!(instructions, "cut a tag and publish");
    assert_eq!(action.position, Position { x: 240.0, y: 0.0 });

    // The documents are gone and the flag is set.
    assert_eq!(docs.list(None).unwrap().len(), 0);
    assert_eq!(settings.get_raw(DOCS_MIGRATED_SETTING).unwrap(), Some(json!(true)));
}

#[test]
fn the_migration_runs_once_even_when_new_documents_appear() {
    let (db, repo) = repo();
    doc(&db, "release", "cut a tag");
    let docs = DocRepo::new(&db, DocKind::Workflow);
    let settings = SettingsRepo::new(&db);

    assert_eq!(migrate_workflow_docs(&docs, &repo, &settings, "t").unwrap(), 1);
    doc(&db, "written-after", "should stay a document");
    assert_eq!(migrate_workflow_docs(&docs, &repo, &settings, "t").unwrap(), 0);
    assert_eq!(repo.list(None).unwrap().len(), 1);
    assert_eq!(docs.list(None).unwrap().len(), 1);
}

/// A document whose name a workflow already holds keeps its own name with a number
/// appended rather than failing the migration or overwriting the workflow.
#[test]
fn a_name_a_workflow_already_holds_is_deduped() {
    let (db, repo) = repo();
    repo.create(&new_workflow("release"), "t").unwrap();
    doc(&db, "release", "the document version");
    let docs = DocRepo::new(&db, DocKind::Workflow);
    let settings = SettingsRepo::new(&db);

    assert_eq!(migrate_workflow_docs(&docs, &repo, &settings, "t").unwrap(), 1);
    let names = repo.list(None).unwrap().into_iter().map(|w| w.name).collect::<Vec<_>>();
    assert_eq!(names, vec!["release".to_string(), "release 2".to_string()]);
    let migrated = repo.get("release 2").unwrap();
    let action = migrated.graph.nodes.iter().find(|n| n.kind == NodeKind::Action).unwrap();
    let NodeData::Action { instructions, .. } = &action.data else { panic!("not an action") };
    assert_eq!(instructions, "the document version");
}

// -- the wire shape ---------------------------------------------------------

/// The GUI mirrors these shapes field for field, so a round trip has to keep every
/// variant of `NodeData` and every option on the trigger.
#[test]
fn a_full_workflow_round_trips_through_json() {
    let (_db, repo) = repo();
    let graph = Graph {
        nodes: vec![
            Node {
                id: "t".into(),
                kind: NodeKind::Trigger,
                position: at(0.0, 0.0),
                data: NodeData::Trigger(Trigger { kind: TriggerKind::Schedule, cron: Some("0 9 * * 1".into()), prompt: None }),
            },
            Node {
                id: "a".into(),
                kind: NodeKind::Action,
                position: at(240.0, 0.0),
                data: NodeData::Action {
                    name: "summarise".into(),
                    instructions: "write the weekly note".into(),
                    agent: "desktop".into(),
                    practices: vec!["rust".into()],
                    memories: Some(MemorySource {
                        kinds: vec!["decision".into()],
                        tags: vec!["atlas".into()],
                        limit: 5,
                        project_id: Some(Uuid::nil()),
                    }),
                },
            },
            Node {
                id: "o".into(),
                kind: NodeKind::Output,
                position: at(480.0, 0.0),
                data: NodeData::Output { propose_memories: true, file_tasks: true },
            },
        ],
        edges: vec![edge("t", "a"), edge("a", "o")],
    };
    let made = repo
        .create(
            &NewWorkflow {
                name: "weekly".into(),
                project_id: None,
                description: "the monday note".into(),
                trigger: Trigger { kind: TriggerKind::Schedule, cron: Some("0 9 * * 1".into()), prompt: None },
                graph,
                enabled: true,
            },
            "t",
        )
        .unwrap();

    let json = serde_json::to_value(&made).unwrap();
    assert_eq!(json["trigger"], json!({"kind": "schedule", "cron": "0 9 * * 1", "prompt": null}));
    assert_eq!(json["graph"]["nodes"][0]["data"], json!({"kind": "schedule", "cron": "0 9 * * 1", "prompt": null}));
    assert_eq!(
        json["graph"]["nodes"][1]["data"],
        json!({
            "name": "summarise",
            "instructions": "write the weekly note",
            "agent": "desktop",
            "practices": ["rust"],
            "memories": {"kinds": ["decision"], "tags": ["atlas"], "limit": 5, "project_id": "00000000-0000-0000-0000-000000000000"}
        })
    );
    assert_eq!(json["graph"]["nodes"][2]["data"], json!({"propose_memories": true, "file_tasks": true}));
    assert_eq!(json["graph"]["nodes"][1]["kind"], "action");

    let back: Workflow = serde_json::from_value(json).unwrap();
    assert_eq!(back.graph, made.graph);
    assert_eq!(back.trigger, made.trigger);
}

/// A run and a step as the API sends them.
#[test]
fn a_run_and_a_step_serialise_with_the_names_the_gui_reads() {
    let (_db, repo) = repo();
    let w = repo.create(&new_workflow("release"), "t").unwrap();
    let run = repo.create_run(w.id, TriggerKind::Manual).unwrap();
    let step = repo.append_step(run.id, 0, "a", "first", "desktop").unwrap();
    repo.append_log(step.id, &LogLine { ts: Utc.with_ymd_and_hms(2026, 9, 3, 10, 0, 0).unwrap(), level: LogLevel::Error, text: "boom".into() })
        .unwrap();

    let run_json = serde_json::to_value(&run).unwrap();
    assert_eq!(run_json["trigger"], "manual");
    assert_eq!(run_json["status"], "queued");
    assert_eq!(run_json["number"], 1);
    assert_eq!(run_json["finished_at"], serde_json::Value::Null);

    let (_, steps) = repo.get_run(run.id).unwrap();
    let step_json = serde_json::to_value(&steps[0]).unwrap();
    assert_eq!(step_json["status"], "running");
    assert_eq!(step_json["action_id"], "a");
    assert_eq!(step_json["log"][0], json!({"ts": "2026-09-03T10:00:00Z", "level": "ERR", "text": "boom"}));
}
