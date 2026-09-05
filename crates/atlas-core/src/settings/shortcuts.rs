//! `ui.global_shortcut` validation: the accelerator grammar
//! `tauri-plugin-global-shortcut` accepts, checked at the settings boundary so a value
//! that could never register with the desktop app is refused before it is stored.
//! Kept beside, not inside, the key registry in `settings.rs`: it is a desktop rule,
//! and the registry should stay a list of keys and their types.

use crate::{AtlasError, Result};

/// The modifier names `tauri-plugin-global-shortcut` accepts, lower-cased for a
/// case-insensitive match.
const ACCELERATOR_MODIFIERS: &[&str] =
    &["cmd", "command", "cmdorctrl", "commandorcontrol", "ctrl", "control", "alt", "altgr", "option", "shift", "super", "meta"];

/// Whether `s` is a Tauri accelerator: one or more modifiers and exactly one trailing
/// key, joined by `+` (e.g. `"CmdOrCtrl+Shift+K"`). Checked here, before `ui.global_shortcut`
/// ever reaches the desktop app's `shortcut_set` command, so a value that could never
/// register with the global-shortcut plugin is refused at the settings boundary instead
/// of failing silently in the running app.
pub fn validate_accelerator(s: &str) -> Result<()> {
    let bad = || AtlasError::Invalid(format!("'{s}' is not a valid shortcut: use one or more modifiers and one key joined by '+', e.g. 'CmdOrCtrl+Shift+K'"));
    let parts: Vec<&str> = s.trim().split('+').map(str::trim).collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return Err(bad());
    }
    let (modifiers, key) = parts.split_at(parts.len() - 1);
    for m in modifiers {
        if !ACCELERATOR_MODIFIERS.contains(&m.to_lowercase().as_str()) {
            return Err(bad());
        }
    }
    if ACCELERATOR_MODIFIERS.contains(&key[0].to_lowercase().as_str()) {
        return Err(bad());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accelerator_accepts_modifiers_plus_one_key() {
        assert!(validate_accelerator("CmdOrCtrl+Shift+K").is_ok());
        assert!(validate_accelerator("Alt+Space").is_ok());
        assert!(validate_accelerator(" Ctrl + Shift + P ").is_ok(), "surrounding whitespace is trimmed");
        assert!(validate_accelerator("ctrl+shift+k").is_ok(), "modifiers are case-insensitive");
    }

    #[test]
    fn validate_accelerator_rejects_a_bare_key_or_a_key_less_combo() {
        assert!(validate_accelerator("K").is_err(), "no modifier");
        assert!(validate_accelerator("").is_err());
        assert!(validate_accelerator("Cmd+").is_err(), "trailing separator with no key");
        assert!(validate_accelerator("Cmd+Shift").is_err(), "ends on a modifier, not a key");
        assert!(validate_accelerator("Bogus+K").is_err(), "unknown modifier");
    }
}
