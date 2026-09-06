//! The daemon tests' view of `atlas_testkit`: the shared harness plus the two boot
//! helpers bound to this crate's own `atlasd` binary.

#![allow(dead_code)]

pub use atlas_testkit::*;
use std::path::Path;

/// A fresh daemon with no extra environment.
pub async fn start() -> Daemon { start_with_env(&[]).await }

/// A daemon with extra environment variables, for the settings the daemon reads at
/// call time rather than from its arguments (`ATLAS_SYNC_HOME`).
pub async fn start_with_env(env: &[(&str, &str)]) -> Daemon {
    Daemon::spawn(Path::new(env!("CARGO_BIN_EXE_atlasd")), env).await
}
