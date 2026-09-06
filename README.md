<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" height="128" alt="Atlas icon">
</p>

<h1 align="center">Atlas</h1>

<p align="center">Shared memory and agent hub for coding agents.</p>

Atlas gives Claude Code, Codex, Claude, DeepSeek and OpenRouter based agents one local place to remember decisions, share a task board, adopt personas and run workflows. A Rust daemon owns a DuckDB file and serves a JSON API and MCP; a CLI with a TUI and a Tauri 2 desktop app are clients of that daemon. Everything stays on your machine.

## What it does

- **Memory**: `remember`, `recall` and `forget` with hybrid BM25 plus embedding search, kinds, tags, scopes and a review queue. Nothing is hard deleted; every write is audited.
- **Projects**: connect a repository, build its profile, and `atlas sync` what Atlas knows into `CLAUDE.md`, `AGENTS.md` and subagent files, inside managed blocks.
- **Board**: a task board with stages, blockers, subtasks, claims and comments that every agent reads and writes over MCP, next to the CLI, the TUI and the desktop app. Changes stream live to the app.
- **Personas**: a library of roles (skills, workflows, practices, MCP servers, per case models, access rules) a project puts on its roster; agents adopt one per task or per session and the daemon enforces its rules.
- **Skills, practices, agents, MCP servers**: discovered from Claude Code and Codex configs and managed from one place.
- **Workflows**: graphs of a trigger, actions and an output on a canvas, run on a schedule, on a prompt or by hand.
- **Extraction**: optional turning of transcripts into candidate memories through an OpenAI compatible endpoint. Off by default.

## Install

Open the dmg from the releases page, drag Atlas to Applications and launch it. The app starts the daemon and keeps it running in the background after the window closes; the menu bar item shows its state. Then in Settings, Command line, click **Install command line tool** to get the `atlas` command (CLI, TUI and the `atlas mcp` server) on your PATH.

From source instead:

    cargo install --path crates/atlas-cli --locked
    cargo install --path crates/atlasd --locked

## Connect agents

    claude mcp add --scope user atlas -- atlas mcp

Codex, in `~/.codex/config.toml`:

    [mcp_servers.atlas]
    command = "atlas"
    args = ["mcp"]

Any MCP client that speaks streamable HTTP can use `http://127.0.0.1:7433/mcp` once `atlas daemon start` has run. The daemon writes its port and a per run token into `~/.atlas/daemon.json`; every client reads both from there.

## Connect a project

    cd repo
    atlas project connect
    atlas sync

`atlas sync --check` reports what would change without writing. Saved agents and roster personas show up as Claude Code and Codex subagents after a sync.

## CLI and TUI

    atlas remember "we deploy to fly.io" --kind decision --tag infra
    atlas recall "where do we deploy"
    atlas task create "Add board export"
    atlas agent list
    atlas daemon status
    atlas tui

## Desktop app

    bun install
    bun run tauri dev

A Tauri 2 app over the same daemon: Dashboard, Projects, Memories, Agents, Practices, Workflows, Review, Permissions, Skills, Personas, MCP and Settings in a VS Code style shell, with a command palette, live updates, OS notifications, a vault for API keys, an updater and a plugin system.

To package it:

    CI=true bun run tauri build

That builds the `atlasd` and `atlas` sidecars in release mode, stages them, and writes `atlas.app` and a `.dmg` under `target/release/bundle`. `CI=true` skips the disk image's Finder styling step, which otherwise needs an Automation grant for the terminal.

## Layout

| Path | What it is |
|---|---|
| `crates/atlas-core` | Library: DuckDB access, models, repositories, search, the `Backend` traits with `LocalBackend` |
| `crates/atlas-mcp` | The MCP tool router, generic over `Backend` |
| `crates/atlasd` | The daemon. The only process that opens the DuckDB file. Loopback only, default port 7433 |
| `crates/atlas-cli` | The `atlas` binary: CLI, TUI and the `atlas mcp` stdio shim |
| `crates/atlas-client` | Daemon control and the `RemoteBackend` HTTP client shared by the CLI and the app |
| `src`, `src-tauri` | The desktop app: Svelte 5 with bun, Tauri 2 host in Rust |
| `packages/plugin-api` | Types for desktop plugins |

## Development

    cargo build --workspace
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    bun run check
    bun run vitest run
    bun run gen:types    # regenerate src/lib/types.generated.ts from the Rust models
    bun run gen:api      # regenerate src/lib/api.generated.ts from the daemon's route table

`ATLAS_HOME` relocates `~/.atlas`, `ATLAS_PORT` changes the port, `ATLAS_NO_EMBED=1` skips the embedding model download.

## License

Free for personal use only. Not free for commercial use. See `LICENSE`; contact the copyright holder for a commercial licence.
