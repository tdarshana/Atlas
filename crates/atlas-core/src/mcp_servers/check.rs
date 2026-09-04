//! Starting a server and asking it what it can do.
//!
//! A check runs the user's own configured command with the user's own configured
//! environment, so it is always an explicit action, never part of a listing. It is capped
//! at [`CHECK_TIMEOUT`] end to end and the child is killed when the check returns: the
//! transport's own `Drop` kills it, which covers the timeout path too, since a timed-out
//! future is dropped where it stands.
//!
//! Nothing here can fail the call. A server that will not start, speaks nonsense or never
//! answers is an answer with `ok: false` and the reason in `error`.

use std::process::Stdio;
use std::time::{Duration, Instant};

use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::ServiceExt;

use crate::models::{McpCheckResult, McpToolInfo, McpTransport};

use super::Resolved;

/// How long a check may take in total: starting the process, the `initialize` handshake
/// and `tools/list`. Long enough for an `npx` server that has to fetch its package on
/// first run, short enough that a wedged server does not hold a desktop button down.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Checks one server. The secret values in `resolved` are passed to the child process or
/// sent as request headers and never appear in the result.
pub async fn run(resolved: &Resolved) -> McpCheckResult {
    let start = Instant::now();
    let outcome = tokio::time::timeout(CHECK_TIMEOUT, probe(resolved)).await;
    let elapsed_ms = start.elapsed().as_millis() as u64;
    match outcome {
        Ok(Ok(result)) => McpCheckResult { elapsed_ms, ..result },
        Ok(Err(error)) => McpCheckResult { ok: false, error: Some(error), elapsed_ms, ..Default::default() },
        Err(_) => McpCheckResult {
            ok: false,
            error: Some(format!("the server did not answer within {}s", CHECK_TIMEOUT.as_secs())),
            elapsed_ms,
            ..Default::default()
        },
    }
}

/// One handshake and one `tools/list`, over whichever transport the entry names.
async fn probe(resolved: &Resolved) -> Result<McpCheckResult, String> {
    match &resolved.entry.transport {
        McpTransport::Stdio { command, args, .. } => {
            let mut cmd = tokio::process::Command::new(command);
            cmd.args(args);
            for (key, value) in &resolved.env {
                cmd.env(key, value);
            }
            // The child's stderr goes nowhere rather than into the daemon's own, which is
            // the default: a checked server's diagnostics are not Atlas's log. It is not
            // piped either, since nothing here would drain the pipe.
            let (transport, _) = TokioChildProcess::builder(cmd)
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("{command} could not be started: {e}"))?;
            let service = ().serve(transport).await.map_err(|e| e.to_string())?;
            interrogate(service).await
        }
        McpTransport::Http { url, .. } => {
            let mut config = rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url.clone());
            for (key, value) in &resolved.headers {
                let name = reqwest::header::HeaderName::try_from(key.as_str()).map_err(|e| format!("header '{key}': {e}"))?;
                let value = reqwest::header::HeaderValue::try_from(value.as_str()).map_err(|e| format!("header '{key}': {e}"))?;
                config.custom_headers.insert(name, value);
            }
            let transport = StreamableHttpClientTransport::with_client(reqwest::Client::new(), config);
            let service = ().serve(transport).await.map_err(|e| e.to_string())?;
            interrogate(service).await
        }
    }
}

/// The part that is the same for both transports: read back who answered the handshake,
/// list the tools, then shut the connection down. The shutdown is best effort, since a
/// server that answered is a successful check whether or not it closes politely.
async fn interrogate(service: rmcp::service::RunningService<rmcp::RoleClient, ()>) -> Result<McpCheckResult, String> {
    let info = service.peer_info();
    let tools = service.list_all_tools().await.map_err(|e| e.to_string());
    let _ = service.cancel().await;
    let tools = tools?;
    Ok(McpCheckResult {
        ok: true,
        server_name: info.as_ref().and_then(|i| i.server_info.as_ref()).map(|s| s.name.to_string()),
        server_version: info.as_ref().and_then(|i| i.server_info.as_ref()).map(|s| s.version.to_string()),
        protocol_version: info.as_ref().map(|i| i.protocol_version.to_string()),
        tools: tools
            .into_iter()
            .map(|t| McpToolInfo { name: t.name.to_string(), description: t.description.map(|d| d.to_string()) })
            .collect(),
        error: None,
        elapsed_ms: 0,
    })
}
