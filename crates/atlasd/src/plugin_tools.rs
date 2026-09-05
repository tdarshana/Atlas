//! The loopback channel a desktop plugin's MCP tools are served over.
//!
//! The desktop app opens one WebSocket to `GET /api/v1/mcp/plugin-channel` and keeps it
//! open. Plugins register their tool tables over the JSON API
//! (`PUT /api/v1/mcp/plugin-tools/{plugin_id}`); the daemon lists them to every MCP
//! client and, when one is called, sends a request frame down the socket and waits for
//! the app's answer. Nothing here opens the database: a plugin tool call is a message
//! forwarded to another process, and the audit row it earns is written by
//! `LocalBackend::call_plugin_tool`, on the same path every built-in write takes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use atlas_core::models::PluginToolDecl;
use atlas_core::plugin_tools::{BoxFuture, PluginToolHost};
use atlas_core::{AtlasError, Result};
use axum::extract::ws::{Message, WebSocket};
use tokio::sync::{mpsc, oneshot};

/// How long a forwarded call waits for the app before it gives up. Well past any
/// healthy plugin and well short of the MCP client timeouts that would otherwise be the
/// only thing to end the wait.
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The app end of the channel, while it is connected.
struct Socket {
    /// Which connection this is. A pending call remembers the generation it was sent
    /// on, so a reply arriving after a replacement can never resolve a newer call.
    generation: u64,
    /// Frames waiting to go out, drained by the socket's own task. Unbounded because
    /// every send is one small JSON frame and a bound here would mean blocking a tool
    /// call on the socket's write half.
    out: mpsc::UnboundedSender<String>,
}

#[derive(Default)]
struct Inner {
    /// Each plugin's declared tools, replaced wholesale by a registration.
    registry: HashMap<String, Vec<PluginToolDecl>>,
    socket: Option<Socket>,
    /// Calls sent and not yet answered, by request id.
    pending: HashMap<u64, (u64, oneshot::Sender<Result<serde_json::Value>>)>,
}

pub struct PluginToolChannel {
    inner: Mutex<Inner>,
    next_id: AtomicU64,
    next_generation: AtomicU64,
}

impl Default for PluginToolChannel {
    fn default() -> Self { Self::new() }
}

impl PluginToolChannel {
    pub fn new() -> Self {
        Self { inner: Mutex::new(Inner::default()), next_id: AtomicU64::new(1), next_generation: AtomicU64::new(1) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Replaces `plugin_id`'s whole tool set. Validation happens here, not in the route,
    /// so nothing an MCP client will be offered can enter the registry unchecked however
    /// many callers this grows.
    pub fn register(&self, plugin_id: String, tools: Vec<PluginToolDecl>) -> Result<()> {
        atlas_core::settings::validate_plugin_id(&plugin_id)?;
        atlas_core::settings::validate_plugin_tool_decls(&tools)?;
        if tools.iter().any(|t| t.plugin_id != plugin_id) {
            return Err(AtlasError::Invalid(format!("every tool registered under {plugin_id} must carry that plugin id")));
        }
        self.lock().registry.insert(plugin_id, tools);
        Ok(())
    }

    /// Drops a plugin's tools. Silent when it had none: the app unregistering a plugin
    /// it never registered is not an error worth reporting.
    pub fn unregister(&self, plugin_id: &str) {
        self.lock().registry.remove(plugin_id);
    }

    /// Fails every pending call with `reason` and, when `clear_registry` is set, forgets
    /// every plugin's tools: an app that is gone contributes no tools, so they must stop
    /// being listed the moment the socket closes.
    fn drop_pending(&self, generation: u64, reason: &str, clear_registry: bool) {
        let waiters: Vec<_> = {
            let mut inner = self.lock();
            if clear_registry {
                inner.registry.clear();
            }
            let ids: Vec<u64> = inner.pending.iter().filter(|(_, (g, _))| *g == generation).map(|(id, _)| *id).collect();
            ids.into_iter().filter_map(|id| inner.pending.remove(&id)).map(|(_, tx)| tx).collect()
        };
        for tx in waiters {
            let _ = tx.send(Err(AtlasError::Invalid(reason.to_string())));
        }
    }

    /// Serves one app connection until it closes. A second connection replaces the
    /// first, whose pending calls fail rather than hanging until their own timeout: two
    /// desktop windows both opening the channel is a user's doing, not an error, and the
    /// newest one is the live app.
    pub async fn serve(self: Arc<Self>, mut socket: WebSocket, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let (out, mut rx) = mpsc::unbounded_channel::<String>();
        let replaced = self.lock().socket.replace(Socket { generation, out });
        if let Some(old) = replaced {
            let old_generation = old.generation;
            // Dropping the predecessor's `out` sender is what ends its task rather than
            // leaving it parked until its client happens to hang up: the registry held
            // the only sender, so its `rx.recv()` arm sees the channel close, breaks, and
            // drops its socket.
            drop(old);
            self.drop_pending(old_generation, "plugin channel replaced", false);
        }

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() { break; }
                }
                frame = rx.recv() => match frame {
                    Some(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() { break; }
                    }
                    None => break,
                },
                incoming = socket.recv() => match incoming {
                    Some(Ok(Message::Text(text))) => self.answer(generation, &text),
                    Some(Ok(Message::Close(_))) | None => break,
                    // Binary frames are not part of the protocol; ping and pong are
                    // answered by axum itself and mean nothing here.
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::debug!("plugin channel read failed: {e}");
                        break;
                    }
                },
            }
        }

        // Only tear down if this connection is still the live one: a replacement has
        // already taken over the slot and must not be evicted by its predecessor's exit.
        let still_current = {
            let mut inner = self.lock();
            match inner.socket.as_ref().map(|s| s.generation) {
                Some(g) if g == generation => { inner.socket = None; true }
                _ => false,
            }
        };
        if still_current {
            self.drop_pending(generation, "the plugin channel closed", true);
        }
    }

    /// Resolves one pending call from an app reply. A frame that names no pending id (a
    /// late answer, a reply on a replaced connection, or a malformed one) is dropped
    /// with a debug line: there is nobody left to tell.
    fn answer(&self, generation: u64, text: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            tracing::debug!("plugin channel sent a frame that was not JSON");
            return;
        };
        let Some(id) = value.get("id").and_then(serde_json::Value::as_u64) else {
            tracing::debug!("plugin channel sent a frame with no id");
            return;
        };
        let waiter = {
            let mut inner = self.lock();
            match inner.pending.get(&id) {
                Some((g, _)) if *g == generation => inner.pending.remove(&id).map(|(_, tx)| tx),
                _ => None,
            }
        };
        let Some(tx) = waiter else { return };
        let answer = if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
            Ok(value.get("result").cloned().unwrap_or(serde_json::Value::Null))
        } else {
            Err(AtlasError::Invalid(value.get("error").and_then(|v| v.as_str()).unwrap_or("the plugin reported an error").to_string()))
        };
        let _ = tx.send(answer);
    }

    /// The whole registry, flattened. Inherent as well as the trait method, so the HTTP
    /// routes can read it without importing `PluginToolHost`.
    pub fn list(&self) -> Vec<PluginToolDecl> {
        self.lock().registry.values().flatten().cloned().collect()
    }

}

impl PluginToolHost for PluginToolChannel {
    fn list(&self) -> Vec<PluginToolDecl> { PluginToolChannel::list(self) }

    fn call(&self, plugin_id: &str, name: &str, args: serde_json::Value) -> BoxFuture<'_, Result<serde_json::Value>> {
        let plugin_id = plugin_id.to_string();
        let name = name.to_string();
        Box::pin(async move {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            {
                let mut inner = self.lock();
                // The socket is checked before the registry, and deliberately: a socket
                // that closed took the registry with it, so "plugin x is not running" is
                // the truthful answer then, not "unknown plugin tool".
                let Some(socket) = inner.socket.as_ref() else {
                    return Err(AtlasError::Invalid(format!("plugin {plugin_id} is not running")));
                };
                let generation = socket.generation;
                let out = socket.out.clone();
                // The plugin id may arrive from an MCP name with its dashes already
                // turned into underscores, so a registered id matches either form.
                let registered_id = inner.registry.iter()
                    .find(|(id, tools)| (id.as_str() == plugin_id || id.replace('-', "_") == plugin_id) && tools.iter().any(|t| t.name == name))
                    .map(|(id, _)| id.clone());
                let Some(registered_id) = registered_id else {
                    return Err(AtlasError::Invalid(format!("unknown plugin tool {}", atlas_mcp::plugin_tool_name(&plugin_id, &name))));
                };
                let frame = serde_json::json!({ "id": id, "plugin_id": registered_id, "tool": name, "args": args }).to_string();
                if out.send(frame).is_err() {
                    return Err(AtlasError::Invalid(format!("plugin {plugin_id} is not running")));
                }
                inner.pending.insert(id, (generation, tx));
            }
            match tokio::time::timeout(CALL_TIMEOUT, rx).await {
                Ok(Ok(answer)) => answer,
                // The sender was dropped without answering, which only happens if the
                // pending entry was removed by something that did not send: treat it the
                // same as a closed channel.
                Ok(Err(_)) => Err(AtlasError::Invalid("the plugin channel closed".into())),
                Err(_) => {
                    self.lock().pending.remove(&id);
                    Err(AtlasError::Invalid(format!("plugin {plugin_id} did not answer within 30s")))
                }
            }
        })
    }
}
