//! Small markdown helpers shared by the framework adapters. Title extraction goes
//! through `pulldown-cmark`'s event stream so bold, code and escaped text inside a
//! heading come out as plain text; checkbox, section and ruling scanning stay
//! line-based since the conventions they read (`- [ ] `, `## Decisions`, `Ruling:`)
//! are themselves literal lines, not markup pulldown-cmark would otherwise unwrap.
//! All three line-based scanners track fenced code blocks (``` and ~~~) and skip
//! matches inside one, so an example checkbox or heading shown in a spec's own
//! code sample is never read as real content.

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

/// The text of the document's first level-1 heading, or `None` if it has none.
/// Goes through the real parser (rather than a line scan), so this one is
/// fenced-code-block-safe for free: a `# heading`-looking line inside a fenced
/// block never becomes a `Heading` event.
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

/// One `- [ ]`/`- [x]` line: whether it is checked, its text, the nearest
/// preceding heading line (any level, `#`s stripped), if any, and this item's
/// 1-based position among the checkboxes sharing that heading (or, when there is
/// no heading, among those with none). Adapters use `heading`+`ordinal` to build
/// a `SourceRef.anchor` that is unique within the document: two checkboxes under
/// the same heading get `"<heading>#1"`, `"<heading>#2"`, not the same anchor.
/// The ordinal is stable against edits elsewhere in the document (a change above
/// the heading, or under a different heading, never renumbers it) but shifts if
/// an item is added or removed earlier under the *same* heading — a line number
/// would avoid even that, at the cost of changing on any edit above it in the
/// whole file, so the ordinal is the more stable choice of the two the brief
/// allows.
pub(crate) struct Checkbox {
    pub checked: bool,
    pub text: String,
    pub heading: Option<String>,
    /// 1-based source line, used as a fallback anchor when there is no heading.
    pub line: usize,
    pub ordinal: usize,
}

/// Scans `text` line by line for GFM checkbox items (`- [ ] foo` / `- [x] foo`),
/// tagging each with the most recent heading line above it and its ordinal under
/// that heading. Lines inside a fenced code block are never matched.
pub(crate) fn checkboxes(text: &str) -> Vec<Checkbox> {
    let mut heading: Option<String> = None;
    let mut ordinal_in_heading: usize = 0;
    let mut out = vec![];
    let mut fence = Fence::default();
    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        if fence.feed(trimmed) || fence.inside() {
            continue;
        }
        if let Some(level) = heading_level(trimmed) {
            let title = trimmed[level..].trim();
            if !title.is_empty() {
                heading = Some(title.to_string());
                ordinal_in_heading = 0;
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
        ordinal_in_heading += 1;
        out.push(Checkbox { checked, text: rest.trim().to_string(), heading: heading.clone(), line: i + 1, ordinal: ordinal_in_heading });
    }
    out
}

/// The bullet lines (`- ` or `* `) under a heading whose text matches `name`
/// (case-insensitive), stopping at the next heading of any level. Matches
/// `## Decisions`, `# Decisions`, and so on regardless of level. Each result is
/// `(text, line, ordinal)`, `ordinal` being the bullet's 1-based position within
/// the section (for a unique `"<name>#<ordinal>"` anchor, the same scheme
/// `checkboxes` uses). Lines inside a fenced code block are never matched, so a
/// `## Decisions` shown as an example inside one doesn't open a real section.
pub(crate) fn section_bullets(text: &str, name: &str) -> Vec<(String, usize, usize)> {
    let mut out = vec![];
    let mut in_section = false;
    let mut ordinal = 0usize;
    let mut fence = Fence::default();
    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        if fence.feed(trimmed) || fence.inside() {
            continue;
        }
        if let Some(level) = heading_level(trimmed) {
            let title = trimmed[level..].trim();
            in_section = title.eq_ignore_ascii_case(name);
            ordinal = 0;
            continue;
        }
        if in_section {
            if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
                ordinal += 1;
                out.push((rest.trim().to_string(), i + 1, ordinal));
            }
        }
    }
    out
}

/// Lines starting with `Ruling:` (leading whitespace ignored), text after the
/// prefix trimmed, paired with the ruling's 1-based position in the file (for a
/// `"Ruling#<ordinal>"` anchor). Used by the Superpowers adapter to read ledger
/// rulings. Lines inside a fenced code block are never matched.
pub(crate) fn ruling_lines(text: &str) -> Vec<(String, usize, usize)> {
    let mut out = vec![];
    let mut ordinal = 0usize;
    let mut fence = Fence::default();
    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        if fence.feed(trimmed) || fence.inside() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Ruling:") {
            ordinal += 1;
            out.push((rest.trim().to_string(), i + 1, ordinal));
        }
    }
    out
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

/// Tracks whether a line-based scan is currently inside a fenced code block
/// (a run of 3+ `` ` `` or `~` opens one, a matching-or-longer run of the same
/// character closes it). Not a full CommonMark implementation (it doesn't check
/// that a closing fence carries no trailing info string, and a fence opened with
/// one character only closes on that same character), but enough to keep the
/// line scanners above from matching a checkbox/heading/bullet shown as markdown
/// source inside a spec or plan's own example.
#[derive(Default)]
struct Fence {
    open: Option<(char, usize)>,
}

impl Fence {
    /// Feeds one already-left-trimmed line. Returns `true` when the line itself
    /// is a fence delimiter (opening or closing), so the caller should treat it
    /// as consumed rather than matching it as content.
    fn feed(&mut self, trimmed: &str) -> bool {
        let Some((ch, len)) = fence_delimiter(trimmed) else { return false };
        self.open = match self.open {
            Some((open_ch, open_len)) if open_ch == ch && len >= open_len => None,
            Some(_) => self.open,
            None => Some((ch, len)),
        };
        true
    }

    fn inside(&self) -> bool {
        self.open.is_some()
    }
}

/// The fence character and run length if `trimmed` opens or closes a fenced
/// code block (a run of 3 or more `` ` `` or `~`), else `None`.
fn fence_delimiter(trimmed: &str) -> Option<(char, usize)> {
    let ch = trimmed.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|&c| c == ch).count();
    (len >= 3).then_some((ch, len))
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
    fn checkbox_lines_carry_the_nearest_heading_and_a_per_heading_ordinal() {
        let text = "### Task 1: Adapters\n\n- [ ] write the trait\n- [x] write tests\n\n### Task 2: Import\n- [ ] wire routes\n";
        let items = checkboxes(text);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].heading.as_deref(), Some("Task 1: Adapters"));
        assert!(!items[0].checked);
        assert_eq!(items[0].ordinal, 1);
        assert!(items[1].checked);
        assert_eq!(items[1].heading.as_deref(), Some("Task 1: Adapters"));
        assert_eq!(items[1].ordinal, 2, "second checkbox under the same heading gets the next ordinal");
        assert_eq!(items[2].heading.as_deref(), Some("Task 2: Import"));
        assert_eq!(items[2].ordinal, 1, "ordinal resets under a new heading");
    }

    #[test]
    fn decisions_section_stops_at_next_heading() {
        let text = "## Decisions\n\n- keep it simple\n- ship it\n\n## Notes\n- not a decision\n";
        let bullets = section_bullets(text, "Decisions");
        assert_eq!(bullets.len(), 2);
        assert_eq!(bullets[0].0, "keep it simple");
        assert_eq!(bullets[0].2, 1);
        assert_eq!(bullets[1].2, 2, "second bullet in the section gets the next ordinal");
    }

    #[test]
    fn ruling_lines_read_the_prefix_and_are_numbered() {
        let text = "Spec: something.\nRuling: field is named X.\nRuling: also Y.\n";
        let rulings = ruling_lines(text);
        assert_eq!(rulings.len(), 2);
        assert_eq!(rulings[0].0, "field is named X.");
        assert_eq!(rulings[0].2, 1);
        assert_eq!(rulings[1].2, 2);
    }

    #[test]
    fn checkbox_and_heading_look_alikes_inside_a_fence_are_ignored() {
        let text = "### Task 1: Real\n\n- [ ] real task\n\n```\n### Task 2: Fake\n- [ ] fake task\n```\n\n- [x] another real task\n";
        let items = checkboxes(text);
        assert_eq!(items.len(), 2, "the checkbox and heading inside the fence must not be picked up");
        assert_eq!(items[0].text, "real task");
        assert_eq!(items[1].text, "another real task");
        // The fake heading inside the fence must not have become the tracked
        // heading: the second real item is still under "Task 1: Real".
        assert_eq!(items[1].heading.as_deref(), Some("Task 1: Real"));
        assert_eq!(items[1].ordinal, 2);
    }

    #[test]
    fn decisions_section_ignores_a_fenced_look_alike() {
        let text = "## Decisions\n\n- real decision\n\n~~~\n## Decisions\n- fake decision\n~~~\n";
        let bullets = section_bullets(text, "Decisions");
        assert_eq!(bullets.len(), 1);
        assert_eq!(bullets[0].0, "real decision");
    }

    #[test]
    fn ruling_line_inside_a_fence_is_ignored() {
        let text = "Ruling: real ruling.\n\n```\nRuling: fake ruling.\n```\n";
        let rulings = ruling_lines(text);
        assert_eq!(rulings.len(), 1);
        assert_eq!(rulings[0].0, "real ruling.");
    }
}
