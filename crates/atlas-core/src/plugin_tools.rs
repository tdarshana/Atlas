//! The seam a desktop plugin's MCP tools reach Atlas through.
//!
//! `LocalBackend` knows nothing about plugins, WebSockets or the desktop app: it holds
//! an optional [`PluginToolHost`] and asks it what tools exist and to run one. The
//! daemon supplies the only real implementation (`atlasd::plugin_tools::
//! PluginToolChannel`, a loopback WebSocket to the app); a test supplies a stub. With no
//! host set, `Backend::plugin_tools` is empty and `Backend::call_plugin_tool` refuses,
//! which is what the CLI's in-process uses and every non-daemon caller sees.

use crate::models::PluginToolDecl;
use crate::Result;

/// A boxed future, spelled out here rather than pulled from `futures`: the one method
/// that needs it is object-safe only in this form, and the crate has no other reason to
/// depend on `futures`.
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Whoever can answer for the plugins currently running. Both methods are cheap and
/// synchronous to call; `call` returns a future because forwarding a tool call means
/// waiting on another process.
pub trait PluginToolHost: Send + Sync {
    /// Every tool every registered plugin declares, in no particular order.
    fn list(&self) -> Vec<PluginToolDecl>;
    /// Forwards one call to the plugin that declared it. `Invalid` when the plugin is
    /// not running, does not declare the tool, or does not answer in time; the plugin's
    /// own failure comes back as `Invalid` with the message the plugin sent.
    fn call(&self, plugin_id: &str, name: &str, args: serde_json::Value) -> BoxFuture<'_, Result<serde_json::Value>>;
}
