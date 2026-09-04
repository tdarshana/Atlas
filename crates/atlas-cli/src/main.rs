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
    /// Inspect workflows and run them
    Workflow { #[command(subcommand)] action: commands::workflow::WorkflowCmd },
    /// List, read and import a project's detected planning frameworks
    Framework { #[command(subcommand)] action: commands::framework::FrameworkCmd },
    /// List, read and gate the skills agents can use
    Skill { #[command(subcommand)] action: commands::skill::SkillCmd },
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

/// Best-effort name of the process that launched this one, for a stdio MCP client
/// registration whose `initialize` handshake carried no client name and whose
/// environment carries no `CLAUDE_CODE` hint either. Shells out to `ps` twice (own pid
/// to parent pid, then parent pid to its command name) since there is no portable std
/// API for a parent's name; `None` on any failure, for the caller to fall back further.
/// Run on `spawn_blocking` since both `Command::output` calls block the thread they
/// run on and this fires during shim startup, on the same runtime that is about to
/// serve stdio traffic.
#[cfg(unix)]
async fn parent_process_name() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        let ppid_out = std::process::Command::new("ps").args(["-o", "ppid=", "-p", &std::process::id().to_string()]).output().ok()?;
        let ppid = String::from_utf8_lossy(&ppid_out.stdout).trim().to_string();
        if ppid.is_empty() { return None; }
        let comm_out = std::process::Command::new("ps").args(["-o", "comm=", "-p", &ppid]).output().ok()?;
        let comm = String::from_utf8_lossy(&comm_out.stdout).trim().to_string();
        // `comm=` reports the full launch path on macOS; keep just the file name for a
        // readable client label.
        let name = std::path::Path::new(&comm).file_name().and_then(|s| s.to_str()).unwrap_or(&comm).to_string();
        (!name.is_empty()).then_some(name)
    }).await.ok().flatten()
}
#[cfg(not(unix))]
async fn parent_process_name() -> Option<String> { None }

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
            let remote = Arc::new(RemoteBackend::new(port));
            // Counts this session's tool calls locally rather than reaching the daemon
            // on every one: the heartbeat task below reports the running total every
            // 60 s, which is precise enough for a status display and costs nothing on
            // the tool-call path itself.
            let tool_calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let hook_calls = tool_calls.clone();
            let mut server = AtlasMcp::new(remote.clone())
                .with_source_tool(std::env::var("ATLAS_SOURCE_TOOL").unwrap_or_else(|_| "stdio".into()))
                .with_on_tool_call(Arc::new(move |_session_id, _client_info, _protocol_version, _project_id| {
                    hook_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }));
            // The client launches the shim in the repository it is working in, so the cwd
            // names the project. Handed over rather than exported: writing an env var is
            // unsound once the process is multi-threaded, which it already is here.
            if let Ok(cwd) = std::env::current_dir() { server = server.with_project_root(cwd); }
            use rmcp::ServiceExt;
            let running = server.serve(rmcp::transport::stdio()).await?;

            // Registers with the daemon so `GET /api/v1/mcp/status` can show this
            // session. Best effort throughout: a client this shim serves must keep
            // working even when the daemon's registry route is unreachable.
            let client_id = uuid::Uuid::new_v4().to_string();
            let peer_info = running.peer_info();
            let client_name = match peer_info.as_ref()
                .map(|info| info.client_info.name.clone())
                .filter(|n| !n.is_empty())
                .or_else(|| std::env::var("CLAUDE_CODE").is_ok().then(|| "claude-code".to_string()))
            {
                Some(n) => n,
                None => parent_process_name().await.unwrap_or_else(|| "unknown".to_string()),
            };
            let client_version = peer_info.and_then(|info| (!info.client_info.version.is_empty()).then(|| info.client_info.version.clone()));
            if let Err(e) = remote.register_mcp_client(&client_id, "stdio", &client_name, client_version.as_deref()).await {
                tracing::debug!("mcp client registration failed: {e}");
            }
            let heartbeat = {
                let remote = remote.clone();
                let client_id = client_id.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                    interval.tick().await; // the first tick fires immediately; skip it
                    loop {
                        interval.tick().await;
                        let calls = tool_calls.load(std::sync::atomic::Ordering::Relaxed);
                        // A 404 means the daemon does not know this id, either because the
                        // initial registration above failed or because a daemon restart
                        // dropped the entry: re-register with the same id rather than
                        // heartbeat forever a session that will never show up in the
                        // status view again.
                        if let Err(atlas_core::AtlasError::NotFound(_)) = remote.heartbeat_mcp_client(&client_id, calls).await {
                            let _ = remote.register_mcp_client(&client_id, "stdio", &client_name, client_version.as_deref()).await;
                        }
                    }
                })
            };

            running.waiting().await?;
            heartbeat.abort();
            let _ = remote.unregister_mcp_client(&client_id).await;
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
        Cmd::Workflow { action } => commands::workflow::run(action, &backend(&paths, cli.port).await?).await?,
        Cmd::Framework { action } => commands::framework::run(action, &backend(&paths, cli.port).await?).await?,
        Cmd::Skill { action } => commands::skill::run(action, &backend(&paths, cli.port).await?).await?,
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
