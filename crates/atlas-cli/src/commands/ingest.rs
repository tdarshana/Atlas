use crate::remote::RemoteBackend;
use crate::daemon_ctl;
use atlas_core::backend::Backend;
use atlas_core::paths::AtlasPaths;
use atlas_core::AtlasError;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How much of a transcript is sent. A long session runs past any model's
/// context, and the end of it is the part that carries what was decided, so the
/// tail is what survives the trim.
const MAX_CHARS: usize = 200_000;

/// How long a hook may take in total, daemon start included. Claude Code kills a
/// hook at 60 s; stopping well short of that turns a wedged daemon into a line on
/// stderr rather than a minute of silence at the end of the turn.
const HOOK_DEADLINE: Duration = Duration::from_secs(20);

#[derive(clap::Args)]
pub struct IngestArgs {
    /// The tool the transcript came from, recorded on every memory extracted from it
    #[arg(long, default_value = "cli")]
    pub tool: String,
    /// Read the transcript from PATH, or `-` for standard input; a `.jsonl` file is read as a Claude Code transcript
    #[arg(long, conflicts_with_all = ["hook_stdin", "hook_arg"])]
    pub file: Option<PathBuf>,
    /// Read Claude Code's Stop hook JSON from standard input and follow its transcript_path
    #[arg(long, conflicts_with = "hook_arg")]
    pub hook_stdin: bool,
    /// Read Codex's agent-turn-complete notification from JSON, as its `notify` command passes it
    #[arg(long, value_name = "JSON")]
    pub hook_arg: Option<String>,
    /// Attribute the transcript to the project at PATH (default: the hook's cwd, else the working directory)
    #[arg(long)]
    pub project: Option<PathBuf>,
}

/// A hook runs inside someone else's turn, so nothing here may fail: Claude Code
/// and Codex both report a non-zero exit as a failed hook, and neither the daemon
/// being down, an old daemon without the route, a rotated transcript nor a payload
/// we cannot parse is the user's problem to see mid-session. Every one of those
/// becomes a single line on stderr and a clean exit. Only a plain invocation - a
/// person at a shell, or a script - gets the failure as an exit code.
pub async fn run(args: IngestArgs, paths: &AtlasPaths, port: u16) -> anyhow::Result<()> {
    if !(args.hook_stdin || args.hook_arg.is_some()) {
        return ingest(args, paths, port).await;
    }
    match tokio::time::timeout(HOOK_DEADLINE, ingest(args, paths, port)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("atlas: {e}"),
        Err(_) => eprintln!("atlas: timed out reaching the daemon"),
    }
    Ok(())
}

async fn ingest(args: IngestArgs, paths: &AtlasPaths, port: u16) -> anyhow::Result<()> {
    let hook = args.hook_stdin || args.hook_arg.is_some();
    let (text, cwd) = read_transcript(&args)?;
    let text = tail(text);
    if text.trim().is_empty() {
        if hook {
            return Ok(());
        }
        anyhow::bail!("nothing to ingest: the transcript was empty");
    }
    // The daemon's working directory is not the caller's, so the root is resolved
    // here; an unknown root is mapped to global memories by the worker.
    let root = super::abs_path(args.project.or(cwd))?;
    let backend = RemoteBackend::new(daemon_ctl::ensure_daemon(paths, port).await?);
    match backend.ingest_transcript(text, args.tool, Some(root)).await {
        Ok(id) => println!("queued job {id}"),
        // "Not configured" is the one failure worth naming plainly rather than as an
        // error, in both modes; only the exit code differs.
        Err(AtlasError::Conflict(msg)) => {
            eprintln!("{msg}");
            if !hook {
                std::process::exit(1);
            }
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// The transcript text and, when a hook supplied one, the directory the turn ran in.
fn read_transcript(args: &IngestArgs) -> anyhow::Result<(String, Option<PathBuf>)> {
    if args.hook_stdin {
        let hook: serde_json::Value = serde_json::from_str(&stdin()?).map_err(|e| anyhow::anyhow!("the Stop hook payload was not JSON: {e}"))?;
        let named = hook["transcript_path"].as_str().ok_or_else(|| anyhow::anyhow!("the Stop hook payload carried no transcript_path"))?;
        let cwd = hook["cwd"].as_str().map(PathBuf::from);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let path = resolve_transcript(named, cwd.as_deref(), home.as_deref());
        let raw = std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("failed to read the transcript at {}: {e}", path.display()))?;
        return Ok((transcript_to_text(&raw), cwd));
    }
    if let Some(json) = &args.hook_arg {
        let note: serde_json::Value = serde_json::from_str(json).map_err(|e| anyhow::anyhow!("the notification argument was not JSON: {e}"))?;
        return Ok((notification_to_text(&note), note["cwd"].as_str().map(PathBuf::from)));
    }
    if let Some(path) = &args.file {
        let raw = super::read_source(path)?;
        let text = if path.extension().is_some_and(|e| e == "jsonl") { transcript_to_text(&raw) } else { raw };
        return Ok((text, None));
    }
    Ok((stdin()?, None))
}

/// Claude Code documents an absolute `transcript_path`, but it costs nothing to
/// take the other two shapes: a leading `~/` against the home directory, and a
/// relative path against the payload's own `cwd` rather than wherever the hook
/// process happened to start. Without this a `~/...` path fails as a missing
/// file, which reads like the wrong thing entirely.
fn resolve_transcript(named: &str, cwd: Option<&Path>, home: Option<&Path>) -> PathBuf {
    if let (Some(rest), Some(home)) = (named.strip_prefix("~/"), home) {
        return home.join(rest);
    }
    let path = Path::new(named);
    match cwd {
        Some(cwd) if path.is_relative() => cwd.join(path),
        _ => path.to_path_buf(),
    }
}

fn stdin() -> std::io::Result<String> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

/// Flattens a Claude Code transcript (one JSON object per line) into the
/// `user:`/`assistant:` lines the extraction prompt expects. Anything else on a
/// line - a summary record, a tool result, a shape from a newer Claude Code -
/// is dropped rather than guessed at, so an unknown record can never derail the
/// whole transcript.
pub fn transcript_to_text(jsonl: &str) -> String {
    let mut out = String::new();
    for line in jsonl.lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line.trim()) else { continue };
        let role = match record["type"].as_str() {
            Some(r @ ("user" | "assistant")) => r,
            _ => continue,
        };
        let text = content_text(&record["message"]["content"]);
        if text.trim().is_empty() {
            continue;
        }
        out.push_str(role);
        out.push_str(": ");
        out.push_str(text.trim());
        out.push('\n');
    }
    out
}

/// `message.content` is either a plain string or a list of blocks. Only `text`
/// blocks carry conversation; `tool_use` and `tool_result` blocks are the agent
/// talking to its own tools and would bury the discussion in file contents.
fn content_text(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    let Some(blocks) = content.as_array() else { return String::new() };
    blocks
        .iter()
        .filter(|b| b["type"].as_str() == Some("text"))
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Codex's `agent-turn-complete` notification carries the turn rather than a
/// transcript file: the prompts that opened it and the reply that closed it.
pub fn notification_to_text(note: &serde_json::Value) -> String {
    let mut out = String::new();
    let mut push = |role: &str, text: &str| {
        if !text.trim().is_empty() {
            out.push_str(role);
            out.push_str(": ");
            out.push_str(text.trim());
            out.push('\n');
        }
    };
    for message in note["input-messages"].as_array().into_iter().flatten() {
        if let Some(s) = message.as_str() {
            push("user", s);
        }
    }
    if let Some(s) = note["last-assistant-message"].as_str() {
        push("assistant", s);
    }
    out
}

/// Keeps the last `MAX_CHARS` characters. Counted in characters, not bytes, so
/// a transcript that is mostly not ASCII is not cut mid-character.
pub fn tail(text: String) -> String {
    let count = text.chars().count();
    if count <= MAX_CHARS {
        return text;
    }
    text.chars().skip(count - MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_reads_string_and_block_content_and_drops_the_rest() {
        let jsonl = concat!(
            r#"{"type":"summary","summary":"a session"}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"we deploy to fly.io"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Noted."},{"type":"tool_use","name":"Read","input":{"file_path":"/x"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"1000 lines of file"}]}}"#,
            "\n",
            "not json at all\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Recorded the decision."}]}}"#,
            "\n",
        );
        assert_eq!(
            transcript_to_text(jsonl),
            "user: we deploy to fly.io\nassistant: Noted.\nassistant: Recorded the decision.\n"
        );
    }

    #[test]
    fn notification_becomes_user_and_assistant_lines() {
        let note = serde_json::json!({
            "type": "agent-turn-complete",
            "thread-id": "t",
            "input-messages": ["first ask", "second ask"],
            "last-assistant-message": "done",
        });
        assert_eq!(notification_to_text(&note), "user: first ask\nuser: second ask\nassistant: done\n");
        // A turn that ended without a reply still carries the asks.
        let note = serde_json::json!({ "input-messages": ["only ask"] });
        assert_eq!(notification_to_text(&note), "user: only ask\n");
    }

    #[test]
    fn transcript_path_expands_home_and_follows_the_payload_cwd() {
        let cwd = Path::new("/repo");
        let home = Path::new("/Users/someone");
        assert_eq!(
            resolve_transcript("~/.claude/projects/a/b.jsonl", Some(cwd), Some(home)),
            Path::new("/Users/someone/.claude/projects/a/b.jsonl"),
        );
        assert_eq!(resolve_transcript(".claude/session.jsonl", Some(cwd), Some(home)), Path::new("/repo/.claude/session.jsonl"));
        // The documented shape, and the two cases with nothing to resolve against.
        assert_eq!(resolve_transcript("/tmp/session.jsonl", Some(cwd), Some(home)), Path::new("/tmp/session.jsonl"));
        assert_eq!(resolve_transcript("session.jsonl", None, None), Path::new("session.jsonl"));
        assert_eq!(resolve_transcript("~/session.jsonl", None, None), Path::new("~/session.jsonl"));
    }

    #[test]
    fn tail_keeps_the_end_of_an_oversized_transcript() {
        let short = "user: hi\n".to_string();
        assert_eq!(tail(short.clone()), short);

        let long = format!("{}TAIL", "x".repeat(MAX_CHARS));
        let kept = tail(long);
        assert_eq!(kept.chars().count(), MAX_CHARS);
        assert!(kept.ends_with("TAIL"), "the end of the transcript is what must survive");
    }
}
