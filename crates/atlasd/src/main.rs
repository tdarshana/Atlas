mod http;
mod scheduler;
mod state;
mod worker;

use std::sync::Arc;
use atlas_core::{backend::LocalBackend, paths::AtlasPaths};
use atlas_mcp::AtlasMcp;
use clap::Parser;
use rmcp::transport::streamable_http_server::{session::local::LocalSessionManager, StreamableHttpService, StreamableHttpServerConfig};
use state::AppState;

#[derive(Parser)]
#[command(name = "atlasd", about = "Atlas memory daemon")]
struct Args {
    #[arg(long, env = "ATLAS_PORT", default_value_t = 7433)] port: u16,
    #[arg(long, env = "ATLAS_HOME")] home: Option<std::path::PathBuf>,
    #[arg(long, env = "ATLAS_NO_EMBED")] no_embed: bool,
}

/// The usual reason `LocalBackend::open` fails is that another atlasd holds the DuckDB
/// lock, so name that process from `daemon.json` when the file is there to say who it is.
fn db_open_error(paths: &AtlasPaths, e: atlas_core::AtlasError) -> anyhow::Error {
    let hint = std::fs::read_to_string(paths.daemon_file()).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| format!("; another atlasd may be running (pid {}, port {}, started {})",
            v["pid"].as_u64().map(|p| p.to_string()).unwrap_or_else(|| "unknown".into()),
            v["port"].as_u64().map(|p| p.to_string()).unwrap_or_else(|| "unknown".into()),
            v["started_at"].as_str().unwrap_or("unknown")))
        .unwrap_or_default();
    anyhow::anyhow!("failed to open {}: {e}{hint}", paths.db_path().display())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("atlasd=info".parse()?)).init();
    let args = Args::parse();
    let paths = args.home.as_ref().map(AtlasPaths::at).unwrap_or_else(AtlasPaths::discover);
    paths.ensure()?;
    // Open the database before binding the port. DuckDB's file lock is what resolves two
    // simultaneous starts: the loser fails here, with the pid of the winner, and exits
    // without ever taking the port hostage.
    let mut backend = LocalBackend::open(&paths, Some(args.port), !args.no_embed).map_err(|e| db_open_error(&paths, e))?;

    // One-time move from Markdown workflow documents to real workflows. Idempotent: a
    // second daemon start finds the settings flag already set and does nothing.
    {
        let docs = atlas_core::library::DocRepo::new(&backend.db, atlas_core::models::DocKind::Workflow);
        let settings = atlas_core::settings::SettingsRepo::new(&backend.db);
        match atlas_core::workflow::migrate_docs::migrate_workflow_docs(&docs, &backend.workflows, &settings, "migrate") {
            Ok(0) => {}
            Ok(n) => tracing::info!("migrated {n} workflow document(s) into real workflows"),
            Err(e) => tracing::warn!("workflow document migration failed: {e}"),
        }
    }

    // `--port 0` asks the OS for an ephemeral port; the listener is the only way to learn
    // which one it picked, so bind before anything downstream (daemon.json, the status
    // report, the log line) needs the real port. Non-zero ports bind to the exact number
    // requested, so this changes nothing for them.
    let listener = tokio::net::TcpListener::bind(std::net::SocketAddr::from(([127, 0, 0, 1], args.port))).await?;
    let addr = listener.local_addr()?;
    backend.port = Some(addr.port());
    let backend = Arc::new(backend);
    let state = AppState { backend: backend.clone() };

    // One worker, in this process: it drains the `jobs` table the API writes into,
    // and it is the only consumer, so a job is never claimed twice.
    tokio::spawn(worker::run(backend.clone()));
    // The workflow scheduler, ticking independently: it only enqueues jobs, the worker
    // above still runs them.
    tokio::spawn(scheduler::run(backend.clone(), chrono::Utc::now()));

    let mcp_backend = backend.clone();
    let mcp = StreamableHttpService::new(
        // The daemon serves every project at once, so ATLAS_PROJECT_ROOT in its own
        // environment says nothing about the repository a client is working in and must
        // not scope anyone. HTTP clients name their project in the tool arguments.
        move || Ok(AtlasMcp::new(mcp_backend.clone()).with_env_project_root(false)),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let app = http::guard_loopback(http::router(state).nest_service("/mcp", mcp).layer(http::cors_layer()));

    let info = serde_json::json!({ "pid": std::process::id(), "port": addr.port(), "started_at": chrono::Utc::now().to_rfc3339() });
    std::fs::write(paths.daemon_file(), serde_json::to_string_pretty(&info)?)?;
    tracing::info!("atlasd listening on http://{addr} (db {})", paths.db_path().display());

    let daemon_file = paths.daemon_file();
    axum::serve(listener, app).with_graceful_shutdown(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("sigterm");
            tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
        }
        #[cfg(not(unix))]
        { let _ = ctrl_c.await; }
        let _ = std::fs::remove_file(&daemon_file);
    }).await?;
    Ok(())
}
