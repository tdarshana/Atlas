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
/// byte-for-byte. When no markers are present, appends `block` after exactly
/// one blank line (or, for empty `existing`, with no leading blank line).
pub fn splice_block(existing: &str, block: &str) -> String {
    if let (Some(s), Some(e)) = (existing.find(START), existing.find(END)) {
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
