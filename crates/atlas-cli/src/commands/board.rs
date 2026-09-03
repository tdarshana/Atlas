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
        /// A project root path or board key; the default is the project the
        /// working directory is in, else the global board
        #[arg(long)]
        project: Option<String>,
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
        /// A project root path or board key; without it the global list is meant
        #[arg(long)]
        project: Option<String>,
        /// The new list as `Name[:done],...`, e.g. "Backlog,In Progress,Testing,Done:done"
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
        TaskCmd::List { project, stage, assignee, ready, all } => {
            let filter = TaskFilter {
                project_id: project_id(project.as_deref(), backend).await?,
                stage,
                assignee,
                ready,
                query: None,
                include_done: all,
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
        TaskCmd::Create { title, project, kind, priority, labels, blocked_by, parent, description_file, stage } => {
            let new = NewTask {
                project_id: project_id(project.as_deref(), backend).await?,
                title,
                description: description_file.as_deref().map(super::read_source).transpose()?,
                kind: parse_opt::<TaskKind>(kind.as_deref())?,
                priority: parse_opt::<TaskPriority>(priority.as_deref())?,
                assignee: None,
                labels: (!labels.is_empty()).then_some(labels),
                parent,
                blocked_by: (!blocked_by.is_empty()).then_some(blocked_by),
                stage,
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
    let BoardCmd::Stages { project, set, clear } = cmd;
    let project = project_id(project.as_deref(), backend).await?;
    match (set, clear) {
        (Some(list), _) => {
            let stages = parse_stage_list(&list)?;
            match project {
                Some(id) => {
                    backend.set_project_stages(id, Some(stages), HashMap::new(), &actor).await?;
                }
                None => {
                    backend.set_board_stages(stages, HashMap::new(), &actor).await?;
                }
            }
        }
        (None, true) => {
            let Some(id) = project else {
                anyhow::bail!("--clear drops a project's override, so it needs --project");
            };
            backend.set_project_stages(id, None, HashMap::new(), &actor).await?;
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

/// Resolves `--project`. An argument is matched against the known board keys and
/// then the known root paths, so both `--project ATL` and `--project ~/code/atlas`
/// work. Without one, the project whose root contains the working directory is
/// meant, and if no project does, the global board.
async fn project_id(arg: Option<&str>, backend: &RemoteBackend) -> anyhow::Result<Option<Uuid>> {
    let projects = backend.list_projects().await?;
    let Some(arg) = arg else {
        return Ok(containing_project(&projects));
    };
    if let Some(p) = projects.iter().find(|p| p.board_key.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(arg))) {
        return Ok(Some(p.id));
    }
    let abs = super::abs_path(Some(PathBuf::from(arg)))?;
    let abs = abs.to_string_lossy().to_string();
    match projects.iter().find(|p| p.root_path == arg || p.root_path == abs) {
        Some(p) => Ok(Some(p.id)),
        None => anyhow::bail!("no project with root or board key '{arg}'"),
    }
}

/// The connected project the working directory sits in. The longest matching root
/// wins, so a project nested inside another one is not shadowed by its parent.
fn containing_project(projects: &[Project]) -> Option<Uuid> {
    let cwd = std::env::current_dir().ok()?;
    projects
        .iter()
        .filter(|p| cwd.starts_with(&p.root_path))
        .max_by_key(|p| p.root_path.len())
        .map(|p| p.id)
}

/// `Name[:done],...`. Anything after the colon that is not `done` is a typo worth
/// naming rather than a stage silently left open.
fn parse_stage_list(list: &str) -> anyhow::Result<Vec<Stage>> {
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
