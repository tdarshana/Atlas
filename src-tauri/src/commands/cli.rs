// The command line tool that ships inside the app. `atlas` (CLI, TUI and the `atlas mcp`
// shim) is bundled next to `atlasd`, so the dmg is the only install; Settings > Command
// line links `/usr/local/bin/atlas` to it the way editors install their `code` command.

use std::path::{Path, PathBuf};

use tauri::{Manager, Runtime};

/// Where the link goes. `/usr/local/bin` is on every macOS and Linux default `PATH`.
pub const LINK_PATH: &str = "/usr/local/bin/atlas";

/// The version the bundled `atlas` carries: the workspace shares one version, so the
/// app's own is the same number without running the binary.
pub const BUNDLED_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What Settings shows about the command line tool.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CliStatus {
    /// The bundled binary, or `None` in a build made without `scripts/prepare-sidecar.sh`.
    pub bundled: Option<String>,
    pub bundled_version: String,
    /// The path the install writes.
    pub link: String,
    /// What `link` currently points to (a symlink's target, or the path itself for a
    /// plain file), or `None` when nothing is there.
    pub link_target: Option<String>,
    /// True when `link` resolves to the bundled binary, so the install is done.
    pub linked_to_bundle: bool,
    /// Another `atlas` that most shells find before `/usr/local/bin`, or `None`.
    pub shadowed_by: Option<String>,
}

/// A binary the bundler placed next to the app binary as a sidecar (`atlasd`, `atlas`),
/// or in a dev build the one cargo left next to `atlas-desktop`. `None` when it is not
/// there, as in a build made without `scripts/prepare-sidecar.sh`.
pub fn sidecar_bin<R: Runtime>(app: &tauri::AppHandle<R>, name: &str) -> Option<PathBuf> {
    // `EXE_SUFFIX` is "" everywhere but Windows, where the bundled sidecar is `<name>.exe`.
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let path = tauri::process::current_binary(&app.env()).ok()?.with_file_name(file);
    path.exists().then_some(path)
}

/// The bundled `atlas`, the command line tool this file installs.
pub fn bundled_atlas<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    sidecar_bin(app, "atlas")
}

/// `~/.cargo/bin/atlas` when it exists: `cargo install` puts it there, and `~/.cargo/bin`
/// precedes `/usr/local/bin` on a shell set up by rustup, so that copy would win.
fn cargo_install_copy() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = Path::new(&home).join(".cargo").join("bin").join(format!("atlas{}", std::env::consts::EXE_SUFFIX));
    path.exists().then_some(path)
}

/// The status as a pure function of the file system facts, so it can be tested on a
/// scratch layout.
pub fn status_from(bundled: Option<&Path>, link: &Path, shadow: Option<&Path>) -> CliStatus {
    let link_target = std::fs::read_link(link).ok().or_else(|| link.exists().then(|| link.to_path_buf()));
    let linked_to_bundle = match (&link_target, bundled) {
        (Some(target), Some(bundled)) => same_file(target, bundled),
        _ => false,
    };
    CliStatus {
        bundled: bundled.map(|p| p.display().to_string()),
        bundled_version: BUNDLED_VERSION.to_string(),
        link: link.display().to_string(),
        link_target: link_target.map(|p| p.display().to_string()),
        linked_to_bundle,
        shadowed_by: shadow.map(|p| p.display().to_string()),
    }
}

/// True when both paths name one file, comparing canonical forms so a relative or a
/// `/private`-prefixed spelling on macOS still matches.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The shell command that installs the link, quoted for `sh`: the directory may not
/// exist on a fresh machine, and `-f` replaces whatever `cargo install` or an earlier
/// link left at the path.
pub fn link_command(bundled: &Path, link: &Path) -> String {
    let dir = link.parent().map(|p| p.display().to_string()).unwrap_or_else(|| "/usr/local/bin".to_string());
    format!("mkdir -p {} && ln -sfn {} {}", sh_quote(&dir), sh_quote(&bundled.display().to_string()), sh_quote(&link.display().to_string()))
}

/// Single-quotes a string for `sh`, closing and reopening around any quote inside it.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The AppleScript that runs `command` with an administrator prompt: the string is
/// double-quoted, so backslashes and double quotes inside it are escaped.
pub fn admin_script(command: &str) -> String {
    let escaped = command.replace('\\', "\\\\").replace('"', "\\\"");
    format!("do shell script \"{escaped}\" with administrator privileges")
}

/// Writes the link. Tries a plain symlink first (a `/usr/local/bin` the user owns, as
/// Homebrew leaves it); when that is refused, asks macOS for an administrator prompt.
pub fn install_link(bundled: &Path, link: &Path) -> Result<(), String> {
    if let Some(dir) = link.parent() {
        if dir.exists() {
            let _ = std::fs::remove_file(link);
            #[cfg(unix)]
            if std::os::unix::fs::symlink(bundled, link).is_ok() {
                return Ok(());
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let script = admin_script(&link_command(bundled, link));
        let out = std::process::Command::new("osascript").arg("-e").arg(&script).output().map_err(|e| format!("could not run osascript: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("-128") {
            return Err("Cancelled.".to_string());
        }
        Err(format!("The link was not written: {}", err.trim()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(format!("Could not write {}: run `sudo {}` in a terminal.", link.display(), link_command(bundled, link)))
    }
}

/// The state of the command line tool, for Settings.
#[tauri::command]
pub fn cli_status<R: Runtime>(app: tauri::AppHandle<R>) -> CliStatus {
    status_from(bundled_atlas(&app).as_deref(), Path::new(LINK_PATH), cargo_install_copy().as_deref())
}

/// Links `/usr/local/bin/atlas` to the bundled binary and answers the new state.
#[tauri::command]
pub async fn cli_install<R: Runtime>(app: tauri::AppHandle<R>) -> Result<CliStatus, String> {
    let bundled = bundled_atlas(&app).ok_or_else(|| "This build carries no atlas binary; install it with cargo instead.".to_string())?;
    let link = PathBuf::from(LINK_PATH);
    tauri::async_runtime::spawn_blocking(move || install_link(&bundled, &link)).await.map_err(|e| e.to_string())??;
    Ok(cli_status(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reports_a_link_to_the_bundle_and_one_to_something_else() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = dir.path().join("atlas.app").join("atlas");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"bin").unwrap();
        let other = dir.path().join("other-atlas");
        std::fs::write(&other, b"bin").unwrap();
        let link = dir.path().join("bin").join("atlas");

        let none = status_from(Some(&bundled), &link, None);
        assert_eq!(none.link_target, None);
        assert!(!none.linked_to_bundle);
        assert_eq!(none.bundled_version, BUNDLED_VERSION);

        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&other, &link).unwrap();
        let elsewhere = status_from(Some(&bundled), &link, Some(&other));
        assert_eq!(elsewhere.link_target.as_deref(), Some(other.to_str().unwrap()));
        assert!(!elsewhere.linked_to_bundle);
        assert_eq!(elsewhere.shadowed_by.as_deref(), Some(other.to_str().unwrap()));

        install_link(&bundled, &link).unwrap();
        let done = status_from(Some(&bundled), &link, None);
        assert!(done.linked_to_bundle, "{done:?}");
        assert_eq!(std::fs::read_link(&link).unwrap(), bundled);
    }

    #[test]
    fn no_bundled_binary_is_reported_not_linked() {
        let dir = tempfile::tempdir().unwrap();
        let s = status_from(None, &dir.path().join("atlas"), None);
        assert_eq!(s.bundled, None);
        assert!(!s.linked_to_bundle);
    }

    #[test]
    fn the_link_command_quotes_paths_for_sh_and_the_script_for_applescript() {
        let cmd = link_command(Path::new("/Applications/My Apps/atlas.app/Contents/MacOS/atlas"), Path::new("/usr/local/bin/atlas"));
        assert_eq!(cmd, "mkdir -p '/usr/local/bin' && ln -sfn '/Applications/My Apps/atlas.app/Contents/MacOS/atlas' '/usr/local/bin/atlas'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        let script = admin_script("echo \"hi\" \\ there");
        assert_eq!(script, "do shell script \"echo \\\"hi\\\" \\\\ there\" with administrator privileges");
    }
}
