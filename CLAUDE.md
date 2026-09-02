# CLAUDE.md

Guidance for Claude Code when working in this repository.

## What Atlas is

Atlas is a local, Rust-based shared memory and agent hub for coding agents (Claude Code, Codex, Claude, DeepSeek and OpenRouter-based agents). One daemon owns a DuckDB file and serves a JSON API plus MCP; a CLI with a TUI and a Tauri 2 desktop app are clients of that daemon. The design spec is `docs/superpowers/specs/2026-09-02-atlas-design.md` and is the authority when code and docs disagree. Phase plans live in `docs/superpowers/plans/`.

## Layout

- `crates/atlas-core`: library. DuckDB access (`db.rs`), memory models and repository, hybrid BM25 plus embedding search (`search/`, `service.rs`), the `Backend` trait with `LocalBackend`.
- `crates/atlas-mcp`: the rmcp tool router, generic over `Backend`, served from both the daemon and the stdio shim.
- `crates/atlasd`: the daemon binary. The only process that opens the DuckDB file. Loopback only, default port 7433, writes `~/.atlas/daemon.json`.
- `crates/atlas-cli`: the `atlas` binary. `atlas mcp` is the stdio MCP server Claude Code and Codex launch; it talks to the daemon over HTTP and starts it if needed.
- `src-tauri` and `src/`: the desktop app (package `atlas-desktop`, Svelte 5, bun). Not yet wired to the daemon.

## Commands

```bash
cargo build --workspace
cargo test --workspace                       # boots real daemons in integration tests
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p atlasd -- --no-embed            # daemon without the embedding model
cargo run -p atlas-cli -- recall "query"     # CLI
```

`ATLAS_HOME` relocates `~/.atlas`; `ATLAS_PORT` changes the port; `ATLAS_NO_EMBED=1` skips the model download. Installed binaries come from `cargo install --path crates/atlas-cli --locked` and the same for `crates/atlasd`.

## Rules that are easy to break

- Never open the DuckDB file from anything but `atlasd`. Clients use the HTTP API.
- Nothing is hard-deleted from `memories`; `forget` supersedes. Every write appends an `audit` row.
- `remember`, `forget`, `reload` and `set_embedder` take the `write_gate` mutex first. Do not take `index`, `vectors` or the Db mutex before it.
- User text never reaches SQL unescaped. Tag literals double single quotes; everything else is bound.
- The `audit` timestamp column is `"at"`, a reserved word; quote it.
- Check the installed crate source under `~/.cargo/registry` for rmcp and duckdb API names before writing against them; both moved recently.
- stdout of `atlas mcp` is the MCP channel. Log to stderr only.

## Quality gates

Before saying a change is done: `cargo test --workspace` green and warning-free, `cargo clippy --workspace --all-targets -- -D warnings` clean, and for daemon changes a live probe of `/api/v1/status` on a temp `ATLAS_HOME`. Report the actual numbers.

## Working in this repo

Say in a line what you are about to do; close with a recap that stands alone. Batch independent tool calls. The request sets the scope; do not widen it, and report pre-existing problems as follow-ups instead of fixing them uninvited. Edit surgically. Write plainly: short sentences, no em dashes, lists only for parallel items.

# context-mode — MANDATORY routing rules

You have context-mode MCP tools available. These rules are NOT optional — they protect your context window from flooding. A single unrouted command can dump 56 KB into context and waste the entire session.

## BLOCKED commands — do NOT attempt these

### curl / wget — BLOCKED
Any Bash command containing `curl` or `wget` is intercepted and replaced with an error message. Do NOT retry.
Instead use:
- `ctx_fetch_and_index(url, source)` to fetch and index web pages
- `ctx_execute(language: "javascript", code: "const r = await fetch(...)")` to run HTTP calls in sandbox

### Inline HTTP — BLOCKED
Any Bash command containing `fetch('http`, `requests.get(`, `requests.post(`, `http.get(`, or `http.request(` is intercepted and replaced with an error message. Do NOT retry with Bash.
Instead use:
- `ctx_execute(language, code)` to run HTTP calls in sandbox — only stdout enters context

### WebFetch — BLOCKED
WebFetch calls are denied entirely. The URL is extracted and you are told to use `ctx_fetch_and_index` instead.
Instead use:
- `ctx_fetch_and_index(url, source)` then `ctx_search(queries)` to query the indexed content

## REDIRECTED tools — use sandbox equivalents

### Bash (>20 lines output)
Bash is ONLY for: `git`, `mkdir`, `rm`, `mv`, `cd`, `ls`, `npm install`, `pip install`, and other short-output commands.
For everything else, use:
- `ctx_batch_execute(commands, queries)` — run multiple commands + search in ONE call
- `ctx_execute(language: "shell", code: "...")` — run in sandbox, only stdout enters context

### Read (for analysis)
If you are reading a file to **Edit** it → Read is correct (Edit needs content in context).
If you are reading to **analyze, explore, or summarize** → use `ctx_execute_file(path, language, code)` instead. Only your printed summary enters context. The raw file content stays in the sandbox.

### Grep (large results)
Grep results can flood context. Use `ctx_execute(language: "shell", code: "grep ...")` to run searches in sandbox. Only your printed summary enters context.

## Tool selection hierarchy

1. **GATHER**: `ctx_batch_execute(commands, queries)` — Primary tool. Runs all commands, auto-indexes output, returns search results. ONE call replaces 30+ individual calls.
2. **FOLLOW-UP**: `ctx_search(queries: ["q1", "q2", ...])` — Query indexed content. Pass ALL questions as array in ONE call.
3. **PROCESSING**: `ctx_execute(language, code)` | `ctx_execute_file(path, language, code)` — Sandbox execution. Only stdout enters context.
4. **WEB**: `ctx_fetch_and_index(url, source)` then `ctx_search(queries)` — Fetch, chunk, index, query. Raw HTML never enters context.
5. **INDEX**: `ctx_index(content, source)` — Store content in FTS5 knowledge base for later search.

## Subagent routing

When spawning subagents (Agent/Task tool), the routing block is automatically injected into their prompt. Bash-type subagents are upgraded to general-purpose so they have access to MCP tools. You do NOT need to manually instruct subagents about context-mode.

## Output constraints

- Keep responses under 500 words.
- Write artifacts (code, configs, PRDs) to FILES — never return them as inline text. Return only: file path + 1-line description.
- When indexing content, use descriptive source labels so others can `ctx_search(source: "label")` later.

## ctx commands

| Command | Action |
|---------|--------|
| `ctx stats` | Call the `ctx_stats` MCP tool and display the full output verbatim |
| `ctx doctor` | Call the `ctx_doctor` MCP tool, run the returned shell command, display as checklist |
| `ctx upgrade` | Call the `ctx_upgrade` MCP tool, run the returned shell command, display as checklist |
