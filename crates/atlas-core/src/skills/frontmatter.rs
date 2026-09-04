//! The two frontmatter keys a `SKILL.md` file is required to carry, read without a
//! YAML parser.
//!
//! Only `name` and `description` are read, and only from the block between the first
//! two `---` lines. A quoted value is unquoted, a value that wraps across lines is
//! folded, and the four block scalar indicators real skills use (`>`, `>-`, `|`, `|-`)
//! are understood. Nothing else of YAML is: anchors, tags, nested maps and the rest are
//! left alone rather than half-understood.

/// How much of a `SKILL.md` file is ever read: enough for any real skill, small enough
/// that a huge or accidental file cannot be pulled into the daemon's memory.
pub const MAX_SKILL_BYTES: usize = 256 * 1024;

/// The longest value either key can carry, whether it came from the frontmatter or from
/// the first paragraph of the body. Without a cap, one unterminated quote would fold the
/// rest of the file into a field that every listing, `project_context` and `skill_list`
/// then carries.
const MAX_VALUE_CHARS: usize = 280;

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
        let rest = rest.trim();
        let value = match block_scalar(rest) {
            // `description: >` and friends: the value is the indented lines below.
            Some(style) => read_block_scalar(&mut lines, style),
            None => {
                let mut value = rest.to_string();
                // A quoted value may run past the end of its line; real skill
                // descriptions do. Keep taking lines until the quote closes, joining
                // them with a space the way a YAML flow scalar folds them, and stop once
                // the value is past the cap so an unterminated quote cannot fold the
                // whole file.
                if let Some(quote) = opening_quote(&value) {
                    while !closes_with(&value, quote) && value.chars().count() <= MAX_VALUE_CHARS {
                        let Some(next) = lines.next() else { break };
                        value.push(' ');
                        value.push_str(next.trim());
                    }
                }
                unquote(&value)
            }
        };
        let value = cap(&value);
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

/// How a block scalar joins its lines and what it does with the trailing newline.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BlockStyle {
    /// `>` folds line breaks into spaces; `|` keeps them.
    folded: bool,
    /// `-` strips the trailing newline; the plain form keeps one.
    strip: bool,
}

/// The block scalar `rest` opens, for the four indicators a real `SKILL.md` uses. A
/// value carrying anything else, including an explicit indentation digit or `+`, is not
/// one: it is read as the literal text it is, which is what this parser did before.
fn block_scalar(rest: &str) -> Option<BlockStyle> {
    match rest {
        ">" => Some(BlockStyle { folded: true, strip: false }),
        ">-" => Some(BlockStyle { folded: true, strip: true }),
        "|" => Some(BlockStyle { folded: false, strip: false }),
        "|-" => Some(BlockStyle { folded: false, strip: true }),
        _ => None,
    }
}

/// The body of a block scalar: every following line indented past the key (which is at
/// column 0, since an indented key is skipped above), with the common indent removed.
/// The first line that is not indented ends the block and is left for the caller's loop
/// to read as the next key; so does the end of the frontmatter.
fn read_block_scalar<'a>(lines: &mut std::iter::Peekable<std::str::Lines<'a>>, style: BlockStyle) -> String {
    let mut collected: Vec<&'a str> = Vec::new();
    while let Some(next) = lines.peek() {
        // A blank line sits inside the block rather than ending it: it is what separates
        // a folded scalar's paragraphs.
        if !next.trim().is_empty() && !next.starts_with(char::is_whitespace) {
            break;
        }
        collected.push(lines.next().unwrap_or_default());
        // The block cannot be longer than the value it will produce, plus room for the
        // indent it is about to lose.
        if collected.len() > MAX_VALUE_CHARS {
            break;
        }
    }
    while collected.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        collected.pop();
    }
    let indent = collected
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    // Indentation is ASCII whitespace, so this byte offset is always a char boundary.
    let body: Vec<&str> = collected.iter().map(|l| if l.len() >= indent { &l[indent..] } else { l.trim_start() }).collect();
    let mut out = String::new();
    for line in &body {
        if line.trim().is_empty() {
            out.push('\n');
        } else if style.folded {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            out.push_str(line);
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    if !style.strip && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Cuts a value to [`MAX_VALUE_CHARS`] characters, counting characters rather than
/// bytes so a multi-byte one is never split.
fn cap(value: &str) -> String {
    if value.chars().count() <= MAX_VALUE_CHARS {
        return value.to_string();
    }
    value.chars().take(MAX_VALUE_CHARS).collect()
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
    cap(&paragraph.join(" "))
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
        assert_eq!(first_paragraph(&text).chars().count(), MAX_VALUE_CHARS);
    }

    /// An unterminated quote must not fold the rest of the file into the description.
    #[test]
    fn a_frontmatter_value_is_capped_too() {
        let text = format!("---\nname: x\ndescription: \"{}\n---\n", "word ".repeat(200));
        let fm = parse(&text);
        assert_eq!(fm.description.unwrap().chars().count(), MAX_VALUE_CHARS);
    }

    /// `>` folds its indented lines onto one, keeping a blank line as a break, and
    /// clips to a single trailing newline.
    #[test]
    fn folded_block_scalar() {
        let fm = parse("---\nname: folded\ndescription: >\n  Use when the user asks\n  about anything.\n\n  A second paragraph.\nother: ignored\n---\n");
        assert_eq!(fm.description.as_deref(), Some("Use when the user asks about anything.\nA second paragraph.\n"));
        assert_eq!(fm.name.as_deref(), Some("folded"));
    }

    /// `>-` folds the same way and drops the trailing newline.
    #[test]
    fn folded_stripped_block_scalar() {
        let fm = parse("---\ndescription: >-\n  Use when the user asks\n  about anything.\n---\n");
        assert_eq!(fm.description.as_deref(), Some("Use when the user asks about anything."));
    }

    /// `|` keeps its line breaks and clips to a single trailing newline.
    #[test]
    fn literal_block_scalar() {
        let fm = parse("---\ndescription: |\n  First line.\n  Second line.\n---\n");
        assert_eq!(fm.description.as_deref(), Some("First line.\nSecond line.\n"));
    }

    /// `|-` keeps its line breaks and drops the trailing newline.
    #[test]
    fn literal_stripped_block_scalar() {
        let fm = parse("---\nname: |-\n  literal-name\ndescription: |-\n  First line.\n  Second line.\n---\n");
        assert_eq!(fm.name.as_deref(), Some("literal-name"));
        assert_eq!(fm.description.as_deref(), Some("First line.\nSecond line."));
    }

    /// A block scalar that runs to the end of the frontmatter closes there rather than
    /// swallowing the body, and the following key after one is still read.
    #[test]
    fn a_block_scalar_at_the_end_of_the_frontmatter() {
        let fm = parse("---\nname: last\ndescription: >\n  The final key.\n---\n\n# Body\n\nNot part of the description.\n");
        assert_eq!(fm.name.as_deref(), Some("last"));
        assert_eq!(fm.description.as_deref(), Some("The final key.\n"));
    }

    /// An indicator this parser does not know is read as the literal text it is, which
    /// is what it did before block scalars were understood at all.
    #[test]
    fn an_unknown_indicator_stays_literal() {
        let fm = parse("---\ndescription: >2\n  indented\n---\n");
        assert_eq!(fm.description.as_deref(), Some(">2"));
    }

    #[test]
    fn first_paragraph_of_a_file_without_frontmatter() {
        assert_eq!(first_paragraph("Plain text only.\n"), "Plain text only.");
        assert_eq!(first_paragraph(""), "");
    }
}
