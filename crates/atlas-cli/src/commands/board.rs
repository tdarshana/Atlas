//! `atlas task ...` and `atlas board stages`: the task board from the command line.
//!
//! Every write is recorded under an actor: `cli`, or `cli/NAME` when the caller
//! passes `--as NAME`. That is the tool-label-plus-agent-name form the design spec
//! asks for and the form [`crate::remote::RemoteBackend::actor`] documents, so the
//! reads in this module carry it too.

use crate::remote::RemoteBackend;
use atlas_core::backend::Backend;
use atlas_core::models::*;
use chrono::{DateTime, Utc};
use clap::Subcommand;
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Subcommand)]
pub enum TaskCmd {
    /// List tasks as a table. Done tasks are left out unless --all.
    List {
        /// A project root path or board key, or `global`; the default is the project
        /// the working directory is in, else the global board
        #[arg(long)]
        project: Option<String>,
        /// The global board, whatever directory you are in
        #[arg(long, conflicts_with = "project")]
        global: bool,
        #[arg(long)]
        stage: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        /// Only tasks nothing is holding up
        #[arg(long)]
        ready: bool,
        /// Include tasks in a done stage
        #[arg(long)]
        all: bool,
    },
    /// Print a task with its blockers, children and events
    Show { key: String },
    /// Create a task and print its key
    Create {
        title: String,
        #[arg(long)]
        project: Option<String>,
        /// File the task on the global board, whatever directory you are in
        #[arg(long, conflicts_with = "project")]
        global: bool,
        /// task, bug, feature or chore
        #[arg(long)]
        kind: Option<String>,
        /// low, medium, high or urgent
        #[arg(long)]
        priority: Option<String>,
        #[arg(long = "label")]
        labels: Vec<String>,
        /// A task this one waits on, by key; repeatable
        #[arg(long = "blocked-by")]
        blocked_by: Vec<String>,
        #[arg(long)]
        parent: Option<String>,
        /// Read the description from a file, or from standard input with `-`
        #[arg(long)]
        description_file: Option<PathBuf>,
        #[arg(long)]
        stage: Option<String>,
    },
    /// Change a task's fields
    Update {
        key: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description_file: Option<PathBuf>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long, conflicts_with = "no_assignee")]
        assignee: Option<String>,
        /// Leave the task unassigned
        #[arg(long)]
        no_assignee: bool,
        /// Replaces the whole label set; repeatable
        #[arg(long = "label")]
        labels: Vec<String>,
        #[arg(long, conflicts_with = "no_parent")]
        parent: Option<String>,
        /// Detach the task from its parent
        #[arg(long)]
        no_parent: bool,
    },
    /// Move a task to another stage
    Move { key: String, stage: String },
    /// Add a comment to a task
    Comment { key: String, body: String },
    /// Assign a task to yourself and start it
    Claim {
        key: String,
        /// Take a task that is assigned to someone else
        #[arg(long)]
        force: bool,
    },
    /// Delete a task for good
    Delete {
        key: String,
        /// Required: deleting a task cannot be undone
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum BoardCmd {
    /// Print the effective stage list, or set or clear it
    Stages {
        /// A project root path or board key, or `global`; without it the project the
        /// working directory is in is meant, else the global list
        #[arg(long)]
        project: Option<String>,
        /// The global stage list, whatever directory you are in
        #[arg(long, conflicts_with = "project")]
        global: bool,
        /// The new list, stage names separated by commas, each optionally followed by
        /// `:done`, e.g. "Backlog,In Progress,Testing,Done:done". A stage name cannot
        /// contain a comma.
        #[arg(long, conflicts_with = "clear")]
        set: Option<String>,
        /// Drop the project's override and go back to the global list
        #[arg(long)]
        clear: bool,
    },
}

/// The actor a board command records its writes under.
pub fn actor(name: Option<&str>) -> String {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => format!("cli/{name}"),
        None => "cli".to_string(),
    }
}

pub async fn run_task(cmd: TaskCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    let actor = backend.actor.clone();
    match cmd {
        TaskCmd::List { project, global, stage, assignee, ready, all } => {
            // `resolve_project` answers `None` for the global board (see its own doc),
            // which means tasks with no project at all, not every project's tasks.
            let target = resolve_project(project.as_deref(), global, backend).await?;
            let filter = TaskFilter {
                project_id: target.as_ref().map(|p| p.id),
                stage,
                assignee,
                ready,
                query: None,
                include_done: all,
                global_only: target.is_none(),
            };
            let tasks = backend.list_tasks(filter).await?;
            let rows: Vec<Vec<String>> = tasks
                .iter()
                .map(|t| {
                    vec![
                        t.key.clone(),
                        t.stage.clone(),
                        t.priority.to_string(),
                        t.assignee.clone().unwrap_or_else(|| "-".into()),
                        one_line(&t.title),
                    ]
                })
                .collect();
            super::print_table(&["KEY", "STAGE", "PRIORITY", "ASSIGNEE", "TITLE"], &rows);
        }
        TaskCmd::Show { key } => show(&backend.get_task(&key).await?),
        TaskCmd::Create { title, project, global, kind, priority, labels, blocked_by, parent, description_file, stage } => {
            let new = NewTask {
                project_id: project_id(project.as_deref(), global, backend).await?,
                title,
                description: description_file.as_deref().map(super::read_source).transpose()?,
                kind: parse_opt::<TaskKind>(kind.as_deref())?,
                priority: parse_opt::<TaskPriority>(priority.as_deref())?,
                assignee: None,
                labels: (!labels.is_empty()).then_some(labels),
                parent,
                blocked_by: (!blocked_by.is_empty()).then_some(blocked_by),
                stage,
                source_ref: None,
            };
            println!("{}", backend.create_task(new, &actor).await?.key);
        }
        TaskCmd::Update { key, title, description_file, kind, priority, assignee, no_assignee, labels, parent, no_parent } => {
            let update = TaskUpdate {
                title,
                description: description_file.as_deref().map(super::read_source).transpose()?,
                kind: parse_opt::<TaskKind>(kind.as_deref())?,
                priority: parse_opt::<TaskPriority>(priority.as_deref())?,
                // A double option: absent leaves the field alone, `Some(None)` clears it.
                assignee: clearable(assignee, no_assignee),
                labels: (!labels.is_empty()).then_some(labels),
                parent: clearable(parent, no_parent),
                expected_updated_at: None,
            };
            println!("{}", line(&backend.update_task(&key, update, &actor).await?));
        }
        TaskCmd::Move { key, stage } => println!("{}", line(&backend.move_task(&key, &stage, None, &actor).await?)),
        TaskCmd::Comment { key, body } => {
            backend.comment_task(&key, &body, &actor).await?;
            println!("commented on {key}");
        }
        TaskCmd::Claim { key, force } => println!("{}", line(&backend.claim_task(&key, force, &actor).await?)),
        TaskCmd::Delete { key, yes } => {
            // Checked here rather than at the daemon: a delete nobody confirmed
            // should not reach the board at all.
            if !yes {
                anyhow::bail!("refusing to delete {key} without --yes");
            }
            backend.delete_task(&key, &actor).await?;
            println!("deleted {key}");
        }
    }
    Ok(())
}

pub async fn run_board(cmd: BoardCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    let actor = backend.actor.clone();
    let BoardCmd::Stages { project, global, set, clear } = cmd;
    let target = resolve_project(project.as_deref(), global, backend).await?;
    let project = target.as_ref().map(|p| p.id);
    match (set, clear) {
        (Some(list), _) => {
            let stages = parse_stage_list(&list)?;
            match &target {
                Some(p) => {
                    backend.set_project_stages(p.id, Some(stages), HashMap::new(), &actor).await?;
                }
                None => {
                    backend.set_board_stages(stages, HashMap::new(), &actor).await?;
                }
            }
            // Without --project or --global the target came from the working
            // directory, so say which board this call just rewrote.
            println!("set the stages on {}", board_label(target.as_ref()));
        }
        (None, true) => {
            let Some(p) = &target else {
                anyhow::bail!("--clear drops a project's override, so it needs --project");
            };
            backend.set_project_stages(p.id, None, HashMap::new(), &actor).await?;
            println!("cleared the override on {}", board_label(target.as_ref()));
        }
        (None, false) => {}
    }
    let list = backend.board_stages(project).await?;
    for stage in &list.stages {
        println!("{}{}", stage.name, if stage.done { "  (done)" } else { "" });
    }
    println!("{}", if list.overridden { "project override" } else { "global list" });
    Ok(())
}

/// The board a command works on, as an id. See [`resolve_project`].
async fn project_id(arg: Option<&str>, global: bool, backend: &RemoteBackend) -> anyhow::Result<Option<Uuid>> {
    Ok(resolve_project(arg, global, backend).await?.map(|p| p.id))
}

/// Resolves `--project` and `--global` to a board: `Some(project)` or, for the
/// project-less board, `None`. An argument is matched against the known board keys
/// and then the known root paths, so both `--project ATL` and `--project ~/code/atlas`
/// work; `--global`, and `--project global` for callers who have only the one flag to
/// hand, name the global board wherever the caller is standing. Without either, the
/// project whose root contains the working directory is meant, and if no project
/// does, the global board.
async fn resolve_project(arg: Option<&str>, global: bool, backend: &RemoteBackend) -> anyhow::Result<Option<Project>> {
    if global || arg.is_some_and(|a| a.eq_ignore_ascii_case("global")) {
        return Ok(None);
    }
    let projects = backend.list_projects().await?;
    let Some(arg) = arg else {
        return Ok(containing_project(projects));
    };
    if let Some(p) = projects.iter().find(|p| p.board_key.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(arg))) {
        return Ok(Some(p.clone()));
    }
    let abs = super::abs_path(Some(PathBuf::from(arg)))?;
    let abs = abs.to_string_lossy().to_string();
    match projects.into_iter().find(|p| p.root_path == arg || p.root_path == abs) {
        Some(p) => Ok(Some(p)),
        None => anyhow::bail!("no project with root or board key '{arg}'"),
    }
}

/// The connected project the working directory sits in. The longest matching root
/// wins, so a project nested inside another one is not shadowed by its parent.
fn containing_project(projects: Vec<Project>) -> Option<Project> {
    let cwd = std::env::current_dir().ok()?;
    projects.into_iter().filter(|p| cwd.starts_with(&p.root_path)).max_by_key(|p| p.root_path.len())
}

/// How a board is named back to the caller: the project's board key and name, or the
/// global board.
fn board_label(project: Option<&Project>) -> String {
    match project {
        Some(p) => match &p.board_key {
            Some(key) => format!("{} ({key})", p.name),
            None => p.name.clone(),
        },
        None => "the global board".to_string(),
    }
}

/// `Name[:done],...`. Anything after the colon that is not `done` is a typo worth
/// naming rather than a stage silently left open.
///
/// Commas separate the names, so a name cannot contain one and there is no quoting
/// that would let it. A double quote in the value is a caller trying to protect a
/// comma; splitting it anyway would quietly make two stages out of one, so say so.
fn parse_stage_list(list: &str) -> anyhow::Result<Vec<Stage>> {
    if list.contains('"') {
        anyhow::bail!("--set separates stage names with commas, so a stage name cannot contain one and quotes do not group it: '{list}'");
    }
    let mut stages = Vec::new();
    for part in list.split(',') {
        let part = part.trim();
        if part.is_empty() {
            anyhow::bail!("a stage name cannot be empty: '{list}'");
        }
        let (name, done) = match part.split_once(':') {
            Some((name, "done")) => (name, true),
            Some((_, flag)) => anyhow::bail!("unknown stage flag ':{flag}'; the only one is ':done'"),
            None => (part, false),
        };
        stages.push(Stage { name: name.trim().to_string(), done });
    }
    Ok(stages)
}

fn parse_opt<T: std::str::FromStr>(s: Option<&str>) -> anyhow::Result<Option<T>>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    Ok(s.map(str::parse).transpose()?)
}

/// Turns a `--field V` and its `--no-field` partner into the double option a
/// [`TaskUpdate`] wants: absent leaves the field alone, `Some(None)` clears it.
fn clearable(value: Option<String>, clear: bool) -> Option<Option<String>> {
    match (value, clear) {
        (Some(v), _) => Some(Some(v)),
        (None, true) => Some(None),
        (None, false) => None,
    }
}

fn show(detail: &TaskDetail) {
    let t = &detail.task;
    println!("{}  {}", t.key, t.title);
    println!("stage       {}", t.stage);
    println!("kind        {}", t.kind);
    println!("priority    {}", t.priority);
    println!("assignee    {}", t.assignee.as_deref().unwrap_or("-"));
    println!("labels      {}", or_dash(&t.labels.join(", ")));
    println!(
        "ready       {}",
        if t.ready { "yes".to_string() } else { format!("no  ({})", t.blocked_reason.as_deref().unwrap_or("blocked")) }
    );
    println!("created     {}  by {}", at(t.created_at), t.created_by);
    println!("updated     {}", at(t.updated_at));
    if let Some(closed) = t.closed_at {
        println!("closed      {}", at(closed));
    }
    if !t.description.trim().is_empty() {
        println!();
        println!("{}", t.description);
    }
    println!();
    println!("blockers    {}", or_dash(&t.blocked_by.join(", ")));
    println!();
    println!("children");
    if detail.children.is_empty() {
        println!("  -");
    }
    for c in &detail.children {
        println!("  {}  {}  {}", c.key, c.stage, one_line(&c.title));
    }
    println!();
    println!("events");
    if detail.events.is_empty() {
        println!("  -");
    }
    for e in &detail.events {
        println!("  {}  {:<14}  {:<10}  {}", at(e.created_at), e.actor, e.kind, one_line(&e.body));
    }
}

/// The one-line form `move`, `update` and `claim` report themselves with.
fn line(t: &Task) -> String {
    format!("{}  {}  {}", t.key, t.stage, one_line(&t.title))
}

fn at(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%d %H:%M").to_string()
}

fn or_dash(s: &str) -> String {
    if s.is_empty() { "-".to_string() } else { s.to_string() }
}

/// A stored title, body or comment may hold newlines; a table cell may not.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_lists_parse_and_reject() {
        let stages = parse_stage_list("Backlog, In Progress ,Testing,Done:done").unwrap();
        assert_eq!(stages.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Backlog", "In Progress", "Testing", "Done"]);
        assert_eq!(stages.iter().map(|s| s.done).collect::<Vec<_>>(), [false, false, false, true]);
        assert!(parse_stage_list("Backlog,,Done:done").is_err());
        assert!(parse_stage_list("Backlog,Done:closed").is_err());
    }

    /// A quoted name is a caller trying to keep a comma inside one stage. Splitting
    /// it would make two stages nobody asked for, so the list is refused instead.
    #[test]
    fn a_quoted_stage_name_is_refused_rather_than_split() {
        let err = parse_stage_list(r#"Backlog,"Wait, then go",Done:done"#).unwrap_err();
        assert!(err.to_string().contains("cannot contain one"), "{err}");
    }

    #[test]
    fn board_label_names_the_project_or_the_global_board() {
        assert_eq!(board_label(None), "the global board");
    }

    #[test]
    fn actor_takes_the_agent_name_after_the_tool_label() {
        assert_eq!(actor(None), "cli");
        assert_eq!(actor(Some("codex")), "cli/codex");
        assert_eq!(actor(Some("  ")), "cli");
    }

    #[test]
    fn clearable_tells_absent_from_cleared() {
        assert_eq!(clearable(Some("ann".into()), false), Some(Some("ann".to_string())));
        assert_eq!(clearable(None, true), Some(None));
        assert_eq!(clearable(None, false), None);
    }
}
