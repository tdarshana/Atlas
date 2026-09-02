use crate::remote::RemoteBackend;
use atlas_core::backend::Backend;
use atlas_core::models::{DocKind, NewDoc};
use clap::Subcommand;
use std::path::PathBuf;
use uuid::Uuid;

/// `practice` and `workflow` are the same commands over different collections,
/// so they share one definition and take the kind from the caller.
#[derive(Subcommand)]
pub enum DocCmd {
    /// List the documents, optionally only those scoped to a project
    List {
        /// Limit to the project at PATH
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Print one document as JSON
    Show { name: String },
    /// Create or replace a document
    Save {
        name: String,
        /// File holding the document body; `-` reads standard input
        #[arg(long)]
        body_file: PathBuf,
        /// Tag to file the document under; repeatable
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Scope the document to the project at PATH
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Remove a document
    Delete { name: String },
}

pub async fn run(kind: DocKind, cmd: DocCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        DocCmd::List { project } => {
            let docs = backend.list_docs(kind, project_id(project, backend).await?).await?;
            let rows: Vec<Vec<String>> = docs
                .iter()
                .map(|d| vec![d.name.clone(), d.tags.join(","), d.project_id.map(|p| p.to_string()).unwrap_or_else(|| "-".into())])
                .collect();
            super::print_table(&["NAME", "TAGS", "PROJECT"], &rows);
            Ok(())
        }
        DocCmd::Show { name } => super::print_json(&backend.get_doc(kind, &name).await?),
        DocCmd::Save { name, body_file, tags, project } => {
            let body = super::read_source(&body_file)?;
            let doc = NewDoc { name, body, tags, project_id: project_id(project, backend).await? };
            super::print_json(&backend.save_doc(kind, doc, "cli").await?)
        }
        DocCmd::Delete { name } => {
            backend.delete_doc(kind, &name, "cli").await?;
            println!("deleted {kind} {name}");
            Ok(())
        }
    }
}

/// Resolves `--project PATH` to an id, connecting the project so that scoping a
/// document to a repository Atlas has not seen yet works the first time.
async fn project_id(path: Option<PathBuf>, backend: &RemoteBackend) -> anyhow::Result<Option<Uuid>> {
    match path {
        Some(p) => Ok(Some(backend.connect_project(super::abs_path(Some(p))?, "cli").await?.id)),
        None => Ok(None),
    }
}
