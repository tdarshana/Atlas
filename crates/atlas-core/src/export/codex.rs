use super::GENERATED_HEADER;
use crate::models::Agent;

/// Renders a Codex agent file: a small TOML document. Hand-built rather than
/// via the `toml` crate so the `developer_instructions` multi-line literal
/// stays byte-stable.
pub fn codex_agent_toml(a: &Agent) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {GENERATED_HEADER}\n"));
    out.push_str(&format!("name = {}\n", toml_string(&a.name)));
    out.push_str(&format!("description = {}\n", toml_string(&a.description)));
    if let Some(hint) = &a.model_hint {
        out.push_str(&format!("model = {}\n", toml_string(hint)));
    }
    // A literal `"""` inside the instructions would prematurely close the
    // multi-line string, so escape it before embedding.
    let escaped = a.instructions.replace("\"\"\"", "\"\"\\\"");
    out.push_str(&format!("developer_instructions = \"\"\"\n{escaped}\n\"\"\"\n"));
    out
}

/// Renders a TOML basic string with the standard escapes.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_string_escapes_quotes() {
        assert_eq!(toml_string("say \"hi\""), "\"say \\\"hi\\\"\"");
    }
}
