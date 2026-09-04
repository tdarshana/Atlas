pub mod backend;
pub mod board;
pub mod db;
pub mod error;
pub mod export;
pub mod extract;
pub mod frameworks;
pub mod jobs;
pub mod library;
pub mod llm;
pub mod mcp_servers;
pub mod memories;
pub mod models;
pub mod paths;
pub mod plugin_tools;
pub mod projects;
pub mod search;
pub mod service;
pub mod settings;
pub mod skills;
pub mod sync;
pub mod workflow;
pub use error::{AtlasError, Result};

/// Shared test fixture helpers (`tests/common/mod.rs`). Declared here rather
/// than with `#[path]` inside `src/projects/{detect,profile}.rs` directly:
/// an inline `mod tests { .. }` block owns a pseudo-directory named after
/// itself for `#[path]` resolution, and since that directory doesn't exist
/// on disk the OS can't resolve a `../..`-style path out of it. `lib.rs`'s
/// owning directory is the real `src/`, so a single `../tests/common/mod.rs`
/// resolves cleanly from here.
#[cfg(test)]
#[path = "../tests/common/mod.rs"]
pub(crate) mod test_common;
