use std::path::Path;
use std::process::{Command, Stdio};

/// Builds a small git-backed fixture project for the CLI tests: a Next.js/React
/// `package.json`, a README, a TypeScript source file, a gitignored
/// `node_modules` dir, one commit, and an `origin` remote. A copy of the helper
/// in `atlas-core/tests/common`, which a test in another crate cannot reach.
pub fn fixture_repo(dir: &Path) {
    run(dir, &["init"]);

    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"fixture","dependencies":{"next":"16.0.0","react":"19.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(dir.join("README.md"), "# fixture\n\nA test fixture.\nSecond line.\n").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/index.ts"), "export const x = 1;\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "node_modules/\n").unwrap();
    std::fs::create_dir_all(dir.join("node_modules")).unwrap();
    std::fs::write(dir.join("node_modules/x.js"), "// ignored\n").unwrap();

    run(dir, &["add", "-A"]);
    run(dir, &["commit", "-m", "initial fixture"]);
    run(
        dir,
        &["remote", "add", "origin", "https://github.com/example/fixture.git"],
    );
}

fn run(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Atlas Test")
        .env("GIT_AUTHOR_EMAIL", "atlas-test@example.com")
        .env("GIT_COMMITTER_NAME", "Atlas Test")
        .env("GIT_COMMITTER_EMAIL", "atlas-test@example.com")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap_or_else(|e| panic!("failed to run git {:?}: {e}", args));
    assert!(status.success(), "git {:?} failed", args);
}

/// A temporary `ATLAS_HOME` and a free port, plus the promise that whatever
/// daemon the test started is stopped again. Stopping in `Drop` and not at the
/// end of the test body is the point: a failed assertion unwinds, and without
/// this an atlasd would outlive the run and hold its DuckDB file open.
pub struct TestDaemon {
    pub home: tempfile::TempDir,
    pub port: u16,
}

impl TestDaemon {
    pub fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        Self { home, port }
    }

    /// An `atlas` command already pointed at this home and port. `PATH` carries
    /// the build directory so the CLI can find the `atlasd` next to it.
    pub fn cmd(&self) -> Command {
        let exe = Path::new(env!("CARGO_BIN_EXE_atlas"));
        let mut c = Command::new(exe);
        c.env("ATLAS_HOME", self.home.path())
            .env("ATLAS_PORT", self.port.to_string())
            .env("ATLAS_NO_EMBED", "1")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    exe.parent().unwrap().display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            );
        c
    }

    /// Asks the CLI to stop the daemon, then makes sure it is really gone.
    /// Both halves ignore their errors: this runs while a test may already be
    /// panicking, and there may be no daemon to stop at all.
    pub fn stop(&self) {
        let _ = self.cmd().args(["daemon", "stop"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        self.kill_leftover();
    }

    /// The fallback for a daemon that `daemon stop` could not reach: signal the
    /// pid `daemon.json` names, but only once `ps` agrees it is still an atlasd.
    /// The file outlives a crash, so the pid in it may have been recycled.
    fn kill_leftover(&self) {
        let Ok(text) = std::fs::read_to_string(self.home.path().join("daemon.json")) else { return };
        let Ok(info) = serde_json::from_str::<serde_json::Value>(&text) else { return };
        let Some(pid) = info["pid"].as_u64() else { return };
        let is_atlasd = Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("atlasd"))
            .unwrap_or(false);
        if is_atlasd {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.stop();
    }
}
