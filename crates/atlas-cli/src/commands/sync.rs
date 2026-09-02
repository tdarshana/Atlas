use crate::remote::RemoteBackend;
use atlas_core::backend::Backend;
use atlas_core::models::{SyncAction, SyncKind, SyncRequest};
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct SyncArgs {
    /// Sync the project at PATH (default: the working directory)
    #[arg(long, conflicts_with = "global")]
    pub project: Option<PathBuf>,
    /// Sync the home directory's agent files instead of a project
    #[arg(long)]
    pub global: bool,
    /// With --global, write into PATH instead of the daemon user's home directory
    #[arg(long, requires = "global")]
    pub home: Option<PathBuf>,
    /// Report what would change without writing; exits 1 if anything would change
    #[arg(long)]
    pub check: bool,
    /// Target to write; repeatable. Defaults to all four, or claude and codex with --global
    #[arg(long = "target", value_enum)]
    pub targets: Vec<Target>,
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Target {
    Claude,
    Codex,
    #[value(name = "agents_md")]
    AgentsMd,
    #[value(name = "claude_md")]
    ClaudeMd,
}

impl Target {
    fn kind(self) -> SyncKind {
        match self {
            Target::Claude => SyncKind::Claude,
            Target::Codex => SyncKind::Codex,
            Target::AgentsMd => SyncKind::AgentsMd,
            Target::ClaudeMd => SyncKind::ClaudeMd,
        }
    }
}

pub async fn run(args: SyncArgs, backend: &RemoteBackend) -> anyhow::Result<()> {
    // An empty target list is what asks the backend for its own defaults, which
    // differ between a project sync and a global one.
    let request = SyncRequest {
        root: if args.global { None } else { Some(super::abs_path(args.project)?) },
        global: args.global,
        home: args.home.map(|h| super::abs_path(Some(h))).transpose()?,
        targets: args.targets.iter().map(|t| t.kind()).collect(),
        check_only: args.check,
    };
    let report = backend.sync(request).await?;
    if args.check {
        for op in &report.ops {
            println!("{:<9}  {}", label(&op.action), op.path.display());
        }
        // A non-zero exit is the point of --check: it lets a hook or CI job fail
        // when the checked-in agent files no longer match Atlas.
        if report.created + report.updated > 0 {
            std::process::exit(1);
        }
        return Ok(());
    }
    println!("created {}, updated {}, unchanged {}, skipped {}", report.created, report.updated, report.unchanged, report.skipped);
    for op in &report.ops {
        if let SyncAction::Skip(reason) = &op.action {
            println!("skipped {}: {reason}", op.path.display());
        }
    }
    Ok(())
}

fn label(action: &SyncAction) -> String {
    match action {
        SyncAction::Create => "create".into(),
        SyncAction::Update => "update".into(),
        SyncAction::Unchanged => "unchanged".into(),
        SyncAction::Skip(reason) => format!("skip ({reason})"),
    }
}
