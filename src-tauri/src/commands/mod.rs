// The desktop's Tauri commands, one file per concern. Every command is re-exported here
// so `lib.rs`'s `generate_handler!` list and the tests keep their names.

pub mod about;
pub mod cli;
pub mod notify;
pub mod permissions;
pub mod platform;
pub mod update;
pub mod vault;
pub mod window;

pub use about::{about_info, about_menu_refresh, log_dir, open_log_folder};
pub use cli::{cli_install, cli_status};
#[cfg(target_os = "macos")]
pub use about::install_app_menu;
pub use notify::{notification_permission, notification_request_permission, notify};
pub use update::{update_check, update_install, UpdateState};
pub use vault::{
    vault_list, vault_lock, vault_put_key, vault_reapply, vault_set_passphrase, vault_status, vault_unlock, VaultState,
};
pub use window::{
    app_exit, app_relaunch, autostart_get, autostart_set, clipboard_write, install_shortcut, shortcut_set, ui_state_all,
    ui_state_get, ui_state_set, window_center, window_move, ShortcutRegistration,
};

/// Shared by the store and vault tests, which both point `HOME` at a scratch directory.
#[cfg(test)]
pub(crate) mod test_support {
    /// Serializes every test that points `HOME` at a scratch directory: `HOME` is
    /// process-global and `cargo test` runs this binary's tests on several threads by
    /// default, so two such tests running at once would each see the other's directory.
    /// Held for the life of the [`HomeGuard`] that acquired it.
    static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A guard that points `HOME` at a scratch directory for the life of one test and
    /// restores it on drop, so the store and vault commands resolve the app data dir
    /// under a throwaway path instead of the real user's home. `_lock` is never read;
    /// it exists only to hold `HOME_LOCK` until this guard drops.
    pub(crate) struct HomeGuard {
        previous: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl HomeGuard {
        pub(crate) fn set(dir: &std::path::Path) -> Self {
            let _lock = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let previous = std::env::var_os("HOME");
            // SAFETY: serialized against every other `HomeGuard` by `HOME_LOCK`, held
            // until this guard drops.
            unsafe { std::env::set_var("HOME", dir) };
            Self { previous, _lock }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            // SAFETY: as above.
            unsafe {
                match &self.previous {
                    Some(v) => std::env::set_var("HOME", v),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// The host reaches the daemon through `atlas_client::RemoteBackend` and nothing
    /// else: no file under `src-tauri/src` builds its own `reqwest` client, except
    /// `plugins/install.rs`, which downloads plugin tarballs from the network rather
    /// than talking to the daemon.
    #[test]
    fn only_the_plugin_installer_builds_a_reqwest_client() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read src-tauri/src") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&root, &mut files);
        assert!(files.len() > 5, "expected the desktop's sources under {}", root.display());
        // Spelled in two halves so this file does not match its own needle.
        let needles = [format!("{}::Client", "reqwest"), format!("{}::blocking", "reqwest")];
        let offenders: Vec<String> = files
            .iter()
            .filter(|f| !f.ends_with("plugins/install.rs"))
            .filter(|f| {
                let src = std::fs::read_to_string(f).expect("read source");
                needles.iter().any(|n| src.contains(n.as_str()))
            })
            .map(|f| f.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        assert!(offenders.is_empty(), "files building their own HTTP client instead of using RemoteBackend: {offenders:?}");
    }
}
