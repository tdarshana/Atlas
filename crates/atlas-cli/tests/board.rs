//! The board from the command line, against a real daemon.

#[allow(dead_code)]
mod common;

use common::TestDaemon;

/// Runs `atlas` and returns the exit code, stdout and stderr.
fn runner(daemon: &TestDaemon) -> impl Fn(&[&str]) -> (i32, String, String) + '_ {
    move |args: &[&str]| {
        let out = daemon.cmd().args(args).output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

/// Create, list, move, a refused move, a refused delete, a comment under `--as`,
/// and finally a real delete: the whole `atlas task` surface in one daemon.
#[test]
fn task_commands_create_list_move_comment_and_delete() {
    let daemon = TestDaemon::new();
    let run = runner(&daemon);

    let (code, out, err) = run(&["task", "create", "Wire the board up"]);
    assert_eq!(code, 0, "{err}");
    let key = out.trim().to_string();
    assert!(!key.is_empty(), "create should print the key, got {out:?}");

    let (code, out, err) = run(&["task", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("KEY") && out.contains("STAGE"), "list should print a table: {out}");
    assert!(
        out.lines().any(|l| l.contains(&key) && l.contains("Backlog") && l.contains("Wire the board up")),
        "a new task should be in Backlog: {out}"
    );
    assert!(out.lines().any(|l| l.contains(&key) && l.contains(" -  ")), "an unassigned task shows a dash: {out}");

    let (code, _, err) = run(&["task", "move", &key, "Testing"]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["task", "list", "--stage", "Testing"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(&key), "the task should be in Testing: {out}");

    let (code, out, err) = run(&["task", "move", &key, "Nowhere"]);
    assert_eq!(code, 1, "a stage that is not on the board should fail: {out}{err}");
    assert!(err.contains("Backlog"), "the failure should name the stages: {err}");

    let (code, out, err) = run(&["task", "delete", &key]);
    assert_eq!(code, 1, "delete without --yes should refuse: {out}");
    assert!(err.contains("--yes"), "the refusal should say what is missing: {err}");
    let (_, out, _) = run(&["task", "list", "--stage", "Testing"]);
    assert!(out.contains(&key), "a refused delete leaves the task: {out}");

    let (code, _, err) = run(&["task", "comment", &key, "verified by hand", "--as", "codex"]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["task", "show", &key]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Wire the board up"), "show should print the title: {out}");
    assert!(out.contains("verified by hand"), "show should print the comment: {out}");
    assert!(out.contains("codex"), "show should print the actor --as named: {out}");

    let (code, _, err) = run(&["task", "delete", &key, "--yes"]);
    assert_eq!(code, 0, "{err}");
    let (_, out, _) = run(&["task", "list", "--all"]);
    assert!(!out.contains(&key), "the task should be gone: {out}");
}

/// `--ready`, `--assignee` and `--all` narrow the list; `claim` takes the task.
#[test]
fn task_list_filters_and_claim() {
    let daemon = TestDaemon::new();
    let run = runner(&daemon);

    let (_, out, _) = run(&["task", "create", "First"]);
    let first = out.trim().to_string();
    let (_, out, _) = run(&["task", "create", "Second", "--blocked-by", &first]);
    let second = out.trim().to_string();

    let (code, out, err) = run(&["task", "list", "--ready"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(&first), "the unblocked task is ready: {out}");
    assert!(!out.contains(&second), "the blocked task is not ready: {out}");

    let (code, _, err) = run(&["task", "claim", &first, "--as", "codex"]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["task", "list", "--assignee", "cli/codex"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(&first), "claim assigns the actor: {out}");

    let (_, out, _) = run(&["task", "move", &first, "Done"]);
    assert!(out.contains("Done"), "move should report the new stage: {out}");
    let (_, out, _) = run(&["task", "list"]);
    assert!(!out.contains(&first), "a done task is out of the default list: {out}");
    let (_, out, _) = run(&["task", "list", "--all"]);
    assert!(out.contains(&first), "--all brings it back: {out}");
}

/// `--project` takes a root path or a board key, and a task created under a
/// project only shows up in that project's list.
#[test]
fn project_scoping_accepts_a_root_or_a_board_key() {
    let daemon = TestDaemon::new();
    let repo = tempfile::tempdir().unwrap();
    common::fixture_repo(repo.path());
    let run = runner(&daemon);
    let root = repo.path().to_str().unwrap();

    let (code, out, err) = run(&["project", "connect", root]);
    assert_eq!(code, 0, "{err}");
    let project: serde_json::Value = serde_json::from_str(&out).unwrap();
    let board_key = project["board_key"].as_str().expect("connect should report a board key").to_string();

    let (code, out, err) = run(&["task", "create", "Scoped", "--project", root]);
    assert_eq!(code, 0, "{err}");
    let scoped = out.trim().to_string();
    assert!(scoped.starts_with(&board_key), "a project task takes the board key: {scoped} / {board_key}");

    let (_, out, _) = run(&["task", "create", "Unscoped"]);
    let unscoped = out.trim().to_string();

    let (code, out, err) = run(&["task", "list", "--project", &board_key]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(&scoped), "the board key should select the project: {out}");
    assert!(!out.contains(&unscoped), "a global task is not in a project list: {out}");

    let (code, out, err) = run(&["task", "list", "--project", root]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(&scoped), "the root path should select the project too: {out}");

    let (code, out, err) = run(&["task", "list", "--project", "nothing-like-this"]);
    assert_eq!(code, 1, "an unknown project should fail: {out}");
    assert!(err.contains("nothing-like-this"), "{err}");
}

/// `atlas board stages` prints the effective list, marks the done stages, says
/// where the list comes from, and can set and clear an override.
#[test]
fn board_stages_prints_sets_and_clears() {
    let daemon = TestDaemon::new();
    let repo = tempfile::tempdir().unwrap();
    common::fixture_repo(repo.path());
    let run = runner(&daemon);
    let root = repo.path().to_str().unwrap();

    let (code, out, err) = run(&["board", "stages"]);
    assert_eq!(code, 0, "{err}");
    for stage in ["Backlog", "In Progress", "Testing", "Done"] {
        assert!(out.contains(stage), "the default board should list {stage}: {out}");
    }
    assert!(out.lines().any(|l| l.starts_with("Done") && l.contains("(done)")), "Done is a done stage: {out}");
    assert!(out.contains("global list"), "an unoverridden board says so: {out}");

    let (code, _, err) = run(&["project", "connect", root]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["board", "stages", "--project", root, "--set", "Todo,Doing,Shipped:done"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Todo") && out.contains("Shipped"), "{out}");
    assert!(out.contains("project override"), "an overridden board says so: {out}");

    // The override is the project's alone; the global board is untouched.
    let (_, out, _) = run(&["board", "stages"]);
    assert!(out.contains("Backlog") && !out.contains("Todo"), "{out}");

    let (code, out, err) = run(&["board", "stages", "--project", root, "--clear"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Backlog") && out.contains("global list"), "clearing goes back to the global list: {out}");

    let (code, out, err) = run(&["board", "stages", "--clear"]);
    assert_eq!(code, 1, "--clear without --project has nothing to clear: {out}");
    assert!(err.contains("--project"), "{err}");
}
