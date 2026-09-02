pub mod data;
pub mod run;
pub mod state;
pub mod ui;

/// Runs the TUI against the daemon on `port`, which the caller has already
/// started. Returns once the user quits or the terminal loop fails.
pub async fn run(port: u16) -> anyhow::Result<()> {
    run::run(port).await
}
