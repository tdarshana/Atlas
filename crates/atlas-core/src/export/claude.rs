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
    // Every value goes through the same quoting, not only the description: a `:` in a
    // tool name or a model hint would otherwise turn one frontmatter line into two keys
    // and `import` would read the tail of the value as a field of its own.
    if !a.tools.is_empty() {
        out.push_str(&format!("tools: {}\n", yaml_scalar(&a.tools.join(", "))));
    }
    // Not part of Claude Code's own frontmatter, which ignores keys it does not
    // know. It is here so that `atlas export` followed by `atlas import` returns
    // an agent's tags instead of clearing them.
    if !a.tags.is_empty() {
        out.push_str(&format!("tags: {}\n", yaml_scalar(&a.tags.join(", "))));
    }
    if let Some(hint) = &a.model_hint {
        out.push_str(&format!("model: {}\n", yaml_scalar(hint)));
    }
    out.push_str("---\n\n");
    out.push_str(&a.instructions);
    out.push('\n');
    out
}

/// Quotes a YAML plain scalar unless it is safe unquoted. Unsafe means: empty, a
/// `:` (which would split the line into another key) or a `#` (which would start
/// a comment) anywhere in it, padding a reader would trim, or a first character
/// YAML treats as syntax.
fn yaml_scalar(s: &str) -> String {
    let starts_special = matches!(s.chars().next(), Some('!' | '&' | '*' | '?' | '|' | '>' | '%' | '@' | '`' | '"' | '\'' | '#' | ',' | '[' | ']' | '{' | '}' | '-'));
    let padded = s.starts_with(' ') || s.ends_with(' ');
    if s.is_empty() || s.contains(':') || s.contains('#') || padded || starts_special {
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

    #[test]
    fn comment_marker_and_padding_are_quoted() {
        assert_eq!(yaml_scalar("counts #1"), "\"counts #1\"");
        assert_eq!(yaml_scalar("trailing "), "\"trailing \"");
        assert_eq!(yaml_scalar(" leading"), "\" leading\"");
    }

    /// Tools, tags and the model hint carry the same risk as the description: an
    /// unquoted `:` would make `import` read the tail of the line as another key.
    #[test]
    fn every_frontmatter_value_is_quoted_when_it_has_to_be() {
        let a = Agent {
            id: uuid::Uuid::nil(),
            name: "reviewer".into(),
            description: "Reviews PRs".into(),
            instructions: "Be strict.".into(),
            model_hint: Some("vendor: opus".into()),
            tools: vec!["mcp: read".into()],
            tags: vec!["area: qa".into()],
            version: 1,
            created_at: Default::default(),
            updated_at: Default::default(),
        };
        let md = claude_agent_md(&a);
        assert!(md.contains("tools: \"mcp: read\"\n"), "{md}");
        assert!(md.contains("tags: \"area: qa\"\n"), "{md}");
        assert!(md.contains("model: \"vendor: opus\"\n"), "{md}");
    }
}
