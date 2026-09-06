use crate::models::Doc;

pub const START: &str = "<!-- atlas:start -->";
pub const END: &str = "<!-- atlas:end -->";

/// Inputs for the managed instruction block spliced into `AGENTS.md` /
/// `CLAUDE.md`.
pub struct BlockContext {
    pub mcp_command: String,
    pub practices: Vec<Doc>,
    pub project_name: Option<String>,
}

/// Renders the managed block, markers included. Empty `practices` omits its
/// section entirely.
pub fn render_block(ctx: &BlockContext) -> String {
    render_block_with(ctx, "")
}

/// [`render_block`] with `extra` (already rendered Markdown, such as the agent
/// sections from [`super::persona`]) placed after the practices and before the end
/// marker. An empty `extra` renders exactly what `render_block` does.
pub fn render_block_with(ctx: &BlockContext, extra: &str) -> String {
    let mut lines: Vec<String> = vec![START.to_string(), "## Atlas (shared memory and agents)".to_string(), String::new()];
    let command = neutralize(&ctx.mcp_command);
    let sentence = match &ctx.project_name {
        Some(name) => format!("This project is connected to Atlas as `{}`. Atlas is available as the MCP server `atlas` (`{command}`).", neutralize(name)),
        None => format!("Atlas is available as the MCP server `atlas` (`{command}`)."),
    };
    lines.push(sentence);
    lines.push("Call `memory_search` before starting a task and `memory_remember` when you learn a durable fact, make a decision, or notice a preference.".to_string());
    if !ctx.practices.is_empty() {
        lines.push(String::new());
        lines.push("### Practices".to_string());
        for p in &ctx.practices {
            lines.push(format!("- **{}**: {}", neutralize(&p.name), neutralize(&p.body)));
        }
    }
    let extra = extra.trim_end();
    if !extra.is_empty() {
        lines.push(String::new());
        lines.push(extra.to_string());
    }
    lines.push(END.to_string());
    format!("{}\n", lines.join("\n"))
}

/// Defuses a managed-block marker inside an interpolated value by putting a
/// zero-width space after `atlas`. A practice body or agent instruction
/// containing `<!-- atlas:end -->` would otherwise close the block early, and
/// every later sync would splice into a shorter span and leave the tail behind,
/// growing the file a copy at a time.
pub(crate) fn neutralize(s: &str) -> String {
    s.replace("<!-- atlas:", "<!-- atlas\u{200b}:")
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
