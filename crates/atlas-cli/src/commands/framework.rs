//! `atlas framework ...`: list, read and import from a project's detected
//! planning frameworks (Superpowers, OpenSpec, SpecKit, GSD).

use crate::remote::RemoteBackend;
use atlas_core::backend::ProjectBackend;
use atlas_core::models::*;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum FrameworkCmd {
    /// List the frameworks detected in a project, with their documents
    List {
        /// A project root path or board key; without it, the project the working
        /// directory is in is meant
        #[arg(long)]
        project: Option<String>,
    },
    /// Print one document's text
    Show {
        /// superpowers, openspec, speckit or gsd
        kind: String,
        /// A document path from `framework list`'s output
        path: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Import a framework's tasks or decisions
    Import {
        /// superpowers, openspec, speckit or gsd
        kind: String,
        /// Import tasks onto the board
        #[arg(long, conflicts_with = "decisions")]
        tasks: bool,
        /// Import decisions as pending memories
        #[arg(long)]
        decisions: bool,
        #[arg(long)]
        project: Option<String>,
    },
}

pub async fn run(cmd: FrameworkCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        FrameworkCmd::List { project } => {
            let p = resolve_project(project.as_deref(), backend).await?;
            let listings = backend.list_frameworks(p.id).await?;
            let rows: Vec<Vec<String>> = listings
                .iter()
                .map(|l| vec![l.inventory.kind.to_string(), l.inventory.roots.join(", "), l.documents.len().to_string(), l.inventory.tasks.to_string()])
                .collect();
            super::print_table(&["FRAMEWORK", "ROOTS", "DOCS", "TASK FILES"], &rows);
        }
        FrameworkCmd::Show { kind, path, project } => {
            let kind: FrameworkKind = kind.parse()?;
            let p = resolve_project(project.as_deref(), backend).await?;
            let content = backend.get_framework_doc(p.id, kind, &path).await?;
            println!("{content}");
        }
        FrameworkCmd::Import { kind, tasks, decisions, project } => {
            let kind: FrameworkKind = kind.parse()?;
            let what = match (tasks, decisions) {
                (true, false) => ImportWhat::Tasks,
                (false, true) => ImportWhat::Decisions,
                (false, false) => anyhow::bail!("pass --tasks or --decisions"),
                (true, true) => unreachable!("clap refuses --tasks and --decisions together"),
            };
            let p = resolve_project(project.as_deref(), backend).await?;
            let report = backend.import_framework(p.id, kind, what, &backend.actor).await?;
            println!(
                "created {}, updated {}, reparented {}, skipped {}",
                report.created, report.updated, report.reparented, report.skipped
            );
        }
    }
    Ok(())
}

/// The project a framework command works on. Unlike the board commands, there is
/// no global board to fall back to: a framework's files live inside one project,
/// so an argument that resolves to none is refused rather than silently widened.
async fn resolve_project(arg: Option<&str>, backend: &RemoteBackend) -> anyhow::Result<Project> {
    let projects = backend.list_projects().await?;
    if let Some(arg) = arg {
        if let Some(p) = projects.iter().find(|p| p.board_key.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(arg))) {
            return Ok(p.clone());
        }
        let abs = super::abs_path(Some(PathBuf::from(arg)))?;
        let abs = abs.to_string_lossy().to_string();
        return projects
            .into_iter()
            .find(|p| p.root_path == arg || p.root_path == abs)
            .ok_or_else(|| anyhow::anyhow!("no project with root or board key '{arg}'"));
    }
    let cwd = std::env::current_dir()?;
    projects
        .into_iter()
        .filter(|p| cwd.starts_with(&p.root_path))
        .max_by_key(|p| p.root_path.len())
        .ok_or_else(|| anyhow::anyhow!("no connected project contains the working directory; pass --project"))
}
