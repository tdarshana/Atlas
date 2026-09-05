use crate::remote::RemoteBackend;
use atlas_core::backend::{PersonaBackend, ProjectBackend};
use atlas_core::models::{Persona, RosterEntry, RosterRow};
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
    /// Show a project's persona roster, or change it with --add, --remove and --default
    Personas {
        /// The project's UUID, or the root path it was connected at
        target: String,
        /// Put a persona (by name or slug) on the roster; repeatable
        #[arg(long)]
        add: Vec<String>,
        /// Take a persona (by name or slug) off the roster; repeatable
        #[arg(long)]
        remove: Vec<String>,
        /// Make a persona on the roster the project's default
        #[arg(long)]
        default: Option<String>,
    },
}

/// The roster after `add`, `remove` and `default` are applied to `current`, as the
/// entries the route takes: an added persona goes on the end, a removed one drops
/// out, and `default` moves the one default to the named persona. `Err` when the
/// default is not on the resulting roster, since the route would only say `Invalid`.
fn edit_roster(current: &[RosterRow], add: Vec<Persona>, remove: &[Persona], default: Option<&Persona>) -> anyhow::Result<Vec<RosterEntry>> {
    let mut entries: Vec<RosterEntry> = current.iter().map(|r| RosterEntry { persona_id: r.persona_id, is_default: r.is_default, position: r.position }).collect();
    for p in add {
        if !entries.iter().any(|e| e.persona_id == p.id) {
            let position = entries.iter().map(|e| e.position + 1).max().unwrap_or(0);
            entries.push(RosterEntry { persona_id: p.id, is_default: false, position });
        }
    }
    entries.retain(|e| !remove.iter().any(|p| p.id == e.persona_id));
    if let Some(p) = default {
        if !entries.iter().any(|e| e.persona_id == p.id) {
            anyhow::bail!("persona '{}' is not on the roster; add it first", p.slug);
        }
        for e in &mut entries {
            e.is_default = e.persona_id == p.id;
        }
    }
    Ok(entries)
}

fn print_roster(rows: &[RosterRow]) {
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|r| vec![r.name.clone(), r.slug.clone(), r.role.clone(), if r.is_default { "yes".into() } else { String::new() }])
        .collect();
    super::print_table(&["NAME", "SLUG", "ROLE", "DEFAULT"], &table);
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

/// The new `mcp_disabled_tools` list for `atlas project set --mcp-disable`/
/// `--mcp-enable`: `current` plus `disable`, minus `enable`, deduplicated and sorted.
/// The underlying patch field replaces the list wholesale rather than merging it, so
/// this reads the project's current override and folds the two flags into it; the
/// caller skips this entirely (leaving the override alone) when both flags are empty.
fn merge_mcp_tools(current: Vec<String>, disable: Vec<String>, enable: Vec<String>) -> Vec<String> {
    let mut tools: std::collections::BTreeSet<String> = current.into_iter().collect();
    tools.extend(disable);
    for t in &enable {
        tools.remove(t);
    }
    tools.into_iter().collect()
}

/// Rejects an unknown name in `--mcp-enable` before the merge: `BTreeSet::remove` is a
/// silent no-op for a name that was never disabled, so a typo would otherwise report
/// success and change nothing. `--mcp-disable` fails the same way already, server-side,
/// since every disabled name ends up validated; this gives `--mcp-enable` the identical
/// error shape without a round trip to find out.
fn validate_enable(names: &[String]) -> anyhow::Result<()> {
    atlas_core::settings::validate_mcp_tool_names(names)?;
    Ok(())
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
                validate_enable(&mcp_enable)?;
                let current = backend.get_project(id).await?.mcp_disabled_tools;
                Some(merge_mcp_tools(current, mcp_disable, mcp_enable))
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
        ProjectCmd::Personas { target, add, remove, default } => {
            let id = resolve(&target, backend).await?;
            let current = backend.project_roster(id).await?;
            if add.is_empty() && remove.is_empty() && default.is_none() {
                print_roster(&current);
                return Ok(());
            }
            let mut added = Vec::with_capacity(add.len());
            for name in &add {
                added.push(backend.get_persona(name).await?);
            }
            let mut removed = Vec::with_capacity(remove.len());
            for name in &remove {
                removed.push(backend.get_persona(name).await?);
            }
            let default = match &default {
                Some(name) => Some(backend.get_persona(name).await?),
                None => None,
            };
            let entries = edit_roster(&current, added, &removed, default.as_ref())?;
            print_roster(&backend.set_project_roster(id, entries, &backend.actor).await?);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn mcp_disable_adds_to_an_empty_override() {
        let merged = merge_mcp_tools(v(&[]), v(&["task_move", "memory_forget"]), v(&[]));
        assert_eq!(merged, v(&["memory_forget", "task_move"]));
    }

    #[test]
    fn mcp_disable_unions_with_the_current_override_without_duplicating() {
        let merged = merge_mcp_tools(v(&["task_move"]), v(&["task_move", "memory_forget"]), v(&[]));
        assert_eq!(merged, v(&["memory_forget", "task_move"]), "task_move must not appear twice");
    }

    #[test]
    fn mcp_enable_removes_from_the_current_override() {
        let merged = merge_mcp_tools(v(&["task_move", "memory_forget"]), v(&[]), v(&["task_move"]));
        assert_eq!(merged, v(&["memory_forget"]));
    }

    /// Enabling a tool that was never disabled is a no-op, not an error.
    #[test]
    fn mcp_enable_of_a_tool_not_in_the_override_is_a_no_op() {
        let merged = merge_mcp_tools(v(&["task_move"]), v(&[]), v(&["memory_forget"]));
        assert_eq!(merged, v(&["task_move"]));
    }

    /// Disabling and enabling the same name in one call is not a contradiction: the
    /// flags apply disable-then-enable, so the name ends up enabled.
    #[test]
    fn mcp_disable_and_enable_of_the_same_tool_leaves_it_enabled() {
        let merged = merge_mcp_tools(v(&[]), v(&["task_move"]), v(&["task_move"]));
        assert!(merged.is_empty(), "{merged:?}");
    }

    #[test]
    fn mcp_disable_and_enable_together_apply_both_sides() {
        let merged = merge_mcp_tools(v(&["memory_forget"]), v(&["task_move"]), v(&["memory_forget"]));
        assert_eq!(merged, v(&["task_move"]));
    }

    /// An unknown `--mcp-enable` name fails with the same message shape a `--mcp-disable`
    /// typo gets from server-side validation, instead of silently changing nothing.
    #[test]
    fn mcp_enable_of_an_unknown_tool_name_fails_loudly() {
        let err = validate_enable(&v(&["not_a_real_tool"])).unwrap_err();
        assert_eq!(err.to_string(), "invalid input: unknown MCP tool name 'not_a_real_tool'");
    }

    #[test]
    fn mcp_enable_of_a_known_tool_name_passes_validation() {
        assert!(validate_enable(&v(&["task_move"])).is_ok());
    }

    fn persona(slug: &str) -> Persona {
        Persona {
            id: uuid::Uuid::new_v4(), name: slug.into(), slug: slug.into(), role: String::new(), summary: String::new(), instructions: String::new(),
            skills: vec![], workflows: vec![], practices: vec![], mcp_servers: vec![], tools: vec![], access: Default::default(), models: Default::default(),
            tags: vec![], created_at: chrono::Utc::now(), updated_at: chrono::Utc::now(),
        }
    }

    fn row(p: &Persona, is_default: bool, position: i32) -> RosterRow {
        RosterRow { persona_id: p.id, name: p.name.clone(), slug: p.slug.clone(), role: String::new(), summary: String::new(), tags: vec![], is_default, position, project_id: uuid::Uuid::nil() }
    }

    /// An add lands at the end without a duplicate, a remove drops the row, and the
    /// default moves to the named persona and nowhere else.
    #[test]
    fn roster_edits_add_remove_and_move_the_default() {
        let (a, b, c) = (persona("a"), persona("b"), persona("c"));
        let current = vec![row(&a, true, 0), row(&b, false, 1)];
        let edited = edit_roster(&current, vec![b.clone(), c.clone()], &[], Some(&c)).unwrap();
        assert_eq!(edited.iter().map(|e| (e.persona_id, e.is_default, e.position)).collect::<Vec<_>>(), vec![(a.id, false, 0), (b.id, false, 1), (c.id, true, 2)]);
        let edited = edit_roster(&current, vec![], std::slice::from_ref(&a), None).unwrap();
        assert_eq!(edited.iter().map(|e| e.persona_id).collect::<Vec<_>>(), vec![b.id]);
        assert!(!edited[0].is_default, "removing the default leaves none rather than picking one");
    }

    /// A default that is not on the resulting roster is refused here, by name.
    #[test]
    fn a_default_off_the_roster_is_refused() {
        let (a, b) = (persona("a"), persona("b"));
        let err = edit_roster(&[row(&a, false, 0)], vec![], &[], Some(&b)).unwrap_err();
        assert_eq!(err.to_string(), "persona 'b' is not on the roster; add it first");
        let err = edit_roster(&[row(&a, false, 0)], vec![], std::slice::from_ref(&a), Some(&a)).unwrap_err();
        assert!(err.to_string().contains("not on the roster"), "a persona removed in the same call cannot be the default");
    }
}
