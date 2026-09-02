mod daemon_ctl;
mod remote;

use std::sync::Arc;
use atlas_core::{models::*, paths::AtlasPaths};
use atlas_mcp::AtlasMcp;
use clap::{Parser, Subcommand};
use remote::RemoteBackend;
use atlas_core::backend::Backend;

#[derive(Parser)]
#[command(name = "atlas", about = "Shared memory and agents for coding agents", version)]
struct Cli {
    #[arg(long, global = true, env = "ATLAS_PORT", default_value_t = 7433)] port: u16,
    #[arg(long, global = true, env = "ATLAS_HOME")] home: Option<std::path::PathBuf>,
    #[command(subcommand)] cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Control the atlasd daemon
    Daemon { #[command(subcommand)] action: DaemonCmd },
    /// Run an MCP server on stdio (for Claude Code, Codex, other MCP clients); starts the daemon if needed
    Mcp,
    /// Store a memory
    Remember { text: String, #[arg(long, default_value = "fact")] kind: String, #[arg(long = "tag")] tags: Vec<String>, #[arg(long)] project_id: Option<uuid::Uuid>, #[arg(long)] agent: Option<String> },
    /// Search memories
    Recall { query: String, #[arg(long, default_value_t = 10)] limit: usize, #[arg(long = "kind")] kinds: Vec<String>, #[arg(long = "tag")] tags: Vec<String>, #[arg(long)] project_id: Option<uuid::Uuid> },
}

#[derive(Subcommand)]
enum DaemonCmd { Start, Stop, Status }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_writer(std::io::stderr).init();
    let cli = Cli::parse();
    let paths = cli.home.as_ref().map(AtlasPaths::at).unwrap_or_else(AtlasPaths::discover);
    match cli.cmd {
        Cmd::Daemon { action: DaemonCmd::Start } => { let p = daemon_ctl::ensure_daemon(&paths, cli.port).await?; println!("atlasd running on http://127.0.0.1:{p}"); }
        Cmd::Daemon { action: DaemonCmd::Stop } => { println!("{}", if daemon_ctl::stop_daemon(&paths)? { "stopped" } else { "not running" }); }
        Cmd::Daemon { action: DaemonCmd::Status } => {
            if daemon_ctl::is_up(cli.port).await { let s = RemoteBackend::new(cli.port).status().await?; println!("{}", serde_json::to_string_pretty(&s)?); }
            else { println!("atlasd is not running on port {}", cli.port); std::process::exit(1); }
        }
        Cmd::Mcp => {
            let port = daemon_ctl::ensure_daemon(&paths, cli.port).await?;
            if std::env::var("ATLAS_SOURCE_TOOL").is_err() { std::env::set_var("ATLAS_SOURCE_TOOL", "stdio"); }
            let server = AtlasMcp::new(Arc::new(RemoteBackend::new(port)));
            use rmcp::ServiceExt;
            let running = server.serve(rmcp::transport::stdio()).await?;
            running.waiting().await?;
        }
        Cmd::Remember { text, kind, tags, project_id, agent } => {
            let port = daemon_ctl::ensure_daemon(&paths, cli.port).await?;
            let scope = if project_id.is_some() { MemoryScope::Project } else { MemoryScope::Global };
            let m = NewMemory { scope, project_id, kind: kind.parse()?, text, tags, source_agent: agent, source_tool: Some("cli".into()), confidence: 1.0, status: MemoryStatus::Active };
            let saved = RemoteBackend::new(port).remember(m, "cli").await?;
            println!("{}", serde_json::to_string_pretty(&saved)?);
        }
        Cmd::Recall { query, limit, kinds, tags, project_id } => {
            let port = daemon_ctl::ensure_daemon(&paths, cli.port).await?;
            let mut ks = vec![]; for k in kinds { ks.push(k.parse::<MemoryKind>()?); }
            let hits = RemoteBackend::new(port).recall(RecallQuery { query, limit, scope: None, project_id, kinds: ks, tags }).await?;
            for h in hits { println!("{:.2}  [{}] {}  {}", h.score, h.memory.kind, h.memory.text, if h.memory.tags.is_empty() { String::new() } else { format!("#{}", h.memory.tags.join(" #")) }); }
        }
    }
    Ok(())
}
