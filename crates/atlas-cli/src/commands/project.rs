use crate::remote::RemoteBackend;
use atlas_core::backend::Backend;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// Detect and record the project at PATH (default: the working directory)
    Connect { path: Option<PathBuf> },
    /// List every project Atlas knows about
    List,
    /// Print the project at PATH with its memories, practices and workflows
    Show { path: Option<PathBuf> },
    /// Remove a project from Atlas by id or root path. Its memories are kept.
    Forget {
        /// The project's UUID, or the root path it was connected at
        target: String,
    },
}

/// Resolves a `forget` argument to a project id. A UUID is taken as an id; anything
/// else is matched against the known roots, exactly first and then by the absolute
/// form of the argument, so both `atlas project forget .` and a path copied out of
/// `atlas project list` work. Listing first also means an unknown id fails here with
/// a readable message instead of as a bare 404.
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
    let matches: Vec<_> = projects.iter().filter(|p| p.root_path == target || p.root_path == abs).collect();
    match matches.as_slice() {
        [p] => Ok(p.id),
        [] => anyhow::bail!("no project with id or root '{target}'"),
        many => anyhow::bail!("'{target}' matches {} projects; pass an id instead", many.len()),
    }
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
        ProjectCmd::List => {
            let projects = backend.list_projects().await?;
            let rows: Vec<Vec<String>> = projects
                .iter()
                .map(|p| vec![p.name.clone(), p.root_path.clone(), p.last_seen_at.format("%Y-%m-%d %H:%M").to_string()])
                .collect();
            super::print_table(&["NAME", "ROOT", "LAST SEEN"], &rows);
            Ok(())
        }
    }
}
