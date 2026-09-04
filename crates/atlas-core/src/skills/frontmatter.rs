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
    // Counted in characters, never in bytes. `trim_start` strips every Unicode
    // whitespace character, and several of those (a non-breaking space, an ideographic
    // space) are more than one byte, so the shared byte offset one line's ASCII indent
    // produces can land inside another line's multi-byte one. That is a panic on a file
    // the daemon does not own, so the whole block is measured and cut by character.
    let indent = collected.iter().filter(|l| !l.trim().is_empty()).map(|l| indent_chars(l)).min().unwrap_or(0);
    let body: Vec<&str> = collected.iter().map(|l| strip_indent(l, indent)).collect();
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

/// How many whitespace characters a line opens with, counted in characters so a
/// multi-byte one counts once rather than two or three times.
fn indent_chars(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// `line` with its first `indent` characters removed. The offset comes from
/// `char_indices`, so it is a character boundary by construction and can never split a
/// multi-byte character; a line holding `indent` characters or fewer has nothing left
/// after the cut and reads as a blank line, which is what a short line inside a block
/// scalar means.
fn strip_indent(line: &str, indent: usize) -> &str {
    match line.char_indices().nth(indent) {
        Some((byte, _)) => &line[byte..],
        None => "",
    }
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

    /// A block whose lines are indented with multi-byte whitespace is measured and cut
    /// by character. Slicing by byte here panics: the ASCII line's indent of two bytes
    /// lands inside the ideographic space that opens the next line.
    #[test]
    fn block_scalar_indent_may_be_multi_byte_whitespace() {
        // The block's common indent is one character, the ideographic space, so the
        // two-space line keeps one of its own. Cutting the same *byte* offset out of the
        // second line instead would land inside `\u{3000}` and panic.
        let fm = parse("---\ndescription: >-\n  Two ASCII spaces.\n\u{3000}One ideographic space.\n---\n");
        assert_eq!(fm.description.as_deref(), Some(" Two ASCII spaces. One ideographic space."));

        // A non-breaking space (two bytes) indenting the whole block: the common indent
        // is one character, and every line loses exactly that.
        let fm = parse("---\ndescription: |-\n\u{a0}First line.\n\u{a0}Second line.\n---\n");
        assert_eq!(fm.description.as_deref(), Some("First line.\nSecond line."));

        // Mixed the other way round: the shortest indent is the multi-byte one.
        let fm = parse("---\ndescription: >-\n\u{a0}One NBSP.\n    Four spaces.\n---\n");
        assert_eq!(fm.description.as_deref(), Some("One NBSP.    Four spaces."));
    }

    /// A line shorter than the block's common indent is a blank line, not a slice out of
    /// bounds.
    #[test]
    fn a_line_shorter_than_the_indent_reads_as_blank() {
        let fm = parse("---\ndescription: |-\n    Indented four.\n  \n    After a short line.\n---\n");
        assert_eq!(fm.description.as_deref(), Some("Indented four.\nAfter a short line."));
    }

    /// `parse` runs on every discovered `SKILL.md`, including third-party plugin files,
    /// so it has to be total: no input may panic it. A few dozen odd shapes, built from
    /// whitespace and delimiters that have tripped hand-written parsers before.
    #[test]
    fn parse_never_panics_on_odd_input() {
        let pieces = [
            "---", "name:", "description:", ">", ">-", "|", "|-", "\u{a0}", "\u{3000}", "\u{2028}", "\u{feff}",
            " ", "\t", "\r", "\n", "\"", "'", ":", "  \u{85}x", "é", "🙂", "\u{200b}", "\u{1e}",
        ];
        let mut cases: Vec<String> = Vec::new();
        for a in &pieces {
            for b in &pieces {
                cases.push(format!("---\ndescription: >\n{a}{b}\n---\n"));
                cases.push(format!("---\nname: |-\n{a}\n{b}\n---\n"));
                cases.push(format!("{a}---\ndescription:{b}\n---\n{a}{b}"));
            }
        }
        // A block whose lines disagree about which whitespace they are indented with is
        // the shape that panicked, so it is built deliberately as well.
        for a in &pieces {
            cases.push(format!("---\ndescription: >-\n  ascii\n{a}other\n\t{a}\n---\n"));
        }
        for case in &cases {
            let fm = parse(case);
            // Whatever came back is still bounded and still valid UTF-8 by construction.
            for value in [fm.name, fm.description].into_iter().flatten() {
                assert!(value.chars().count() <= MAX_VALUE_CHARS, "{case:?} produced {value:?}");
            }
            let _ = first_paragraph(case);
        }
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
