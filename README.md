# Atlas

Shared memory and agent hub for coding agents (Claude Code, Codex, Claude, DeepSeek and OpenRouter-based agents). Rust daemon with DuckDB storage, MCP over stdio and HTTP, a CLI with a TUI, and a Tauri 2 desktop app.

## Install

    cargo install --path crates/atlas-cli --locked
    cargo install --path crates/atlasd --locked

## Connect agents

    claude mcp add --scope user atlas -- atlas mcp

Codex, in `~/.codex/config.toml`:

    [mcp_servers.atlas]
    command = "atlas"
    args = ["mcp"]

Any MCP client that speaks streamable HTTP can use `http://127.0.0.1:7433/mcp` once `atlas daemon start` has run.

## CLI

    atlas remember "we deploy to fly.io" --kind decision --tag infra
    atlas recall "where do we deploy"
    atlas daemon status

Data lives in `~/.atlas/atlas.duckdb`. Set `ATLAS_HOME` to relocate it. See `docs/usage.md` and `docs/superpowers/specs/2026-09-02-atlas-design.md`.
