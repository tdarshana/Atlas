use crate::remote::RemoteBackend;
use crate::daemon_ctl;
use atlas_core::backend::JobBackend;
use atlas_core::paths::AtlasPaths;
use atlas_core::AtlasError;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
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
    let backend = RemoteBackend::new(paths, daemon_ctl::ensure_daemon(paths, port).await?);
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
        let text = stream_transcript(&path).map_err(|e| anyhow::anyhow!("failed to read the transcript at {}: {e}", path.display()))?;
        return Ok((text, cwd));
    }
    if let Some(json) = &args.hook_arg {
        let note: serde_json::Value = serde_json::from_str(json).map_err(|e| anyhow::anyhow!("the notification argument was not JSON: {e}"))?;
        return Ok((notification_to_text(&note), note["cwd"].as_str().map(PathBuf::from)));
    }
    if let Some(path) = &args.file {
        if path.extension().is_some_and(|e| e == "jsonl") {
            let text = stream_transcript(path).map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
            return Ok((text, None));
        }
        return Ok((super::read_source(path)?, None));
    }
    let text = stream_plain(std::io::stdin().lock()).map_err(|e| anyhow::anyhow!("failed to read standard input: {e}"))?;
    Ok((text, None))
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
        if let Some(entry) = transcript_line(line) {
            out.push_str(&entry);
        }
    }
    out
}

/// Converts one line of a Claude Code transcript into its `role: text\n`
/// form, or `None` when the line is not a `user`/`assistant` record worth
/// keeping. Shared by `transcript_to_text` and `stream_transcript` so both
/// read the same record the same way.
fn transcript_line(line: &str) -> Option<String> {
    let record: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let role = match record["type"].as_str() {
        Some(r @ ("user" | "assistant")) => r,
        _ => return None,
    };
    let text = content_text(&record["message"]["content"]);
    if text.trim().is_empty() {
        return None;
    }
    Some(format!("{role}: {}\n", text.trim()))
}

/// Reads a Claude Code transcript at `path` one line at a time and converts
/// it exactly as `transcript_to_text` would, but never holds more than the
/// last `MAX_CHARS` characters of converted text: a transcript can reach
/// hundreds of megabytes, and loading the whole file first would hold all of
/// it in memory only to throw most of it away in `tail`.
fn stream_transcript(path: &Path) -> std::io::Result<String> {
    let reader = BufReader::new(std::fs::File::open(path)?);
    let mut buffer: VecDeque<String> = VecDeque::new();
    let mut total = 0usize;
    for line in reader.lines() {
        // A line that is not valid UTF-8 is as unusable as one that is not
        // valid JSON, so it is skipped the same way rather than failing the
        // whole transcript.
        let Ok(line) = line else { continue };
        if let Some(entry) = transcript_line(&line) {
            push_bounded(&mut buffer, &mut total, entry);
        }
    }
    Ok(buffer.into_iter().collect())
}

/// Reads a plain (non-JSONL) transcript one line at a time, bounded by the same
/// ring buffer [`stream_transcript`] uses for a Claude Code file: piped in over
/// standard input, a transcript can be arbitrarily large, and reading it whole
/// before trimming in [`tail`] would hold all of it in memory only to throw most
/// of it away.
fn stream_plain(r: impl BufRead) -> std::io::Result<String> {
    let mut buffer: VecDeque<String> = VecDeque::new();
    let mut total = 0usize;
    for line in r.lines() {
        // A line that is not valid UTF-8 is skipped, the same way stream_transcript
        // skips one, rather than failing the whole transcript.
        let Ok(line) = line else { continue };
        let mut entry = line;
        entry.push('\n');
        push_bounded(&mut buffer, &mut total, entry);
    }
    Ok(buffer.into_iter().collect())
}

/// Appends `entry` to the ring buffer, then trims from the front until at
/// most `MAX_CHARS` characters remain. Applying this after every line keeps
/// exactly the same characters as building the whole text and calling `tail`
/// once at the end, since "keep only the last N characters" gives the same
/// result whether it runs once or after every append.
fn push_bounded(buffer: &mut VecDeque<String>, total: &mut usize, entry: String) {
    *total += entry.chars().count();
    buffer.push_back(entry);
    while *total > MAX_CHARS {
        let excess = *total - MAX_CHARS;
        let front_len = buffer.front().expect("total > 0 implies a front entry").chars().count();
        if front_len <= excess {
            buffer.pop_front();
            *total -= front_len;
        } else {
            let front = buffer.front_mut().expect("checked above");
            *front = front.chars().skip(excess).collect();
            *total -= excess;
        }
    }
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

    #[test]
    fn stream_transcript_matches_the_full_read_under_the_limit() {
        let jsonl = concat!(
            r#"{"type":"summary","summary":"a session"}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"we deploy to fly.io"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Noted."},{"type":"tool_use","name":"Read","input":{"file_path":"/x"}}]}}"#,
            "\n",
            "not json at all\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Recorded the decision."}]}}"#,
            "\n",
        );
        let dir = std::env::temp_dir().join(format!("atlas-ingest-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.jsonl");
        std::fs::write(&path, jsonl).unwrap();

        let streamed = stream_transcript(&path).unwrap();
        assert_eq!(streamed, transcript_to_text(jsonl));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn stream_plain_matches_the_full_read_under_the_limit() {
        let text = "line one\nline two\nline three\n";
        let streamed = stream_plain(text.as_bytes()).unwrap();
        assert_eq!(streamed, text);
    }

    /// Plain stdin ingest (no `--file`, `--hook-stdin` or `--hook-arg`) has no JSON to
    /// parse, but it must still bound memory the same way `stream_transcript` does for
    /// a file: proof that only the tail survives, without ever building the whole
    /// converted string in memory.
    #[test]
    fn stream_plain_bounds_a_large_input_to_the_tail() {
        let mut text = String::new();
        let mut i = 0usize;
        while text.len() < MAX_CHARS + 1_000_000 {
            text.push_str(&format!("line {i} {}\n", "x".repeat(400)));
            i += 1;
        }
        let last_line = i - 1;

        let streamed = stream_plain(text.as_bytes()).unwrap();
        let expected = tail(text);

        assert_eq!(streamed, expected);
        assert!(streamed.chars().count() <= MAX_CHARS);
        assert!(
            streamed.ends_with(&format!("line {last_line} {}\n", "x".repeat(400))),
            "the streamed text must end with the input's last line"
        );
    }

    /// Generates a transcript well past `MAX_CHARS` of converted text and
    /// streams it, so the test both proves the tail matches a full read and,
    /// by never building the whole converted string, that memory use is
    /// bounded by the ring buffer rather than the file's size.
    #[test]
    fn stream_transcript_bounds_a_large_transcript_to_the_tail() {
        let dir = std::env::temp_dir().join("atlas-ingest-large-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("large-{}.jsonl", std::process::id()));

        let last_line;
        {
            use std::io::Write as _;
            let mut writer = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
            let mut written = 0usize;
            let mut i = 0usize;
            // 50 MB of JSONL, far more than the 200,000 converted characters
            // that can survive, so the ring buffer is forced to drop the start.
            while written < 50 * 1024 * 1024 {
                let role = if i.is_multiple_of(2) { "user" } else { "assistant" };
                let text = format!("line {i} {}", "x".repeat(400));
                let record = serde_json::json!({"type": role, "message": {"content": text}}).to_string();
                writeln!(writer, "{record}").unwrap();
                written += record.len() + 1;
                i += 1;
            }
            writer.flush().unwrap();
            last_line = i - 1;
        }

        let start = std::time::Instant::now();
        let text = stream_transcript(&path).unwrap();
        eprintln!("stream_transcript on a 50 MB transcript took {:?}", start.elapsed());

        assert!(text.chars().count() <= MAX_CHARS);
        assert!(
            text.ends_with(&format!("line {last_line} {}\n", "x".repeat(400))),
            "the converted text must end with the transcript's last line"
        );

        std::fs::remove_file(&path).unwrap();
    }
}
