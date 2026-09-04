// The macOS permissions Atlas needs, read live and reported to the Permissions view.
//
// Four rows: notifications, Automation for Finder (the dmg bundling step drives Finder),
// Accessibility (the global shortcut while the app is in the background) and files and
// folders (the connected project roots and the app data directory). Only the first and
// the last mean anything off macOS; the two macOS-only ones report `not_applicable`
// there rather than inventing an answer.
//
// Nothing here changes system state. `permissions_status` only reads; `permission_request`
// may raise the system's own consent prompt, which is the whole point of the button.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{Manager, Runtime};
use tauri_plugin_notification::NotificationExt;

/// The four rows, by id. The web side names the same strings.
pub const NOTIFICATIONS: &str = "notifications";
pub const AUTOMATION_FINDER: &str = "automation-finder";
pub const ACCESSIBILITY: &str = "accessibility";
pub const FILES: &str = "files";

/// What one permission looks like right now. `granted` is one of `granted`, `denied`,
/// `not_determined`, `unknown` or `not_applicable`; `detail` carries the reason whenever
/// the status alone would not explain itself (a failing path, an unmapped OS code).
#[derive(Debug, Clone, Serialize)]
pub struct PermissionStatus {
    pub id: String,
    pub granted: String,
    pub detail: Option<String>,
}

impl PermissionStatus {
    fn new(id: &str, granted: &str, detail: Option<String>) -> Self {
        Self { id: id.to_string(), granted: granted.to_string(), detail }
    }
}

/// The notification plugin's `PermissionState`, in this module's vocabulary. `Prompt` and
/// `PromptWithRationale` are both "the user has not decided yet".
fn notifications_from(state: tauri::plugin::PermissionState) -> PermissionStatus {
    use tauri::plugin::PermissionState;
    let granted = match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::Prompt | PermissionState::PromptWithRationale => "not_determined",
    };
    PermissionStatus::new(NOTIFICATIONS, granted, None)
}

fn notifications_status<R: Runtime>(app: &tauri::AppHandle<R>) -> PermissionStatus {
    match app.notification().permission_state() {
        Ok(state) => notifications_from(state),
        Err(e) => PermissionStatus::new(NOTIFICATIONS, "unknown", Some(e.to_string())),
    }
}

/// Reads every connected project root and writes a probe file in the app data directory.
/// The first failure decides the row, and its path rides along in `detail` so the user
/// knows which folder to grant rather than being told "files and folders" and nothing else.
fn files_status(app_data: &Path, roots: &[String]) -> PermissionStatus {
    for root in roots {
        let path = PathBuf::from(root);
        if !path.exists() {
            // A folder that has been moved or unmounted is not a permission problem, and
            // reporting it as denied would send the user to the wrong settings pane.
            continue;
        }
        if let Err(e) = std::fs::read_dir(&path) {
            return PermissionStatus::new(FILES, "denied", Some(format!("{root}: {e}")));
        }
    }

    if let Err(e) = std::fs::create_dir_all(app_data) {
        return PermissionStatus::new(FILES, "denied", Some(format!("{}: {e}", app_data.display())));
    }
    let probe = app_data.join(".atlas-permission-probe");
    if let Err(e) = std::fs::write(&probe, b"atlas") {
        return PermissionStatus::new(FILES, "denied", Some(format!("{}: {e}", probe.display())));
    }
    // The probe has served its purpose; a leftover dotfile in the app data directory has not.
    let _ = std::fs::remove_file(&probe);
    PermissionStatus::new(FILES, "granted", None)
}

// -- macOS -------------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    /// `AEDesc`: a four-character type code and an opaque data handle.
    #[repr(C)]
    struct AeDesc {
        descriptor_type: u32,
        data_handle: *mut c_void,
    }

    /// `typeApplicationBundleID`, the descriptor type that names a target app by bundle id.
    const TYPE_APPLICATION_BUNDLE_ID: u32 = u32::from_be_bytes(*b"bund");
    /// `typeWildCard`: any event class, any event id. The question is whether this process
    /// may drive the target at all, not whether one particular event is allowed.
    const TYPE_WILDCARD: u32 = u32::from_be_bytes(*b"****");

    /// The bundle id of the app the dmg bundling step drives.
    const FINDER_BUNDLE_ID: &[u8] = b"com.apple.finder";

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn AECreateDesc(
            type_code: u32,
            data_ptr: *const c_void,
            data_size: isize,
            result: *mut AeDesc,
        ) -> i16;
        fn AEDisposeDesc(desc: *mut AeDesc) -> i16;
        fn AEDeterminePermissionToAutomateTarget(
            target: *const AeDesc,
            event_class: u32,
            event_id: u32,
            ask_user_if_needed: u8,
        ) -> i32;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: *const c_void;
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        static kCFBooleanTrue: *const c_void;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_call_backs: *const c_void,
            value_call_backs: *const c_void,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    /// The raw `OSStatus` from asking whether this process may drive the Finder.
    /// `ask_user_if_needed` true is what raises the system's consent prompt.
    pub fn automation_os_status(ask_user_if_needed: bool) -> i32 {
        let mut desc = AeDesc { descriptor_type: 0, data_handle: std::ptr::null_mut() };
        // SAFETY: `desc` is a live, correctly shaped `AEDesc`, and the bundle id is a
        // borrowed byte slice that outlives the call.
        let created = unsafe {
            AECreateDesc(
                TYPE_APPLICATION_BUNDLE_ID,
                FINDER_BUNDLE_ID.as_ptr().cast(),
                FINDER_BUNDLE_ID.len() as isize,
                &mut desc,
            )
        };
        if created != 0 {
            return created as i32;
        }
        // SAFETY: `desc` was created above and is disposed of before this function returns.
        let status = unsafe {
            AEDeterminePermissionToAutomateTarget(
                &desc,
                TYPE_WILDCARD,
                TYPE_WILDCARD,
                u8::from(ask_user_if_needed),
            )
        };
        // SAFETY: same descriptor, disposed exactly once.
        unsafe { AEDisposeDesc(&mut desc) };
        status
    }

    /// Whether this process is in the Accessibility list.
    pub fn accessibility_trusted() -> bool {
        // SAFETY: no arguments, no state.
        unsafe { AXIsProcessTrusted() != 0 }
    }

    /// The same question, but asking the system to show its "open System Settings" prompt
    /// when the answer is no. Returns the trusted state as it stands, which is still false
    /// right after the prompt appears: the user grants it in System Settings, and the
    /// page's poll picks the change up.
    pub fn accessibility_trusted_with_prompt() -> bool {
        // SAFETY: a one-entry `CFDictionary` built from CoreFoundation's own type
        // callbacks and released before returning.
        unsafe {
            let key = kAXTrustedCheckOptionPrompt;
            let value = kCFBooleanTrue;
            let options = CFDictionaryCreate(
                std::ptr::null(),
                &key,
                &value,
                1,
                std::ptr::addr_of!(kCFTypeDictionaryKeyCallBacks),
                std::ptr::addr_of!(kCFTypeDictionaryValueCallBacks),
            );
            if options.is_null() {
                return AXIsProcessTrusted() != 0;
            }
            let trusted = AXIsProcessTrustedWithOptions(options) != 0;
            CFRelease(options);
            trusted
        }
    }
}

/// `noErr`: this process may drive the target.
const NO_ERR: i32 = 0;
/// `procNotFound`: the target is not running, so the system cannot answer.
const PROC_NOT_FOUND: i32 = -600;
/// `errAEEventNotPermitted`: the user said no, or the grant was revoked.
const ERR_AE_EVENT_NOT_PERMITTED: i32 = -1743;
/// `errAEEventWouldRequireUserConsent`: nobody has been asked yet.
const ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT: i32 = -1744;

/// The four statuses an `AEDeterminePermissionToAutomateTarget` result maps onto. Pure,
/// so the mapping is tested without a Finder or a TCC database in the loop.
pub fn automation_status_from(code: i32) -> PermissionStatus {
    match code {
        NO_ERR => PermissionStatus::new(AUTOMATION_FINDER, "granted", None),
        ERR_AE_EVENT_NOT_PERMITTED => PermissionStatus::new(AUTOMATION_FINDER, "denied", None),
        ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT => {
            PermissionStatus::new(AUTOMATION_FINDER, "not_determined", None)
        }
        PROC_NOT_FOUND => PermissionStatus::new(
            AUTOMATION_FINDER,
            "unknown",
            Some("The Finder is not running, so macOS cannot answer yet.".into()),
        ),
        other => PermissionStatus::new(
            AUTOMATION_FINDER,
            "unknown",
            Some(format!("macOS answered with status {other}.")),
        ),
    }
}

#[cfg(target_os = "macos")]
fn automation_status(ask: bool) -> PermissionStatus {
    automation_status_from(mac::automation_os_status(ask))
}

#[cfg(not(target_os = "macos"))]
fn automation_status(_ask: bool) -> PermissionStatus {
    PermissionStatus::new(AUTOMATION_FINDER, "not_applicable", None)
}

#[cfg(target_os = "macos")]
fn accessibility_status(ask: bool) -> PermissionStatus {
    let trusted =
        if ask { mac::accessibility_trusted_with_prompt() } else { mac::accessibility_trusted() };
    // macOS has no "not determined" here: a process is in the list or it is not.
    PermissionStatus::new(ACCESSIBILITY, if trusted { "granted" } else { "denied" }, None)
}

#[cfg(not(target_os = "macos"))]
fn accessibility_status(_ask: bool) -> PermissionStatus {
    PermissionStatus::new(ACCESSIBILITY, "not_applicable", None)
}

fn app_data_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// Every permission row, in the order the view draws them. `roots` are the connected
/// project roots the app already holds; the files row probes those and the app data
/// directory rather than guessing which folders matter.
#[tauri::command]
pub async fn permissions_status<R: Runtime>(
    app: tauri::AppHandle<R>,
    roots: Vec<String>,
) -> Result<Vec<PermissionStatus>, String> {
    let app_data = app_data_dir(&app)?;
    let notifications = notifications_status(&app);
    // The filesystem probes and the Apple Event round trip both block; the notification
    // state is a cheap in-process read, so it is taken before the hop.
    tauri::async_runtime::spawn_blocking(move || {
        vec![
            notifications,
            automation_status(false),
            accessibility_status(false),
            files_status(&app_data, &roots),
        ]
    })
    .await
    .map_err(|e| e.to_string())
}

/// Asks for one permission, which on macOS means letting the system raise its own consent
/// prompt. Answers with that permission's status as it stands once the call returns; a
/// grant made in System Settings lands on the page's next poll rather than here.
#[tauri::command]
pub async fn permission_request<R: Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
) -> Result<PermissionStatus, String> {
    match id.as_str() {
        NOTIFICATIONS => match app.notification().request_permission() {
            Ok(state) => Ok(notifications_from(state)),
            Err(e) => Err(e.to_string()),
        },
        AUTOMATION_FINDER => tauri::async_runtime::spawn_blocking(|| automation_status(true))
            .await
            .map_err(|e| e.to_string()),
        ACCESSIBILITY => Ok(accessibility_status(true)),
        FILES => {
            let app_data = app_data_dir(&app)?;
            // Nothing to prompt for: macOS grants folder access when the app reads the
            // folder, so the probe is the request.
            tauri::async_runtime::spawn_blocking(move || files_status(&app_data, &[]))
                .await
                .map_err(|e| e.to_string())
        }
        other => Err(format!("'{other}' is not a permission Atlas tracks.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "atlas-desktop-permissions-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn automation_maps_the_four_apple_event_outcomes() {
        assert_eq!(automation_status_from(0).granted, "granted");
        assert_eq!(automation_status_from(-1743).granted, "denied");
        assert_eq!(automation_status_from(-1744).granted, "not_determined");

        let missing = automation_status_from(-600);
        assert_eq!(missing.granted, "unknown");
        assert!(missing.detail.unwrap().contains("Finder"));

        let odd = automation_status_from(-42);
        assert_eq!(odd.granted, "unknown");
        assert!(odd.detail.unwrap().contains("-42"));
        assert_eq!(automation_status_from(0).id, AUTOMATION_FINDER);
    }

    #[test]
    fn files_is_granted_when_the_roots_read_and_the_app_data_dir_writes() {
        let app_data = scratch_dir("files-ok");
        let root = scratch_dir("files-root");
        let status = files_status(&app_data, &[root.to_string_lossy().into_owned()]);
        assert_eq!(status.granted, "granted");
        assert_eq!(status.id, FILES);
        // The probe file does not outlive the check.
        assert!(!app_data.join(".atlas-permission-probe").exists());
    }

    #[test]
    fn a_root_that_is_gone_is_not_a_denial() {
        let app_data = scratch_dir("files-missing-root");
        let status = files_status(&app_data, &["/no/such/folder/anywhere".into()]);
        assert_eq!(status.granted, "granted");
    }

    #[test]
    fn a_root_that_cannot_be_read_is_denied_and_names_the_path() {
        let app_data = scratch_dir("files-unreadable");
        // A file is not a directory, so `read_dir` fails on it the way an unreadable
        // folder does, without needing a chmod that a CI runner may ignore as root.
        let file = app_data.join("not-a-folder");
        std::fs::write(&file, b"x").unwrap();
        let status = files_status(&app_data, &[file.to_string_lossy().into_owned()]);
        assert_eq!(status.granted, "denied");
        assert!(status.detail.unwrap().contains("not-a-folder"));
    }
}
