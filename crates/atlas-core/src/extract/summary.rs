//! Condenses a `ProjectProfile` into a short summary the model writes, stored
//! into `profile.summary` so a project's context carries a sentence or two of
//! prose alongside its raw fields.

use super::prompt::project_summary_prompt;
use crate::llm::LlmClient;
use crate::models::ProjectProfile;
use crate::Result;

/// The system prompt for the summarization call. Kept separate from
/// `EXTRACTION_SYSTEM_PROMPT`: this call condenses a profile, it does not read a
/// transcript for memories.
const SUMMARY_SYSTEM_PROMPT: &str = "You summarize a software project's profile into a short paragraph for a coding agent's memory.";

/// Characters of rendered profile text sent to the model. A profile's tree can run
/// to 200 entries; capped so one oversized project cannot blow past the model's
/// context window.
const MAX_PROFILE_CHARS: usize = 20_000;

/// Characters kept from the model's reply. The prompt asks for 2-3 sentences, but
/// nothing stops a model from ignoring that, so the stored summary is capped too.
const MAX_SUMMARY_CHARS: usize = 2_000;

/// Renders the parts of a profile worth summarizing as plain text. `name` and
/// `root` are not included: they identify the project, they are not something to
/// summarize.
fn render(profile: &ProjectProfile) -> String {
    let mut out = String::new();
    if !profile.languages.is_empty() {
        out.push_str("Languages: ");
        out.push_str(&profile.languages.join(", "));
        out.push('\n');
    }
    if !profile.frameworks.is_empty() {
        out.push_str("Frameworks: ");
        out.push_str(&profile.frameworks.join(", "));
        out.push('\n');
    }
    if !profile.tree.is_empty() {
        out.push_str("Tree:\n");
        for entry in &profile.tree {
            out.push_str("- ");
            out.push_str(entry);
            out.push('\n');
        }
    }
    if !profile.readme_head.is_empty() {
        out.push_str("README:\n");
        out.push_str(&profile.readme_head);
        out.push('\n');
    }
    if !profile.recent_commits.is_empty() {
        out.push_str("Recent commits:\n");
        for commit in &profile.recent_commits {
            out.push_str("- ");
            out.push_str(commit);
            out.push('\n');
        }
    }
    out
}

fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Asks the model to summarize `profile` in a couple of sentences. The prompt
/// input is capped at `MAX_PROFILE_CHARS` and the reply is trimmed and capped at
/// `MAX_SUMMARY_CHARS` before it is returned.
pub async fn summarize_project(profile: &ProjectProfile, llm: &LlmClient) -> Result<String> {
    let text = take_chars(&render(profile), MAX_PROFILE_CHARS);
    let reply = llm.chat(SUMMARY_SYSTEM_PROMPT, &project_summary_prompt(&text)).await?;
    Ok(take_chars(reply.trim(), MAX_SUMMARY_CHARS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> ProjectProfile {
        ProjectProfile {
            name: "fixture".into(),
            languages: vec!["rust".into()],
            frameworks: vec!["axum".into()],
            tree: vec!["src/main.rs".into()],
            readme_head: "# fixture\n\nA test fixture.".into(),
            recent_commits: vec!["initial commit".into()],
            summary: None,
            built_at: chrono::Utc::now(),
            planning_frameworks: vec![],
        }
    }

    #[test]
    fn render_includes_the_expected_sections_but_not_name() {
        let text = render(&profile());
        assert!(text.contains("Languages: rust"), "{text}");
        assert!(text.contains("Frameworks: axum"), "{text}");
        assert!(text.contains("- src/main.rs"), "{text}");
        assert!(text.contains("README:"), "{text}");
        assert!(text.contains("- initial commit"), "{text}");
        assert!(!text.contains("Name:"), "the profile name is not part of the rendered text: {text}");
    }

    #[test]
    fn take_chars_cuts_on_char_boundaries_not_bytes() {
        let s = "a".repeat(5) + "é";
        assert_eq!(take_chars(&s, 5), "aaaaa");
    }
}
