//! The terminal loop: keys and finished requests in, draws out.
//!
//! Everything that decides anything lives in [`super::state`]; everything that
//! talks to the daemon lives in [`super::data`]. This file only owns the
//! terminal, and it must hand it back however it leaves.

use super::data::perform;
use super::state::{initial_effects, reduce, Action, App, Effect};
use super::ui::draw;
use crate::remote::RemoteBackend;
use anyhow::Context;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{self, UnboundedSender};

/// How often the loop wakes on its own. Nothing but a redraw depends on it.
const TICK: Duration = Duration::from_millis(250);

pub async fn run(port: u16) -> anyhow::Result<()> {
    let backend = Arc::new(RemoteBackend::new(port));
    let cwd = std::env::current_dir()?;

    // The hook goes in before the terminal changes modes: a panic in between
    // would otherwise leave the user's shell in raw mode with no way back.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        previous(info);
    }));

    // `try_init`, not `init`: piping the TUI anywhere should be one clear error
    // line and exit 1, not a panic and a backtrace note. It leaves behind
    // whatever it did manage to change, so undo that before returning.
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(e) => {
            ratatui::restore();
            return Err(e).context("atlas tui needs an interactive terminal");
        }
    };
    // Restore on the error path too, so a failed draw does not cost the shell.
    let result = event_loop(&mut terminal, backend, cwd).await;
    ratatui::restore();
    result
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    backend: Arc<RemoteBackend>,
    cwd: PathBuf,
) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Action>();
    let mut app = App::default();
    for effect in initial_effects() {
        spawn(effect, &tx, &backend, &cwd);
    }

    let mut events = EventStream::new();
    let mut ticks = tokio::time::interval(TICK);
    terminal.draw(|f| draw(f, &app))?;

    loop {
        let action = tokio::select! {
            event = events.next() => match event {
                // Windows reports press and release; only the press is a keystroke.
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press =>
                    Action::Key(key.code, key.modifiers),
                // Resizes and mouse events change nothing but still want a redraw.
                Some(Ok(_)) => Action::Tick,
                Some(Err(e)) => Action::Error(e.to_string()),
                None => break,
            },
            action = rx.recv() => match action {
                Some(action) => action,
                None => break,
            },
            _ = ticks.tick() => Action::Tick,
        };

        for effect in reduce(&mut app, action) {
            app.loading = true;
            spawn(effect, &tx, &backend, &cwd);
        }
        terminal.draw(|f| draw(f, &app))?;
        if app.quit {
            break;
        }
    }
    Ok(())
}

/// Runs one effect off the loop's thread. A failed send means the loop is
/// already gone, which is not an error worth reporting to anyone.
fn spawn(effect: Effect, tx: &UnboundedSender<Action>, backend: &Arc<RemoteBackend>, cwd: &Path) {
    let tx = tx.clone();
    let backend = Arc::clone(backend);
    let cwd = cwd.to_path_buf();
    tokio::spawn(async move {
        let _ = tx.send(perform(effect, backend, cwd).await);
    });
}
