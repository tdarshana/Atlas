/// Quotes a YAML plain scalar unless it is safe unquoted. Unsafe means: empty, a
/// `:` (which would split the line into another key) or a `#` (which would start
/// a comment) anywhere in it, padding a reader would trim, or a first character
/// YAML treats as syntax.
pub(crate) fn yaml_scalar(s: &str) -> String {
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
}
