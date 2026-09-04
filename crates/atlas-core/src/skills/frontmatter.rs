//! The two frontmatter keys a `SKILL.md` file is required to carry, read without a
//! YAML parser.
//!
//! Only `name` and `description` are read, and only from the block between the first
//! two `---` lines. A quoted value is unquoted; nothing else of YAML is interpreted.
//! Everything the format allows but Atlas does not use (anchors, block scalars, nested
//! maps) is left alone rather than half-understood.

/// How much of a `SKILL.md` file is ever read: enough for any real skill, small enough
/// that a huge or accidental file cannot be pulled into the daemon's memory.
pub const MAX_SKILL_BYTES: usize = 256 * 1024;

/// The longest description built from a body paragraph, when the frontmatter names none.
const DESCRIPTION_FALLBACK_CHARS: usize = 280;

#[derive(Debug, Default, PartialEq)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Reads `name` and `description` out of `text`'s frontmatter. A file that does not
/// open with a `---` line has none, and comes back empty rather than as an error: a
/// `SKILL.md` without frontmatter is still a skill, it just falls back to its folder
/// name and its first paragraph.
pub fn parse(text: &str) -> Frontmatter {
    let mut out = Frontmatter::default();
    let Some(block) = block(text) else { return out };
    let mut lines = block.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((key, rest)) = line.split_once(':') else { continue };
        // Only a top-level key counts: an indented line belongs to a structure this
        // parser deliberately does not understand.
        if key.starts_with(char::is_whitespace) {
            continue;
        }
        let key = key.trim();
        if key != "name" && key != "description" {
            continue;
        }
        let mut value = rest.trim().to_string();
        // A quoted value may run past the end of its line; real skill descriptions do.
        // Keep taking lines until the quote closes, joining them with a space the way a
        // YAML flow scalar folds them.
        if let Some(quote) = opening_quote(&value) {
            while !closes_with(&value, quote) {
                let Some(next) = lines.next() else { break };
                value.push(' ');
                value.push_str(next.trim());
            }
        }
        let value = unquote(&value);
        if value.is_empty() {
            continue;
        }
        match key {
            "name" => out.name = Some(value),
            _ => out.description = Some(value),
        }
    }
    out
}

/// The description to show when the frontmatter carries none: the first non-empty
/// paragraph of the body, its Markdown heading markers dropped, folded onto one line and
/// cut to 280 characters.
pub fn first_paragraph(text: &str) -> String {
    let body = match block_end(text) {
        Some(end) => &text[end..],
        None => text,
    };
    let mut paragraph: Vec<&str> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            if paragraph.is_empty() {
                continue;
            }
            break;
        }
        // A leading `# Title` is the file's heading, not its description.
        if paragraph.is_empty() && line.starts_with('#') {
            continue;
        }
        paragraph.push(line);
    }
    let joined = paragraph.join(" ");
    if joined.chars().count() <= DESCRIPTION_FALLBACK_CHARS {
        return joined;
    }
    joined.chars().take(DESCRIPTION_FALLBACK_CHARS).collect()
}

/// The text between the first two `---` lines, or `None` when the file does not open
/// with one or never closes it.
fn block(text: &str) -> Option<&str> {
    let rest = open(text)?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// The byte offset in `text` just past the closing `---` line, for the body split.
fn block_end(text: &str) -> Option<usize> {
    let rest = open(text)?;
    let offset = text.len() - rest.len();
    let end = rest.find("\n---")?;
    // Past `\n---` and past the rest of that line.
    let after = &rest[end + 4..];
    Some(offset + end + 4 + after.find('\n').map(|i| i + 1).unwrap_or(after.len()))
}

/// Everything after an opening `---` line, or `None` when there is none.
fn open(text: &str) -> Option<&str> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text.strip_prefix("---")?;
    let rest = rest.strip_prefix('\r').unwrap_or(rest);
    rest.strip_prefix('\n')
}

fn opening_quote(value: &str) -> Option<char> {
    match value.chars().next() {
        Some(q @ ('"' | '\'')) => Some(q),
        _ => None,
    }
}

/// Whether `value`, which opens with `quote`, also closes with it.
fn closes_with(value: &str, quote: char) -> bool {
    value.chars().count() > 1 && value.ends_with(quote)
}

/// Strips a matching pair of surrounding quotes, and trims what is left.
fn unquote(value: &str) -> String {
    let value = value.trim();
    if let Some(q) = opening_quote(value) {
        if closes_with(value, q) {
            return value[q.len_utf8()..value.len() - q.len_utf8()].trim().to_string();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_unquoted_keys() {
        let fm = parse("---\nname: review-pr\ndescription: Review a pull request.\n---\n\nBody.\n");
        assert_eq!(fm.name.as_deref(), Some("review-pr"));
        assert_eq!(fm.description.as_deref(), Some("Review a pull request."));
    }

    #[test]
    fn unquotes_quoted_values() {
        let fm = parse("---\nname: \"review-pr\"\ndescription: 'Review a PR: carefully.'\n---\n");
        assert_eq!(fm.name.as_deref(), Some("review-pr"));
        assert_eq!(fm.description.as_deref(), Some("Review a PR: carefully."));
    }

    /// Real skill descriptions are long quoted strings that wrap across lines; the
    /// value keeps going until the quote closes.
    #[test]
    fn joins_a_quoted_value_that_wraps() {
        let fm = parse("---\nname: long\ndescription: \"Use when the user asks\n  about anything at all,\n  including edge cases.\"\n---\n");
        assert_eq!(fm.description.as_deref(), Some("Use when the user asks about anything at all, including edge cases."));
    }

    #[test]
    fn a_missing_key_is_none() {
        let fm = parse("---\nname: only-a-name\n---\n");
        assert_eq!(fm.name.as_deref(), Some("only-a-name"));
        assert_eq!(fm.description, None);
    }

    #[test]
    fn no_frontmatter_reads_nothing() {
        assert_eq!(parse("# Just a heading\n\nSome text.\n"), Frontmatter::default());
        // An unterminated block is not frontmatter either.
        assert_eq!(parse("---\nname: never-closed\n"), Frontmatter::default());
    }

    /// Nothing but `name` and `description` at the top level is read, so a nested key
    /// with either name cannot masquerade as one.
    #[test]
    fn other_and_indented_keys_are_ignored() {
        let fm = parse("---\nname: real\nallowed-tools: Bash\nmeta:\n  description: nested\n---\n");
        assert_eq!(fm.name.as_deref(), Some("real"));
        assert_eq!(fm.description, None);
    }

    #[test]
    fn first_paragraph_skips_the_frontmatter_and_the_heading() {
        let text = "---\nname: x\n---\n\n# Title\n\nThe first real paragraph,\nwrapped over two lines.\n\nA second one.\n";
        assert_eq!(first_paragraph(text), "The first real paragraph, wrapped over two lines.");
    }

    #[test]
    fn first_paragraph_is_capped() {
        let text = format!("---\nname: x\n---\n\n{}\n", "word ".repeat(200));
        assert_eq!(first_paragraph(&text).chars().count(), DESCRIPTION_FALLBACK_CHARS);
    }

    #[test]
    fn first_paragraph_of_a_file_without_frontmatter() {
        assert_eq!(first_paragraph("Plain text only.\n"), "Plain text only.");
        assert_eq!(first_paragraph(""), "");
    }
}
