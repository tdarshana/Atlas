use crate::models::{Agent, Doc};

pub const START: &str = "<!-- atlas:start -->";
pub const END: &str = "<!-- atlas:end -->";

/// Inputs for the managed instruction block spliced into `AGENTS.md` /
/// `CLAUDE.md`.
pub struct BlockContext {
    pub mcp_command: String,
    pub agents: Vec<Agent>,
    pub practices: Vec<Doc>,
    pub project_name: Option<String>,
}

/// Renders the managed block, markers included. Empty `agents`/`practices`
/// omit their section entirely.
pub fn render_block(ctx: &BlockContext) -> String {
    let mut lines: Vec<String> = vec![START.to_string(), "## Atlas (shared memory and agents)".to_string(), String::new()];
    let sentence = match &ctx.project_name {
        Some(name) => format!("This project is connected to Atlas as `{name}`. Atlas is available as the MCP server `atlas` (`{}`).", ctx.mcp_command),
        None => format!("Atlas is available as the MCP server `atlas` (`{}`).", ctx.mcp_command),
    };
    lines.push(sentence);
    lines.push("Call `recall` before starting a task and `remember` when you learn a durable fact, make a decision, or notice a preference.".to_string());
    if !ctx.agents.is_empty() {
        lines.push(String::new());
        lines.push("### Agents".to_string());
        for a in &ctx.agents {
            lines.push(format!("- `{}`: {}", a.name, a.description));
        }
    }
    if !ctx.practices.is_empty() {
        lines.push(String::new());
        lines.push("### Practices".to_string());
        for p in &ctx.practices {
            lines.push(format!("- **{}**: {}", p.name, p.body));
        }
    }
    lines.push(END.to_string());
    format!("{}\n", lines.join("\n"))
}

/// Replaces the content between `START`/`END` with `block` (which itself
/// carries fresh markers), preserving everything else in `existing`
/// byte-for-byte. When no markers are present — or an `END` is found before
/// any `START` (an orphaned marker, e.g. from a pasted example, rather than a
/// real span) — appends `block` after exactly one blank line (or, for empty
/// `existing`, with no leading blank line).
pub fn splice_block(existing: &str, block: &str) -> String {
    let span = match (existing.find(START), existing.find(END)) {
        (Some(s), Some(e)) if e > s => Some((s, e)),
        _ => None,
    };
    if let Some((s, e)) = span {
        let e_end = e + END.len();
        format!("{}{}{}", &existing[..s], block, &existing[e_end..])
    } else {
        let trimmed = existing.trim_end_matches('\n');
        if trimmed.is_empty() {
            format!("{block}\n")
        } else {
            format!("{trimmed}\n\n{block}\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_before_start_is_left_intact_with_block_appended() {
        let existing = "notes\n<!-- atlas:end -->\nmore notes\n<!-- atlas:start -->\ntail\n";
        let block = "<!-- atlas:start -->\nnew\n<!-- atlas:end -->";
        let out = splice_block(existing, block);
        assert_eq!(out, "notes\n<!-- atlas:end -->\nmore notes\n<!-- atlas:start -->\ntail\n\n<!-- atlas:start -->\nnew\n<!-- atlas:end -->\n");
    }

    #[test]
    fn start_without_end_appends() {
        let existing = "notes\n<!-- atlas:start -->\nunterminated\n";
        let block = "<!-- atlas:start -->\nnew\n<!-- atlas:end -->";
        let out = splice_block(existing, block);
        assert_eq!(out, "notes\n<!-- atlas:start -->\nunterminated\n\n<!-- atlas:start -->\nnew\n<!-- atlas:end -->\n");
    }

    #[test]
    fn two_spans_replaces_only_the_first() {
        let existing = "<!-- atlas:start -->\nold1\n<!-- atlas:end -->\nmiddle\n<!-- atlas:start -->\nold2\n<!-- atlas:end -->\n";
        let block = "<!-- atlas:start -->\nnew\n<!-- atlas:end -->";
        let out = splice_block(existing, block);
        assert_eq!(out, "<!-- atlas:start -->\nnew\n<!-- atlas:end -->\nmiddle\n<!-- atlas:start -->\nold2\n<!-- atlas:end -->\n");
    }
}
