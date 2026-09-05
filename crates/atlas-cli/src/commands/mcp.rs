//! `atlas mcp servers` and `atlas mcp check`: the MCP servers the user's agents are wired
//! to, and what one of them actually offers.
//!
//! Bare `atlas mcp` is not here. It stays in `main.rs` and still serves the stdio MCP
//! shim, which is what Claude Code and Codex launch: adding subcommands must not change
//! what the bare command does.

use crate::remote::RemoteBackend;
use atlas_core::backend::{ProjectBackend, McpBackend};
use atlas_core::models::{McpTransport, Project};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum McpCmd {
    /// List the MCP servers the agents use, globally or for one project
    Servers {
        /// A project root path or board key; without it, the user-level servers are listed
        #[arg(long)]
        project: Option<String>,
    },
    /// Start one server and list the tools it offers
    Check {
        /// A server id from `mcp servers`
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
}

pub async fn run(cmd: McpCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        McpCmd::Servers { project } => {
            let project = resolve_project(project.as_deref(), backend).await?;
            let list = backend.list_mcp_servers(project.map(|p| p.id)).await?;
            let rows: Vec<Vec<String>> = list
                .servers
                .iter()
                .map(|s| {
                    vec![
                        s.id.clone(),
                        s.source.to_string(),
                        s.scope.to_string(),
                        if s.enabled { "on".into() } else { "off".into() },
                        describe(&s.transport),
                    ]
                })
                .collect();
            super::print_table(&["ID", "SOURCE", "SCOPE", "STATE", "TRANSPORT"], &rows);
            for warning in &list.warnings {
                eprintln!("warning: {warning}");
            }
        }
        McpCmd::Check { id, project } => {
            let project = resolve_project(project.as_deref(), backend).await?;
            let result = backend.check_mcp_server(project.map(|p| p.id), &id).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            if !result.ok {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

/// One line saying how the server is reached, with no secret in it: the transport a
/// listing carries holds key names only.
fn describe(transport: &McpTransport) -> String {
    match transport {
        McpTransport::Stdio { command, args, env_keys } => {
            let mut line = command.clone();
            for arg in args {
                line.push(' ');
                line.push_str(arg);
            }
            if !env_keys.is_empty() {
                line.push_str(&format!("  (env: {})", env_keys.join(", ")));
            }
            line
        }
        McpTransport::Http { url, header_keys } => {
            if header_keys.is_empty() {
                url.clone()
            } else {
                format!("{url}  (headers: {})", header_keys.join(", "))
            }
        }
    }
}

/// The project an MCP command works on. No argument is not an error: the user-level
/// servers are a real answer, the same rule `atlas skill` uses.
async fn resolve_project(arg: Option<&str>, backend: &RemoteBackend) -> anyhow::Result<Option<Project>> {
    let Some(arg) = arg else { return Ok(None) };
    let projects = backend.list_projects().await?;
    if let Some(p) = projects.iter().find(|p| p.board_key.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(arg))) {
        return Ok(Some(p.clone()));
    }
    let abs = super::abs_path(Some(PathBuf::from(arg)))?;
    let abs = abs.to_string_lossy().to_string();
    projects
        .into_iter()
        .find(|p| p.root_path == arg || p.root_path == abs)
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("no project with root or board key '{arg}'"))
}
