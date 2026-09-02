use std::path::Path;
use std::process::{Command, Stdio};

/// Builds a small git-backed fixture project used by `projects::detect` and
/// `projects::profile` tests: a Next.js/React `package.json`, a README, a
/// TypeScript source file, a gitignored `node_modules` dir, one commit, and an
/// `origin` remote.
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
