pub mod data;
pub mod run;
pub mod state;
pub mod ui;

/// Runs the TUI against the daemon on `port`, which the caller has already
/// started under `paths` (where its token is read from). Returns once the user quits
/// or the terminal loop fails.
pub async fn run(paths: &atlas_core::paths::AtlasPaths, port: u16) -> anyhow::Result<()> {
    run::run(paths, port).await
}
