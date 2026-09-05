//! `atlas workflow`: list and inspect workflows, start and follow their runs.

use crate::remote::RemoteBackend;
use atlas_core::backend::{ProjectBackend, WorkflowBackend};
use atlas_core::models::{TriggerKind, WorkflowRun};
use clap::Subcommand;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

#[derive(Subcommand)]
pub enum WorkflowCmd {
    /// List workflows, optionally only those scoped to a project
    List {
        /// Limit to the project at PATH
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Print one workflow as JSON
    Show { name: String },
    /// Start a run of a workflow
    Run {
        name: String,
        /// Text for the first action's "Input:" line, when the workflow expects one
        #[arg(long)]
        input: Option<String>,
        /// Poll until the run finishes and print its final state
        #[arg(long)]
        wait: bool,
        /// Give up waiting after this many seconds (only with --wait)
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },
    /// List a workflow's runs, newest first
    Runs {
        name: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Print a run's full log
    Log { run_id: Uuid },
    /// Cancel a queued or running run
    Cancel { run_id: Uuid },
}

pub async fn run(cmd: WorkflowCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        WorkflowCmd::List { project } => {
            let project_id = match project {
                Some(p) => Some(backend.connect_project(super::abs_path(Some(p))?, "cli").await?.id),
                None => None,
            };
            let workflows = backend.list_workflows(project_id).await?;
            let rows: Vec<Vec<String>> = workflows
                .iter()
                .map(|w| {
                    vec![
                        w.name.clone(),
                        w.trigger.kind.to_string(),
                        if w.enabled { "yes".into() } else { "no".into() },
                        w.last_status.map(|s| s.to_string()).unwrap_or_else(|| "-".into()),
                    ]
                })
                .collect();
            super::print_table(&["NAME", "TRIGGER", "ENABLED", "LAST STATUS"], &rows);
            Ok(())
        }
        WorkflowCmd::Show { name } => super::print_json(&backend.get_workflow(&name).await?),
        WorkflowCmd::Run { name, input, wait, timeout } => {
            let run = backend.run_workflow(&name, TriggerKind::Manual, "cli", input).await?;
            println!("started run #{} ({})", run.number, run.id);
            if wait {
                let finished = poll_until_done(backend, run.id, Duration::from_secs(timeout))
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("timed out waiting for run {}", run.number))?;
                println!("{}", serde_json::to_string_pretty(&finished)?);
            }
            Ok(())
        }
        WorkflowCmd::Runs { name, limit } => {
            let runs = backend.list_runs(&name, limit).await?;
            let rows: Vec<Vec<String>> =
                runs.iter().map(|r| vec![r.number.to_string(), r.trigger.to_string(), r.status.to_string(), r.started_at.to_rfc3339()]).collect();
            super::print_table(&["#", "TRIGGER", "STATUS", "STARTED"], &rows);
            Ok(())
        }
        WorkflowCmd::Log { run_id } => {
            print!("{}", backend.export_run_log(run_id).await?);
            Ok(())
        }
        WorkflowCmd::Cancel { run_id } => {
            let run = backend.cancel_run(run_id, "cli").await?;
            println!("cancelled run #{}", run.number);
            Ok(())
        }
    }
}

/// Polls `GET /runs/{id}` every half second until the run reaches a terminal status,
/// or `None` once `timeout` has passed without one.
async fn poll_until_done(backend: &RemoteBackend, run_id: Uuid, timeout: Duration) -> anyhow::Result<Option<WorkflowRun>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let (run, _) = backend.get_run(run_id).await?;
        if run.status.is_terminal() {
            return Ok(Some(run));
        }
        if std::time::Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
