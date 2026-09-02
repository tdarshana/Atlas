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
}

pub async fn run(cmd: ProjectCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        ProjectCmd::Connect { path } => super::print_json(&backend.connect_project(super::abs_path(path)?, "cli").await?),
        ProjectCmd::Show { path } => super::print_json(&backend.project_context(super::abs_path(path)?, "cli").await?),
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
