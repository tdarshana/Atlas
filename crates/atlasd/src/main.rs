mod http;
mod state;

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
    let backend = Arc::new(LocalBackend::open(&paths, Some(args.port), !args.no_embed).map_err(|e| db_open_error(&paths, e))?);
    let state = AppState { backend: backend.clone() };

    let mcp_backend = backend.clone();
    let mcp = StreamableHttpService::new(move || Ok(AtlasMcp::new(mcp_backend.clone())), LocalSessionManager::default().into(), StreamableHttpServerConfig::default());
    let app = http::guard_loopback(http::router(state).nest_service("/mcp", mcp));

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let info = serde_json::json!({ "pid": std::process::id(), "port": args.port, "started_at": chrono::Utc::now().to_rfc3339() });
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
