//! `ui.theme_pack` validation.
//!
//! A pack is `{ "name", "base": "dark"|"light", "tokens": { "--token": "value" } }`. It
//! may only override colour tokens and the two radius tokens (`--radius-sm`,
//! `--radius-md`); the allowed names are read out of the same token files the desktop
//! app ships (`colors.css`, `spacing.css`) rather than hand-copied here, so an added or
//! renamed token follows automatically instead of silently going stale. This is the
//! one place the core crate knows the desktop's design tokens, which is why it is a
//! module of its own rather than part of the key registry in `settings.rs`.

use std::collections::HashSet;

use serde_json::Value;

use crate::{AtlasError, Result};

const COLORS_CSS: &str = include_str!("../../../../src/lib/ds/tokens/colors.css");
const SPACING_CSS: &str = include_str!("../../../../src/lib/ds/tokens/spacing.css");

/// The only two length tokens a pack may override, per the design requirements
/// (inputs/buttons and cards/popovers). Checked against `spacing.css` itself, not just
/// asserted, so a rename there fails loudly instead of this list going stale.
const RADIUS_TOKEN_NAMES: &[&str] = &["--radius-sm", "--radius-md"];

/// A pack's JSON text is capped so a client cannot write an unbounded blob into the
/// setting (the token allow list itself is finite, but `name` is a free string).
/// Matches `MAX_THEME_PACK_BYTES` in `src/lib/shell/theme-pack.ts`.
const MAX_THEME_PACK_BYTES: usize = 16 * 1024;

/// `name`'s own cap, tighter than the whole-pack one since it is shown in the Theme
/// select. Matches `MAX_THEME_PACK_NAME_CHARS` in `src/lib/shell/theme-pack.ts`.
const MAX_THEME_PACK_NAME_CHARS: usize = 64;

/// Strips `/* ... */` comments out of a CSS file so a colon inside a comment (e.g. an
/// "AA fix: ..." note) is never mistaken for part of a declaration.
fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = match rest[start + 2..].find("*/") {
            Some(end) => &rest[start + 2 + end + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Every `--name: value;` custom property declared in a CSS file, in source order.
fn parse_declarations(css: &str) -> Vec<(String, String)> {
    let cleaned = strip_css_comments(css);
    let mut out = Vec::new();
    for chunk in cleaned.split(';') {
        let chunk = chunk.trim();
        let Some(rest) = chunk.strip_prefix("--") else { continue };
        let Some((name, value)) = rest.split_once(':') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if !name.is_empty() && !value.is_empty() {
            out.push((format!("--{name}"), value.to_string()));
        }
    }
    out
}

/// The set of custom properties a theme pack may override: every token in
/// `colors.css` whose declared value is a literal colour (not a `var()` alias or a
/// shadow), plus the two named radius tokens, checked present in `spacing.css`.
pub(super) fn theme_token_allowlist() -> HashSet<String> {
    let mut names: HashSet<String> = HashSet::new();
    for (name, value) in parse_declarations(COLORS_CSS) {
        if is_css_color(&value) {
            names.insert(name);
        }
    }
    for radius in RADIUS_TOKEN_NAMES {
        if parse_declarations(SPACING_CSS).iter().any(|(name, _)| name == radius) {
            names.insert((*radius).to_string());
        }
    }
    names
}

/// Whether `value` is a CSS colour: `#rgb`, `#rrggbb`, `#rrggbbaa`, or a
/// `rgb()`/`rgba()`/`hsl()`/`hsla()`/`oklch()` function call. Not a full CSS grammar
/// check, just enough to keep a pack from writing an arbitrary declaration into a
/// custom property.
fn is_css_color(value: &str) -> bool {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    ["rgb(", "rgba(", "hsl(", "hsla(", "oklch("].iter().any(|p| v.starts_with(p) && v.ends_with(')'))
}

/// Whether `value` is a plain CSS length (`3px`, `0.5rem`, `0`), the shape the two
/// radius tokens take. Matches `isCssLength`'s `/^\d+(\.\d+)?(px|rem|em|%)$/` in
/// `src/lib/shell/theme-pack.ts`: a digit must lead, and a `.` must be followed by at
/// least one digit, so `.px` and `3.` are rejected along with everything else that
/// is not a plain number.
fn is_css_length(value: &str) -> bool {
    let v = value.trim();
    if v == "0" {
        return true;
    }
    for unit in ["px", "rem", "em", "%"] {
        if let Some(num) = v.strip_suffix(unit) {
            let (int_part, frac_part) = match num.split_once('.') {
                Some((i, f)) => (i, Some(f)),
                None => (num, None),
            };
            let int_ok = !int_part.is_empty() && int_part.chars().all(|c| c.is_ascii_digit());
            let frac_ok = frac_part.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()));
            if int_ok && frac_ok {
                return true;
            }
        }
    }
    false
}

/// Validates a theme pack's JSON text against the shape `{ name, base, tokens }`, the
/// known token allow list, and colour/length syntax for each value, before it is ever
/// stored: a pack that failed this can never reach `documentElement.style`.
pub(super) fn validate_theme_pack(s: &str) -> Result<()> {
    let bad = |msg: String| AtlasError::Invalid(msg);
    if s.len() > MAX_THEME_PACK_BYTES {
        return Err(bad(format!("ui.theme_pack must be at most {MAX_THEME_PACK_BYTES} bytes of JSON")));
    }
    let v: Value = serde_json::from_str(s).map_err(|e| bad(format!("ui.theme_pack must be valid JSON: {e}")))?;
    let obj = v.as_object().ok_or_else(|| bad("ui.theme_pack must be a JSON object".into()))?;
    match obj.get("name").and_then(|n| n.as_str()) {
        Some(n) if n.trim().is_empty() => return Err(bad("ui.theme_pack.name must be a non-empty string".into())),
        Some(n) if n.chars().count() > MAX_THEME_PACK_NAME_CHARS => {
            return Err(bad(format!("ui.theme_pack.name must be at most {MAX_THEME_PACK_NAME_CHARS} characters")));
        }
        Some(_) => {}
        None => return Err(bad("ui.theme_pack.name must be a non-empty string".into())),
    }
    match obj.get("base").and_then(|b| b.as_str()) {
        Some("dark") | Some("light") => {}
        _ => return Err(bad("ui.theme_pack.base must be \"dark\" or \"light\"".into())),
    }
    let tokens = obj.get("tokens").and_then(|t| t.as_object()).ok_or_else(|| bad("ui.theme_pack.tokens must be an object".into()))?;
    let allowed = theme_token_allowlist();
    for (name, value) in tokens {
        if !allowed.contains(name.as_str()) {
            return Err(bad(format!("ui.theme_pack.tokens has an unknown token '{name}'")));
        }
        let value = value.as_str().ok_or_else(|| bad(format!("ui.theme_pack.tokens['{name}'] must be a string")))?;
        let ok = if RADIUS_TOKEN_NAMES.contains(&name.as_str()) { is_css_length(value) } else { is_css_color(value) };
        if !ok {
            let want = if RADIUS_TOKEN_NAMES.contains(&name.as_str()) { "a CSS length" } else { "a CSS colour" };
            return Err(bad(format!("ui.theme_pack.tokens['{name}'] must be {want}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_token_allowlist_has_colours_and_the_two_radius_tokens_but_not_aliases_or_shadows() {
        let allowed = theme_token_allowlist();
        for name in ["--bg-base", "--bg-surface", "--accent", "--accent-muted", "--text-primary", "--border-strong"] {
            assert!(allowed.contains(name), "{name} should be a colour token");
        }
        assert!(allowed.contains("--radius-sm"));
        assert!(allowed.contains("--radius-md"));
        // Not offered: aliases resolve through `var()` rather than a literal colour,
        // shadows are not colours, and radius-lg is a modal radius, not one of the
        // two the design requirements call out.
        assert!(!allowed.contains("--surface-window"), "aliases are var() references, not literal colours");
        assert!(!allowed.contains("--shadow-sm"), "shadows are not colours");
        assert!(!allowed.contains("--radius-lg"), "only radius-sm and radius-md are offered");
    }

    #[test]
    fn is_css_color_accepts_hex_and_function_forms_and_rejects_everything_else() {
        for good in ["#fff", "#ffff", "#4C8DF6", "#4C8DF6AA", "rgb(1,2,3)", "rgba(1,2,3,0.5)", "hsl(1,2%,3%)", "oklch(0.5 0.1 200)"] {
            assert!(is_css_color(good), "{good} should be a valid colour");
        }
        for bad in ["red", "3px", "#gggggg", "#12345", "var(--bg-base)", ""] {
            assert!(!is_css_color(bad), "{bad} should not be a valid colour");
        }
    }

    #[test]
    fn is_css_length_accepts_plain_lengths_and_rejects_everything_else() {
        // Same literal lists as `isCssLength`'s test in `src/lib/shell/theme-pack.test.ts`.
        for good in ["0", "3px", "0.5rem", "12em", "50%"] {
            assert!(is_css_length(good), "{good} should be a valid length");
        }
        for bad in ["px", "3", "3xy", "-3px-", "", ".px", ".5rem", "3."] {
            assert!(!is_css_length(bad), "{bad} should not be a valid length");
        }
    }

    #[test]
    fn validate_theme_pack_accepts_a_well_formed_pack() {
        let pack = r##"{"name":"Ocean","base":"light","tokens":{"--accent":"#2563EB","--bg-base":"rgba(0,0,0,0.1)","--radius-md":"6px"}}"##;
        assert!(validate_theme_pack(pack).is_ok());
    }

    #[test]
    fn validate_theme_pack_accepts_a_name_at_exactly_64_characters() {
        let name = "x".repeat(64);
        let pack = format!(r#"{{"name":"{name}","base":"dark","tokens":{{}}}}"#);
        assert!(validate_theme_pack(&pack).is_ok());
    }

    #[test]
    fn validate_theme_pack_rejects_a_name_over_64_characters() {
        let name = "x".repeat(65);
        let pack = format!(r#"{{"name":"{name}","base":"dark","tokens":{{}}}}"#);
        assert!(validate_theme_pack(&pack).is_err());
    }

    #[test]
    fn validate_theme_pack_rejects_json_over_16kb() {
        let padding = "x".repeat(17 * 1024);
        let pack = format!(r#"{{"name":"x","base":"dark","tokens":{{}},"padding":"{padding}"}}"#);
        assert!(validate_theme_pack(&pack).is_err());
    }
}
