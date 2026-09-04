use crate::remote::RemoteBackend;
use atlas_core::backend::Backend;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// Detect and record the project at PATH (default: the working directory)
    Connect { path: Option<PathBuf> },
    /// List every project Atlas knows about, with each one's short id; `atlas project
    /// show <path>` reaches the full one
    List,
    /// Print the project at PATH with its memories, practices and workflows
    Show { path: Option<PathBuf> },
    /// Remove a project from Atlas by id or root path. Its memories are kept.
    Forget {
        /// The project's UUID, or the root path it was connected at
        target: String,
    },
    /// Change a project's name, board key prefix or git remote
    Set {
        /// The project's UUID, or the root path it was connected at
        target: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New board key prefix, e.g. ATL. Every task key on the board is renamed with it.
        #[arg(long)]
        key: Option<String>,
        /// New git remote
        #[arg(long, conflicts_with = "no_remote")]
        remote: Option<String>,
        /// Clear the git remote
        #[arg(long)]
        no_remote: bool,
        /// MCP tool names to disable for this project, on top of the global list, e.g.
        /// --mcp-disable task_move,memory_forget
        #[arg(long, value_delimiter = ',')]
        mcp_disable: Vec<String>,
        /// MCP tool names to re-enable for this project
        #[arg(long, value_delimiter = ',')]
        mcp_enable: Vec<String>,
    },
    /// Show a project's unified log: task events, memory writes, project and sync
    /// audit rows, and extraction jobs, newest first
    Log {
        /// The project's UUID, or the root path it was connected at
        target: String,
        /// Keep only entries written by this actor
        #[arg(long)]
        source: Option<String>,
        /// Keep only entries of this kind, e.g. moved, remembered, synced
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Print the entries as JSON instead of a table
        #[arg(long)]
        json: bool,
    },
}

/// Resolves a `forget`, `log` or `set` argument to a project id. A UUID is taken as an
/// id; anything else is matched against the known projects: their root (exactly, then
/// by the argument's absolute form, so both `atlas project forget .` and a path copied
/// out of `atlas project list` work) or their display name (exactly, case-insensitive).
/// Listing first also means an unknown id fails here with a readable message instead of
/// as a bare 404.
async fn resolve(target: &str, backend: &RemoteBackend) -> anyhow::Result<uuid::Uuid> {
    let projects = backend.list_projects().await?;
    if let Ok(id) = uuid::Uuid::parse_str(target) {
        return match projects.iter().find(|p| p.id == id) {
            Some(p) => Ok(p.id),
            None => anyhow::bail!("no project with id {id}"),
        };
    }
    let abs = super::abs_path(Some(PathBuf::from(target)))?;
    let abs = abs.to_string_lossy().to_string();
    let matches: Vec<_> = projects.iter().filter(|p| p.root_path == target || p.root_path == abs || p.name.eq_ignore_ascii_case(target)).collect();
    match matches.as_slice() {
        [p] => Ok(p.id),
        [] => anyhow::bail!("no project with id, root or name '{target}'"),
        many => {
            let roots = many.iter().map(|p| p.root_path.as_str()).collect::<Vec<_>>().join(", ");
            anyhow::bail!("'{target}' matches {} projects ({roots}); pass an id instead", many.len())
        }
    }
}

/// The first 8 hex characters of a project's id, for a table column narrow enough to
/// read alongside the name and root; `atlas project show <path>` prints the full id.
fn short_id(id: uuid::Uuid) -> String {
    id.to_string()[..8].to_string()
}

pub async fn run(cmd: ProjectCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        ProjectCmd::Connect { path } => super::print_json(&backend.connect_project(super::abs_path(path)?, "cli").await?),
        ProjectCmd::Show { path } => super::print_json(&backend.project_context(super::abs_path(path)?, "cli").await?),
        ProjectCmd::Forget { target } => {
            let id = resolve(&target, backend).await?;
            backend.delete_project(id, "cli").await?;
            println!("forgot project {id}");
            Ok(())
        }
        ProjectCmd::Set { target, name, key, remote, no_remote, mcp_disable, mcp_enable } => {
            let id = resolve(&target, backend).await?;
            // Absent leaves the remote alone; `--no-remote` clears it; `--remote` sets it.
            let git_remote = match (remote, no_remote) {
                (Some(r), _) => Some(Some(r)),
                (None, true) => Some(None),
                (None, false) => None,
            };
            // `--mcp-disable`/`--mcp-enable` add to and remove from the project's
            // current override; absent both leaves it alone, since the underlying
            // patch field replaces the list wholesale rather than merging it.
            let mcp_disabled_tools = if mcp_disable.is_empty() && mcp_enable.is_empty() {
                None
            } else {
                let mut tools: std::collections::BTreeSet<String> = backend.get_project(id).await?.mcp_disabled_tools.into_iter().collect();
                tools.extend(mcp_disable);
                for t in &mcp_enable {
                    tools.remove(t);
                }
                Some(tools.into_iter().collect())
            };
            let patch = atlas_core::models::ProjectPatch { name, board_key: key, git_remote, mcp_disabled_tools };
            super::print_json(&backend.update_project(id, patch, "cli").await?)
        }
        ProjectCmd::Log { target, source, kind, limit, json } => {
            let id = resolve(&target, backend).await?;
            let filter = atlas_core::models::LogFilter { source, kind, q: None, after: None, limit: Some(limit) };
            let entries = backend.project_log(id, filter).await?;
            if json {
                return super::print_json(&entries);
            }
            let rows: Vec<Vec<String>> = entries
                .iter()
                .map(|e| {
                    let reference = e
                        .reference
                        .as_ref()
                        .map(|r| r.key.clone().unwrap_or_else(|| r.id.map(|i| i.to_string()).unwrap_or_else(|| r.kind.clone())))
                        .unwrap_or_default();
                    vec![
                        e.time.format("%Y-%m-%d %H:%M").to_string(),
                        e.source.clone(),
                        e.kind.clone(),
                        e.detail.lines().next().unwrap_or_default().chars().take(80).collect(),
                        reference,
                    ]
                })
                .collect();
            super::print_table(&["TIME", "SOURCE", "EVENT", "DETAIL", "REF"], &rows);
            Ok(())
        }
        ProjectCmd::List => {
            let projects = backend.list_projects().await?;
            let rows: Vec<Vec<String>> = projects
                .iter()
                .map(|p| vec![short_id(p.id), p.name.clone(), p.root_path.clone(), p.last_seen_at.format("%Y-%m-%d %H:%M").to_string()])
                .collect();
            super::print_table(&["ID", "NAME", "ROOT", "LAST SEEN"], &rows);
            Ok(())
        }
    }
}
