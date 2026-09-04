//! Small markdown helpers shared by the framework adapters. Title extraction goes
//! through `pulldown-cmark`'s event stream so bold, code and escaped text inside a
//! heading come out as plain text; checkbox and section scanning stay line-based
//! since the conventions they read (`- [ ] `, `## Decisions`, `Ruling:`) are
//! themselves literal lines, not markup pulldown-cmark would otherwise unwrap.

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

/// The text of the document's first level-1 heading, or `None` if it has none.
pub(crate) fn first_heading(text: &str) -> Option<String> {
    let mut in_h1 = false;
    let mut buf = String::new();
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }) => {
                in_h1 = true;
                buf.clear();
            }
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => {
                let title = buf.trim().to_string();
                if !title.is_empty() {
                    return Some(title);
                }
                in_h1 = false;
            }
            Event::Text(t) | Event::Code(t) if in_h1 => buf.push_str(&t),
            _ => {}
        }
    }
    None
}

/// One `- [ ]`/`- [x]` line: whether it is checked, its text, and the nearest
/// preceding heading line (any level, `#`s stripped), if any.
pub(crate) struct Checkbox {
    pub checked: bool,
    pub text: String,
    pub heading: Option<String>,
    /// 1-based source line, used as a fallback anchor when there is no heading.
    pub line: usize,
}

/// Scans `text` line by line for GFM checkbox items (`- [ ] foo` / `- [x] foo`),
/// tagging each with the most recent heading line above it.
pub(crate) fn checkboxes(text: &str) -> Vec<Checkbox> {
    let mut heading: Option<String> = None;
    let mut out = vec![];
    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        if let Some(level) = heading_level(trimmed) {
            let title = trimmed[level..].trim();
            if !title.is_empty() {
                heading = Some(title.to_string());
            }
            continue;
        }
        let (checked, rest) = if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            (false, rest)
        } else if let Some(rest) = trimmed.strip_prefix("- [x] ").or_else(|| trimmed.strip_prefix("- [X] ")) {
            (true, rest)
        } else {
            continue;
        };
        out.push(Checkbox { checked, text: rest.trim().to_string(), heading: heading.clone(), line: i + 1 });
    }
    out
}

/// The bullet lines (`- ` or `* `) under a heading whose text matches `name`
/// (case-insensitive), stopping at the next heading of any level. Matches
/// `## Decisions`, `# Decisions`, and so on regardless of level.
pub(crate) fn section_bullets(text: &str, name: &str) -> Vec<(String, usize)> {
    let mut out = vec![];
    let mut in_section = false;
    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        if let Some(level) = heading_level(trimmed) {
            let title = trimmed[level..].trim();
            in_section = title.eq_ignore_ascii_case(name);
            continue;
        }
        if in_section {
            if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
                out.push((rest.trim().to_string(), i + 1));
            }
        }
    }
    out
}

/// Lines starting with `Ruling:` (leading whitespace ignored), text after the
/// prefix trimmed. Used by the Superpowers adapter to read ledger rulings.
pub(crate) fn ruling_lines(text: &str) -> Vec<(String, usize)> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| l.trim_start().strip_prefix("Ruling:").map(|rest| (rest.trim().to_string(), i + 1)))
        .collect()
}

/// The number of leading `#` characters if `s` is an ATX heading line (1-6 of
/// them followed by a space, or nothing but hashes), else `None`.
fn heading_level(s: &str) -> Option<usize> {
    let n = s.chars().take_while(|&c| c == '#').count();
    if n == 0 || n > 6 {
        return None;
    }
    match s.as_bytes().get(n) {
        None => Some(n),
        Some(b' ') => Some(n),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_from_first_h1() {
        assert_eq!(first_heading("# Hello\n\nbody"), Some("Hello".to_string()));
        assert_eq!(first_heading("no heading here"), None);
        assert_eq!(first_heading("## Not H1\n# Real Title\n"), Some("Real Title".to_string()));
    }

    #[test]
    fn checkbox_lines_carry_the_nearest_heading() {
        let text = "### Task 1: Adapters\n\n- [ ] write the trait\n- [x] write tests\n\n### Task 2: Import\n- [ ] wire routes\n";
        let items = checkboxes(text);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].heading.as_deref(), Some("Task 1: Adapters"));
        assert!(!items[0].checked);
        assert!(items[1].checked);
        assert_eq!(items[2].heading.as_deref(), Some("Task 2: Import"));
    }

    #[test]
    fn decisions_section_stops_at_next_heading() {
        let text = "## Decisions\n\n- keep it simple\n- ship it\n\n## Notes\n- not a decision\n";
        let bullets = section_bullets(text, "Decisions");
        assert_eq!(bullets.len(), 2);
        assert_eq!(bullets[0].0, "keep it simple");
    }

    #[test]
    fn ruling_lines_read_the_prefix() {
        let text = "Spec: something.\nRuling: field is named X.\nRuling: also Y.\n";
        let rulings = ruling_lines(text);
        assert_eq!(rulings.len(), 2);
        assert_eq!(rulings[0].0, "field is named X.");
    }
}
