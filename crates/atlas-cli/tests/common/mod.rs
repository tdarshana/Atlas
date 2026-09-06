//! The CLI tests' view of `atlas_testkit`: the shared fixture repository plus the
//! daemon harness bound to this crate's own `atlas` binary.

#![allow(unused_imports)]

pub use atlas_testkit::fixture_repo;
use atlas_testkit::CliDaemon;
use std::path::Path;

/// A harness around this crate's `atlas`, which starts and stops its own daemon. Derefs
/// to the shared [`CliDaemon`] for `cmd`, `client`, `stop`, `home` and `port`.
pub struct TestDaemon(pub CliDaemon);

impl TestDaemon {
    pub fn new() -> Self { Self(CliDaemon::new(Path::new(env!("CARGO_BIN_EXE_atlas")))) }
}

impl std::ops::Deref for TestDaemon {
    type Target = CliDaemon;
    fn deref(&self) -> &CliDaemon { &self.0 }
}
