/// System prompt for the opt-in LLM extraction step: turns a transcript into
/// a JSON array of memory candidates. Kept under 40 lines; edit with care.
pub const EXTRACTION_SYSTEM_PROMPT: &str = r#"You are extracting durable memories from a coding-agent conversation transcript.

Read the transcript and identify durable facts, decisions, preferences, and
insights about the project or about the user's way of working. Skip
transient chatter: greetings, acknowledgements, one-off command output, and
anything that will not still be true or useful in a week.

For each memory found, emit one JSON object with these fields:
- "text": a short, self-contained sentence stating the memory
- "kind": one of "fact", "decision", "preference", "insight", "todo"
- "tags": 1-5 short lowercase words describing the topic
- "confidence": a number from 0 to 1 for how sure you are this is durable

Return ONLY a JSON array of these objects, nothing else: no prose, no
Markdown fences, no explanation. If nothing durable is present, return an
empty array: []

Example:
[
  {"text": "The project uses bun, not npm or pnpm.", "kind": "fact", "tags": ["tooling"], "confidence": 0.9},
  {"text": "User prefers surgical edits over full-file rewrites.", "kind": "preference", "tags": ["workflow"], "confidence": 0.8}
]
"#;

/// Prompt to have the model turn a `ProjectProfile`'s raw text into a short
/// summary suitable for a project's memory context.
pub fn project_summary_prompt(profile_text: &str) -> String {
    format!(
        "Summarize this project profile in 2-3 sentences for a coding agent's memory. \
Focus on what the project is, its stack, and anything a new agent should know \
before working in it. Respond with the summary only, no preamble.\n\n{profile_text}"
    )
}
