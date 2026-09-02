use std::process::Command;
fn atlas() -> Command { Command::new(env!("CARGO_BIN_EXE_atlas")) }

#[test]
fn remember_and_recall_via_cli_starting_daemon() {
    let home = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let atlasd_dir = std::path::Path::new(env!("CARGO_BIN_EXE_atlas")).parent().unwrap().to_path_buf();
    let env = |c: &mut Command| { c.env("ATLAS_HOME", home.path()).env("ATLAS_PORT", port.to_string()).env("ATLAS_NO_EMBED", "1").env("PATH", format!("{}:{}", atlasd_dir.display(), std::env::var("PATH").unwrap_or_default())); };
    let mut c = atlas(); env(&mut c);
    let out = c.args(["remember", "the deploy target is fly.io", "--kind", "decision", "--tag", "infra"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut c = atlas(); env(&mut c);
    let out = c.args(["recall", "where do we deploy"]).output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("fly.io"), "{s}");
    let mut c = atlas(); env(&mut c);
    let out = c.args(["daemon", "status"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("memories_active"));
    let mut c = atlas(); env(&mut c);
    assert!(c.args(["daemon", "stop"]).status().unwrap().success());
}
