use std::path::PathBuf;
use std::time::{Duration, Instant};
use atlas_core::paths::AtlasPaths;

pub fn daemon_info(paths: &AtlasPaths) -> Option<serde_json::Value> { std::fs::read_to_string(paths.daemon_file()).ok().and_then(|s| serde_json::from_str(&s).ok()) }

pub async fn is_up(port: u16) -> bool {
    reqwest::Client::builder().timeout(Duration::from_millis(500)).build().unwrap().get(format!("http://127.0.0.1:{port}/api/v1/status")).send().await.map(|r| r.status().is_success()).unwrap_or(false)
}

fn atlasd_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() { let sib = exe.with_file_name("atlasd"); if sib.exists() { return sib; } }
    PathBuf::from("atlasd")
}

pub async fn ensure_daemon(paths: &AtlasPaths, port: u16) -> anyhow::Result<u16> {
    ensure_daemon_with(paths, port, None).await
}

/// Like [`ensure_daemon`], but spawns an explicitly named `atlasd`. The desktop app passes
/// its bundled sidecar so a packaged install works on a machine with no `atlasd` on PATH.
/// `None` keeps the default search: next to the current executable, then PATH.
/// How long a caller waits, in short steps, for another caller's already-in-flight
/// daemon start to announce itself (`daemon.json` written, or the port answering)
/// before spawning a competing one. A manual `atlas daemon start` moments earlier may
/// still be mid-startup, with its atlasd spawned but neither signal there yet; a
/// second caller that spawns anyway only loses the DuckDB file lock race and dies, so
/// this is worth a short wait first, not the full readiness deadline below.
const START_RACE_TIMEOUT: Duration = Duration::from_millis(1_500);
const START_RACE_STEP: Duration = Duration::from_millis(150);
/// How long to wait on a start that has already written `daemon.json` from a live
/// atlasd pid before giving up on it and spawning anyway.
const LIVE_START_TIMEOUT: Duration = Duration::from_secs(10);

/// Waits up to `timeout`, polling every `step`, for the daemon to announce itself
/// either by its port answering or by `daemon.json` appearing. Returns whether either
/// happened before the timeout elapsed; a stale `daemon.json` left by a crash (see
/// [`stop_daemon`]) still counts here, since the worst case is a caller waiting up to
/// `timeout` before spawning anyway, not one stuck waiting forever on it.
async fn wait_for_daemon(paths: &AtlasPaths, port: u16, timeout: Duration, step: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if is_up(port).await || daemon_info(paths).is_some() { return true; }
        if Instant::now() >= deadline { return false; }
        tokio::time::sleep(step).await;
    }
}

/// Whether `ensure_daemon_with`'s pre-spawn wait is worth paying, judged from the same
/// first probe `wait_for_daemon` itself would poll on: a genuinely cold start, with no
/// `daemon.json` anywhere, has no in-flight start to catch up to, so the wait would
/// only delay the spawn every such start was always going to need. `daemon.json`
/// existing is the signal that an in-flight start, or a crash-stale file (harmless to
/// wait a moment on; it still falls through to a fresh spawn either way), might
/// resolve, so the wait stays worth it there.
fn needs_start_race_wait(paths: &AtlasPaths) -> bool {
    daemon_info(paths).is_some()
}

/// The size past which a start moves `atlasd.log` aside instead of appending to it.
/// The same cap the desktop app puts on its own log.
const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

/// Moves `atlasd.log` to `atlasd.log.1` (replacing any older one) once it is over
/// [`LOG_ROTATE_BYTES`], so a daemon that repeats a warning on every boot or tick
/// cannot grow the file without bound. Called before each spawn, which is the only
/// point where nothing has the file open.
fn rotate_log(paths: &AtlasPaths) -> std::io::Result<()> {
    let log = paths.log_file();
    match std::fs::metadata(&log) {
        Ok(meta) if meta.len() > LOG_ROTATE_BYTES => std::fs::rename(&log, log.with_extension("log.1")),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub async fn ensure_daemon_with(paths: &AtlasPaths, port: u16, atlasd: Option<PathBuf>) -> anyhow::Result<u16> {
    if is_up(port).await { return Ok(port); }
    if needs_start_race_wait(paths) {
        wait_for_daemon(paths, port, START_RACE_TIMEOUT, START_RACE_STEP).await;
        if is_up(port).await { return Ok(port); }
    }
    // `daemon.json` lands before atlasd serves, so a live pid in it means a start is
    // in flight: wait for that port instead of spawning a competitor that would only
    // lose the DuckDB lock race. A stale file from a crash (dead or recycled pid) falls
    // through to a fresh spawn.
    if let Some(pid) = daemon_info(paths).and_then(|i| i["pid"].as_u64()) {
        if is_atlasd(pid) {
            let deadline = Instant::now() + LIVE_START_TIMEOUT;
            while Instant::now() < deadline {
                if is_up(port).await { return Ok(port); }
                tokio::time::sleep(START_RACE_STEP).await;
            }
        }
    }
    paths.ensure()?;
    rotate_log(paths)?;
    let bin = atlasd.unwrap_or_else(atlasd_path);
    let log = std::fs::OpenOptions::new().create(true).append(true).open(paths.log_file())?;
    let mut cmd = std::process::Command::new(&bin);
    cmd.arg("--port").arg(port.to_string()).arg("--home").arg(&paths.home)
        .stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
    if std::env::var("ATLAS_NO_EMBED").is_ok() { cmd.arg("--no-embed"); }
    #[cfg(unix)] { use std::os::unix::process::CommandExt; cmd.process_group(0); }
    let mut child = cmd.spawn().map_err(|e| anyhow::anyhow!("failed to start atlasd ({}): {e}", bin.display()))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        // Both signals, not just the port: `daemon.json` is written just before atlasd
        // starts serving (see its main), but a caller reading it right after this
        // returns (`daemon stop`, for one) must not race the write.
        if is_up(port).await && daemon_info(paths).is_some() { return Ok(port); }
        // An atlasd that dies on startup (port taken, DB locked, bad home) must not cost
        // the caller the full 30 s wait, so notice the dead child and report it at once.
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("atlasd exited with {status}; see {}", paths.log_file().display());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    anyhow::bail!("atlasd did not become ready on port {port}; see {}", paths.log_file().display())
}

/// True when `pid` names a live process whose command is atlasd. `daemon.json` survives a
/// crash, so the pid it records may since have been recycled by an unrelated process.
#[cfg(unix)]
fn is_atlasd(pid: u64) -> bool {
    std::process::Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("atlasd")).unwrap_or(false)
}

/// Stop the running daemon. Returns `Ok(true)` only when a signal was actually sent to a
/// verified atlasd; a `daemon.json` left behind by a crash is cleared and reported as
/// "not running" rather than used to signal whatever now owns that pid.
pub async fn stop_daemon(paths: &AtlasPaths) -> anyhow::Result<bool> {
    let Some(info) = daemon_info(paths) else { return Ok(false) };
    let stale = || { let _ = std::fs::remove_file(paths.daemon_file()); Ok(false) };
    let (Some(pid), Some(port)) = (info["pid"].as_u64(), info["port"].as_u64().and_then(|p| u16::try_from(p).ok())) else { return stale() };
    if !is_up(port).await { return stale() }
    #[cfg(unix)] {
        if !is_atlasd(pid) { return stale() }
        let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
        // `kill` only sends the signal; wait for the process to actually exit so
        // callers (and tests) don't observe atlasd still running right after this returns.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let alive = std::process::Command::new("kill").args(["-0", &pid.to_string()]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
            if !alive { break; }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    #[cfg(windows)] { let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).status(); }
    let _ = std::fs::remove_file(paths.daemon_file());
    Ok(true)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// The desktop app's sidecar depends on `ensure_daemon_with` spawning the path it is
    /// handed rather than searching for one, so point it at a fake atlasd in a temp dir and
    /// check that script is what ran. The fake exits immediately, which makes the call fail
    /// with "exited with" instead of waiting out the 30 s readiness deadline; that error is
    /// itself proof the spawn succeeded, and the marker file says which binary it spawned.
    ///
    /// No `daemon.json` is ever written here, so this also exercises the genuinely
    /// cold start `needs_start_race_wait` skips the pre-spawn wait for (L5); the
    /// timing itself is pinned separately, without real process spawns, by
    /// `needs_start_race_wait_is_false_cold_and_true_with_daemon_json` below, since a
    /// wall-clock bound on a real spawn is unreliable when the suite runs in parallel.
    #[tokio::test]
    async fn ensure_daemon_with_uses_the_explicit_path() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let fake = dir.path().join("fake-atlasd");
        std::fs::write(&fake, format!("#!/bin/sh\necho \"$0 $*\" > {}\n", marker.display())).unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        // A port nothing answers on, so `is_up` is false and a spawn is actually attempted.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let paths = AtlasPaths::at(dir.path().join("home"));
        let err = ensure_daemon_with(&paths, port, Some(fake.clone())).await.unwrap_err().to_string();

        assert!(err.contains("exited with"), "expected the fake to be spawned and die, got: {err}");
        let ran = std::fs::read_to_string(&marker).expect("the fake atlasd never ran");
        assert!(ran.contains(fake.to_str().unwrap()), "a different binary ran: {ran}");
        assert!(ran.contains(&format!("--port {port}")), "the fake did not get the port: {ran}");
    }

    /// PERF-13: the log is appended by every start, so a start that finds it over the
    /// cap moves it aside to `atlasd.log.1` (replacing any older one) and lets the new
    /// daemon begin a fresh file; one under the cap is left alone.
    #[test]
    fn rotate_log_moves_an_oversized_log_aside_and_keeps_a_small_one() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AtlasPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        let previous = paths.log_file().with_extension("log.1");

        rotate_log(&paths).unwrap();
        assert!(!paths.log_file().exists() && !previous.exists(), "nothing to rotate");

        std::fs::write(paths.log_file(), "small").unwrap();
        rotate_log(&paths).unwrap();
        assert_eq!(std::fs::read_to_string(paths.log_file()).unwrap(), "small");
        assert!(!previous.exists());

        std::fs::write(&previous, "older").unwrap();
        std::fs::write(paths.log_file(), vec![b'x'; LOG_ROTATE_BYTES as usize + 1]).unwrap();
        rotate_log(&paths).unwrap();
        assert!(!paths.log_file().exists(), "the oversized log should have moved aside");
        assert_eq!(std::fs::metadata(&previous).unwrap().len(), LOG_ROTATE_BYTES + 1, "the oversized log replaces the older one");
    }

    /// L5: `ensure_daemon_with`'s pre-spawn wait is worth paying only when
    /// `daemon.json` exists at the first probe. Checked directly against the pure
    /// predicate rather than by timing a real `ensure_daemon_with` call, since a
    /// wall-clock bound around a process spawn is unreliable when the suite runs
    /// every test in parallel (both paths are also exercised end to end, without
    /// timing, by `ensure_daemon_with_uses_the_explicit_path` above and
    /// `a_stale_daemon_json_does_not_block_a_needed_spawn` below).
    #[test]
    fn needs_start_race_wait_is_false_cold_and_true_with_daemon_json() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AtlasPaths::at(dir.path().join("home"));
        assert!(!needs_start_race_wait(&paths), "no daemon.json anywhere: a genuinely cold start, nothing to wait on");

        paths.ensure().unwrap();
        std::fs::write(paths.daemon_file(), r#"{"pid":1,"port":1,"started_at":"x"}"#).unwrap();
        assert!(needs_start_race_wait(&paths), "daemon.json present: an in-flight start might still resolve, worth the wait");
    }

    /// The pre-spawn wait notices `daemon.json` as soon as it appears, rather than
    /// sitting out its whole budget: proof it would let an already-in-flight start
    /// catch up instead of always paying the full wait before spawning a competitor.
    #[tokio::test]
    async fn wait_for_daemon_returns_as_soon_as_daemon_json_appears() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AtlasPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        let daemon_file = paths.daemon_file();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            std::fs::write(&daemon_file, r#"{"pid":1,"port":1,"started_at":"x"}"#).unwrap();
        });
        // Nothing is listening here; only `daemon.json` should trip the wait.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();

        // No wall-clock assertion: nothing listens on the port, so `found` can only be
        // true because `daemon.json` tripped the wait before its ceiling. A timing bound
        // here flaked whenever the whole workspace ran its tests in parallel.
        let found = wait_for_daemon(&paths, port, Duration::from_secs(10), Duration::from_millis(50)).await;
        assert!(found, "wait_for_daemon should have noticed daemon.json");
    }

    /// With neither signal, the wait times out rather than hanging.
    #[tokio::test]
    async fn wait_for_daemon_times_out_when_nothing_appears() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AtlasPaths::at(dir.path().join("home"));
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let found = wait_for_daemon(&paths, port, Duration::from_millis(300), Duration::from_millis(50)).await;
        assert!(!found);
    }

    /// A `daemon.json` left behind by a crash (see `stop_daemon`) must not block a
    /// spawn forever when nothing is actually listening: the pre-spawn wait notices it
    /// and moves on, and the fallback spawn still runs.
    ///
    /// Also exercises the `daemon.json`-exists half of `needs_start_race_wait` (L5)
    /// end to end: the pre-spawn wait is entered (unlike the genuinely cold start in
    /// `ensure_daemon_with_uses_the_explicit_path`) and still falls through to a
    /// fresh spawn once it finds nothing actually listening.
    #[tokio::test]
    async fn a_stale_daemon_json_does_not_block_a_needed_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let fake = dir.path().join("fake-atlasd");
        std::fs::write(&fake, format!("#!/bin/sh\necho \"$0 $*\" > {}\n", marker.display())).unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let paths = AtlasPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        std::fs::write(paths.daemon_file(), r#"{"pid":1,"port":1,"started_at":"x"}"#).unwrap();

        let err = ensure_daemon_with(&paths, port, Some(fake.clone())).await.unwrap_err().to_string();
        assert!(err.contains("exited with"), "expected the fake to still be spawned, got: {err}");
        assert!(marker.exists(), "a stale daemon.json must not stop a spawn when nothing answers the port");
    }
}
