use super::GENERATED_HEADER;
use crate::models::Agent;

/// Renders a Claude Code agent file: YAML frontmatter followed by the
/// instructions body. Hand-built rather than via a YAML crate since the
/// frontmatter shape here is fixed and small.
pub fn claude_agent_md(a: &Agent) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("# {GENERATED_HEADER}\n"));
    out.push_str(&format!("name: {}\n", a.name));
    out.push_str(&format!("description: {}\n", yaml_scalar(&a.description)));
    if !a.tools.is_empty() {
        out.push_str(&format!("tools: {}\n", a.tools.join(", ")));
    }
    if let Some(hint) = &a.model_hint {
        out.push_str(&format!("model: {hint}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(&a.instructions);
    out.push('\n');
    out
}

/// Quotes a YAML plain scalar only when it isn't safe unquoted (contains a
/// `:` or starts with a character that YAML would otherwise treat as syntax).
fn yaml_scalar(s: &str) -> String {
    let starts_special = matches!(s.chars().next(), Some('!' | '&' | '*' | '?' | '|' | '>' | '%' | '@' | '`' | '"' | '\'' | '#' | ',' | '[' | ']' | '{' | '}' | '-' | ' '));
    if s.is_empty() || s.contains(':') || starts_special {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_scalar_left_unquoted() {
        assert_eq!(yaml_scalar("Reviews pull requests"), "Reviews pull requests");
    }

    #[test]
    fn scalar_with_colon_is_quoted() {
        assert_eq!(yaml_scalar("note: careful"), "\"note: careful\"");
    }
}
