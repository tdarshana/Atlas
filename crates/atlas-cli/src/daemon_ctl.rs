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
pub async fn ensure_daemon_with(paths: &AtlasPaths, port: u16, atlasd: Option<PathBuf>) -> anyhow::Result<u16> {
    if is_up(port).await { return Ok(port); }
    paths.ensure()?;
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
        if is_up(port).await { return Ok(port); }
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
}
