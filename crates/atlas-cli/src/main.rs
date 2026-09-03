use std::path::PathBuf;
use std::sync::Arc;
use atlas_cli::{commands, daemon_ctl, remote::RemoteBackend};
use atlas_core::backend::Backend;
use atlas_core::{models::*, paths::AtlasPaths};
use atlas_mcp::AtlasMcp;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "atlas", about = "Shared memory and agents for coding agents", version)]
struct Cli {
    #[arg(long, global = true, env = "ATLAS_PORT", default_value_t = 7433)] port: u16,
    #[arg(long, global = true, env = "ATLAS_HOME")] home: Option<PathBuf>,
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
    /// Connect and inspect projects
    Project { #[command(subcommand)] action: commands::project::ProjectCmd },
    /// Manage the agents Atlas exports to Claude Code and Codex
    Agent { #[command(subcommand)] action: commands::agent::AgentCmd },
    /// Work with the task board
    Task {
        /// Record this command's writes as `cli/NAME` instead of `cli`
        #[arg(long = "as", global = true)] actor: Option<String>,
        #[command(subcommand)] action: commands::board::TaskCmd,
    },
    /// Inspect and set the board's stages
    Board {
        /// Record this command's writes as `cli/NAME` instead of `cli`
        #[arg(long = "as", global = true)] actor: Option<String>,
        #[command(subcommand)] action: commands::board::BoardCmd,
    },
    /// Manage practices
    Practice { #[command(subcommand)] action: commands::doc::DocCmd },
    /// Manage workflows
    Workflow { #[command(subcommand)] action: commands::doc::DocCmd },
    /// Write agent files and managed instruction blocks into a project or the home directory
    Sync(commands::sync::SyncArgs),
    /// Write the whole library to DIR as JSONL and Markdown
    Export {
        dir: PathBuf,
        /// Empty DIR's agents/, practices/ and workflows/ even when they hold files this export did not write
        #[arg(long)]
        force: bool,
    },
    /// Read a directory written by `atlas export` back into Atlas
    Import { dir: PathBuf },
    /// Queue a transcript for extraction; the hook modes read Claude Code's and Codex's own payloads
    Ingest(commands::ingest::IngestArgs),
    /// Open the terminal UI
    Tui,
}

#[derive(Subcommand)]
enum DaemonCmd { Start, Stop, Status }

/// Starts the daemon if it is not already up and returns a client for it.
async fn backend(paths: &AtlasPaths, port: u16) -> anyhow::Result<RemoteBackend> {
    Ok(RemoteBackend::new(daemon_ctl::ensure_daemon(paths, port).await?))
}

/// A client for the board commands, whose reads and writes are both recorded
/// under `--as`.
async fn board_backend(paths: &AtlasPaths, port: u16, actor: Option<String>) -> anyhow::Result<RemoteBackend> {
    let mut backend = backend(paths, port).await?;
    backend.actor = commands::board::actor(actor.as_deref());
    Ok(backend)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_writer(std::io::stderr).init();
    let cli = Cli::parse();
    let paths = cli.home.as_ref().map(AtlasPaths::at).unwrap_or_else(AtlasPaths::discover);
    match cli.cmd {
        Cmd::Daemon { action: DaemonCmd::Start } => { let p = daemon_ctl::ensure_daemon(&paths, cli.port).await?; println!("atlasd running on http://127.0.0.1:{p}"); }
        Cmd::Daemon { action: DaemonCmd::Stop } => { println!("{}", if daemon_ctl::stop_daemon(&paths).await? { "stopped" } else { "not running" }); }
        Cmd::Daemon { action: DaemonCmd::Status } => {
            if daemon_ctl::is_up(cli.port).await { let s = RemoteBackend::new(cli.port).status().await?; println!("{}", serde_json::to_string_pretty(&s)?); }
            else { println!("atlasd is not running on port {}", cli.port); std::process::exit(1); }
        }
        Cmd::Mcp => {
            let port = daemon_ctl::ensure_daemon(&paths, cli.port).await?;
            let mut server = AtlasMcp::new(Arc::new(RemoteBackend::new(port)))
                .with_source_tool(std::env::var("ATLAS_SOURCE_TOOL").unwrap_or_else(|_| "stdio".into()));
            // The client launches the shim in the repository it is working in, so the cwd
            // names the project. Handed over rather than exported: writing an env var is
            // unsound once the process is multi-threaded, which it already is here.
            if let Ok(cwd) = std::env::current_dir() { server = server.with_project_root(cwd); }
            use rmcp::ServiceExt;
            let running = server.serve(rmcp::transport::stdio()).await?;
            running.waiting().await?;
        }
        Cmd::Remember { text, kind, tags, project_id, agent } => {
            let scope = if project_id.is_some() { MemoryScope::Project } else { MemoryScope::Global };
            let m = NewMemory { scope, project_id, kind: kind.parse()?, text, tags, source_agent: agent, source_tool: Some("cli".into()), confidence: 1.0, status: MemoryStatus::Active };
            let saved = backend(&paths, cli.port).await?.remember(m, "cli").await?;
            println!("{}", serde_json::to_string_pretty(&saved)?);
        }
        Cmd::Recall { query, limit, kinds, tags, project_id } => {
            let mut ks = vec![]; for k in kinds { ks.push(k.parse::<MemoryKind>()?); }
            let hits = backend(&paths, cli.port).await?.recall(RecallQuery { query, limit, scope: None, list_scope: atlas_core::models::MemoryScopeFilter::All, project_id, kinds: ks, tags }).await?;
            for h in hits { println!("{:.2}  [{}] {}  {}", h.score, h.memory.kind, h.memory.text, if h.memory.tags.is_empty() { String::new() } else { format!("#{}", h.memory.tags.join(" #")) }); }
        }
        Cmd::Project { action } => commands::project::run(action, &backend(&paths, cli.port).await?).await?,
        Cmd::Agent { action } => commands::agent::run(action, &backend(&paths, cli.port).await?).await?,
        // Board reads carry the actor too: it travels in a header, not a per-call
        // argument, so it has to be on the client before the first call.
        Cmd::Task { actor, action } => commands::board::run_task(action, &board_backend(&paths, cli.port, actor).await?).await?,
        Cmd::Board { actor, action } => commands::board::run_board(action, &board_backend(&paths, cli.port, actor).await?).await?,
        Cmd::Practice { action } => commands::doc::run(DocKind::Practice, action, &backend(&paths, cli.port).await?).await?,
        Cmd::Workflow { action } => commands::doc::run(DocKind::Workflow, action, &backend(&paths, cli.port).await?).await?,
        Cmd::Sync(args) => commands::sync::run(args, &backend(&paths, cli.port).await?).await?,
        Cmd::Export { dir, force } => commands::export::run(dir, force, &backend(&paths, cli.port).await?).await?,
        Cmd::Import { dir } => commands::import::run(dir, &backend(&paths, cli.port).await?).await?,
        // `ingest` reaches the daemon itself: in hook mode a daemon that will not start
        // has to be reported the same quiet way as any other failure.
        Cmd::Ingest(args) => commands::ingest::run(args, &paths, cli.port).await?,
        Cmd::Tui => {
            let port = daemon_ctl::ensure_daemon(&paths, cli.port).await?;
            atlas_cli::tui::run(port).await?;
        }
    }
    Ok(())
}
