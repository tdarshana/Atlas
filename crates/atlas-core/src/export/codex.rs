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
    out.push_str(&format!("developer_instructions = {}\n", multiline(&a.instructions)));
    out
}

/// Renders the instructions as a TOML multi-line string. The literal form
/// (`'''`) is used by default: it has no escapes at all, so a Windows path or a
/// regex backslash survives as written, which the basic form would have turned
/// into an invalid escape sequence. Instructions that cannot go in a literal —
/// they contain `'''`, or a control character a literal may not hold — fall
/// back to the basic form with everything escaped.
fn multiline(s: &str) -> String {
    let needs_escaping = s.contains("'''") || s.chars().any(|c| is_c0(c) && c != '\n' && c != '\t');
    if !needs_escaping {
        return format!("'''\n{s}\n'''");
    }
    let mut body = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '\\' => body.push_str("\\\\"),
            // A lone `"` is legal inside a multi-line basic string but three in a row
            // would close it, so every one is escaped rather than counted.
            '"' => body.push_str("\\\""),
            // Newlines and tabs are what the multi-line form exists to carry; every
            // other control character has to be escaped to keep the document valid.
            '\n' | '\t' => body.push(c),
            c if is_c0(c) => body.push_str(&escape_unicode(c)),
            c => body.push(c),
        }
    }
    format!("\"\"\"\n{body}\n\"\"\"")
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
            // Every other control character is illegal raw in a TOML string, so it
            // goes in as an escape rather than as a byte that fails to parse.
            c if is_c0(c) => out.push_str(&escape_unicode(c)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The characters TOML forbids raw in a string: the C0 controls and DEL.
fn is_c0(c: char) -> bool {
    (c as u32) < 0x20 || c == '\u{7f}'
}

fn escape_unicode(c: char) -> String {
    format!("\\u{:04X}", c as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_with(instructions: &str) -> Agent {
        Agent {
            id: uuid::Uuid::nil(),
            name: "reviewer".into(),
            description: "Reviews pull requests".into(),
            instructions: instructions.into(),
            model_hint: Some("opus".into()),
            tools: vec![],
            tags: vec![],
            version: 1,
            created_at: Default::default(),
            updated_at: Default::default(),
        }
    }

    /// Parses the rendered document and returns the instructions it round-trips to.
    /// The closing delimiter sits on its own line, so the parsed value carries one
    /// trailing newline the stored instructions do not; it is dropped here.
    fn parsed_instructions(instructions: &str) -> String {
        let rendered = codex_agent_toml(&agent_with(instructions));
        let value: toml::Value = toml::from_str(&rendered).unwrap_or_else(|e| panic!("invalid TOML ({e}):\n{rendered}"));
        let text = value["developer_instructions"].as_str().unwrap();
        text.strip_suffix('\n').unwrap_or(text).to_string()
    }

    #[test]
    fn toml_string_escapes_quotes() {
        assert_eq!(toml_string("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn toml_string_escapes_control_characters() {
        assert_eq!(toml_string("a\rb\u{1}"), "\"a\\u000Db\\u0001\"");
    }

    /// Backslashes are what the basic form got wrong: a Windows path or a regex
    /// escape used to render as an invalid escape sequence.
    #[test]
    fn backslashes_and_quotes_survive_as_valid_toml() {
        let instructions = "Look under C:\\path\\to for a \\d+ match.\nQuote it as \"\"\" if you must.";
        assert_eq!(parsed_instructions(instructions), instructions);
    }

    #[test]
    fn literal_delimiter_falls_back_to_the_escaped_form() {
        let instructions = "A literal ''' closes the literal form.\nSo does C:\\path.";
        let rendered = codex_agent_toml(&agent_with(instructions));
        assert!(rendered.contains("developer_instructions = \"\"\"\n"), "expected the basic form: {rendered}");
        assert_eq!(parsed_instructions(instructions), instructions);
    }

    #[test]
    fn control_characters_in_instructions_stay_valid() {
        let instructions = "before\u{1}after";
        assert_eq!(parsed_instructions(instructions), instructions);
    }
}
