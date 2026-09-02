pub mod data;
pub mod state;

/// Placeholder for the terminal loop. Task 4 replaces this with the real
/// ratatui event loop; keeping it a no-op lets `atlas tui` exist and compile now.
pub async fn run(_port: u16) -> anyhow::Result<()> {
    Ok(())
}
