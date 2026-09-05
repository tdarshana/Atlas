// Installing a plugin from a local folder or a GitHub repository. Both paths end up
// validating the same `atlas-plugin.json` and copying the same way into
// `<plugins_dir>/<id>`; only where the files come from, and what gets recorded as the
// install's `source`, differs.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::manifest::{compatible, Manifest, ATLAS_API_VERSION};
use super::registry::{plugins_dir, record_install, PluginInfo, SourceRef};
use crate::scratch::ScratchDir;

/// A downloaded archive over this size is refused rather than extracted.
const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Validates the manifest at `src`, copies `src` into `<plugins_dir>/<id>` (replacing an
/// existing install of the same id) and records `source` in the state file. Shared by
/// [`install_from_folder`] and [`install_from_archive_url`], which differ only in where
/// `src` came from and what `source` says about it.
fn install_prepared(app_data: &Path, src: &Path, source: SourceRef) -> Result<PluginInfo, String> {
    let manifest_path = src.join("atlas-plugin.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Could not read atlas-plugin.json: {e}"))?;
    let manifest = Manifest::parse(&text)?;
    manifest.validate(src)?;

    let dest = plugins_dir(app_data).join(&manifest.id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(plugins_dir(app_data)).map_err(|e| e.to_string())?;
    copy_dir(src, &dest)?;

    let is_compatible = compatible(&manifest.api);
    // A fresh install is recorded disabled, whatever its compatibility: installing is not
    // consenting, and nothing of the plugin runs until the user ticks `Enabled`. `granted`
    // is still seeded from the manifest so the permission chips read as what the plugin
    // asked for; the Permissions view is where any of it is taken back.
    let granted = manifest.permissions.clone();
    record_install(app_data, &manifest.id, false, source, granted.clone())?;

    let reason = (!is_compatible).then(|| {
        format!("'{}' needs API {} but this app provides {ATLAS_API_VERSION}.", manifest.id, manifest.api)
    });
    Ok(PluginInfo {
        id: manifest.id.clone(),
        manifest: Some(manifest),
        enabled: false,
        compatible: is_compatible,
        reason,
        dir: dest,
        granted,
    })
}

/// Copies `src` into `dest` recursively, skipping `.git` and `node_modules` at every
/// level.
fn copy_dir(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name == ".git" || name == "node_modules" {
            continue;
        }
        let src_path = entry.path();
        let dest_path = dest.join(&name);
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_dir() {
            copy_dir(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dest_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Installs the plugin at `src`, a folder on this machine holding `atlas-plugin.json`.
pub fn install_from_folder(app_data: &Path, src: &Path) -> Result<PluginInfo, String> {
    install_prepared(app_data, src, SourceRef { kind: "folder".into(), value: src.display().to_string() })
}

/// `https://github.com/<owner>/<repo>`, with an optional `/tree/<ref>`, split into its
/// parts. `ref` defaults to `HEAD`.
fn parse_github_url(url: &str) -> Result<(String, String, String), String> {
    let bad = || format!("'{url}' is not a github.com repository URL.");
    let rest = url.strip_prefix("https://github.com/").ok_or_else(bad)?;
    let rest = rest.trim_end_matches('/');
    let (owner_repo, git_ref) = match rest.split_once("/tree/") {
        Some((owner_repo, r)) if !r.is_empty() => (owner_repo, r.to_string()),
        Some((owner_repo, _)) => (owner_repo, "HEAD".to_string()),
        None => (rest, "HEAD".to_string()),
    };
    let (owner, repo) = owner_repo.split_once('/').ok_or_else(bad)?;
    if owner.is_empty() || repo.is_empty() {
        return Err(bad());
    }
    // Each part is interpolated into the codeload path, so anything that could move the
    // fetch off that path (an extra segment, a query or fragment start, a `..`) is
    // refused rather than escaped: the URL fetched must be the URL the user typed.
    for part in [owner, repo, git_ref.as_str()] {
        if part.contains('/') || part.contains('?') || part.contains('#') || part.contains("..") {
            return Err(bad());
        }
    }
    Ok((owner.to_string(), repo.to_string(), git_ref))
}

/// Installs the plugin at a GitHub repository URL by downloading its `codeload.github.com`
/// tarball and handing off to [`install_from_archive_url`].
pub fn install_from_github(app_data: &Path, url: &str) -> Result<PluginInfo, String> {
    let (owner, repo, git_ref) = parse_github_url(url)?;
    let archive_url = format!("https://codeload.github.com/{owner}/{repo}/tar.gz/{git_ref}");
    install_from_archive_url(app_data, &archive_url, url)
}

/// Downloads the `.tar.gz` at `url`, extracts it, strips the single top-level directory
/// GitHub's archives carry, and installs what is left as `source_label` (the URL to
/// record in the state file, which for a real GitHub install is the repository page, not
/// `url` itself). Split out from [`install_from_github`] so a test can point `url` at a
/// local server instead of the real internet.
pub fn install_from_archive_url(app_data: &Path, url: &str, source_label: &str) -> Result<PluginInfo, String> {
    let bytes = download_capped(url)?;
    let temp = ScratchDir::new("download")?;
    extract_tar_gz(&bytes, temp.path())?;
    let root = single_top_level_dir(temp.path())?;
    install_prepared(app_data, &root, SourceRef { kind: "github".into(), value: source_label.to_string() })
}

/// Downloads `url` with a 30s timeout, refusing anything over [`MAX_ARCHIVE_BYTES`].
fn download_capped(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    let response = response.error_for_status().map_err(|e| e.to_string())?;

    let mut limited = response.take(MAX_ARCHIVE_BYTES + 1);
    let mut buf = Vec::new();
    limited.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(format!("The archive is larger than the {} MiB limit.", MAX_ARCHIVE_BYTES / (1024 * 1024)));
    }
    Ok(buf)
}

fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(bytes);
    tar::Archive::new(decoder).unpack(dest).map_err(|e| e.to_string())
}

/// The one directory a freshly extracted GitHub archive holds at its top level (a repo
/// tarball is always `<repo>-<ref>/...`), unwrapped so callers see the plugin's own
/// files directly.
fn single_top_level_dir(dir: &Path) -> Result<PathBuf, String> {
    let entries: Vec<PathBuf> =
        std::fs::read_dir(dir).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    match entries.as_slice() {
        [only] if only.is_dir() => Ok(only.clone()),
        _ => Err("The archive did not contain a single top-level directory.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::registry::list;
    use std::io::Write;
    use std::net::TcpListener;

    /// A scratch app-data directory that removes itself when the test's binding drops, so
    /// a run leaves nothing behind in the OS temp dir.
    fn scratch_dir(label: &str) -> ScratchDir {
        ScratchDir::new(&format!("atlas-desktop-plugins-install-test-{label}", )).unwrap()
    }

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello-plugin")
    }

    #[test]
    fn install_from_folder_copies_files_and_records_state() {
        let app_data = scratch_dir("folder");
        let info = install_from_folder(&app_data, &fixture_dir()).unwrap();
        assert_eq!(info.id, "hello-world");
        assert!(info.compatible);
        assert!(plugins_dir(&app_data).join("hello-world/atlas-plugin.json").is_file());
        assert!(plugins_dir(&app_data).join("hello-world/main.js").is_file());

        let infos = list(&app_data).unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "hello-world");
    }

    /// Installing is not consenting: a fresh install is recorded disabled even though it
    /// is compatible, and `list` reads it back the same way. The chips still show what the
    /// manifest asked for, so the user can read them before ticking `Enabled`.
    #[test]
    fn install_from_folder_lands_disabled_with_the_manifest_permissions_granted() {
        let app_data = scratch_dir("disabled");
        let info = install_from_folder(&app_data, &fixture_dir()).unwrap();
        assert!(info.compatible, "the fixture is compatible with this app");
        assert!(!info.enabled, "a fresh install must not be enabled");
        assert_eq!(
            info.granted,
            info.manifest.as_ref().unwrap().permissions,
            "the chips read as the manifest's permissions until the user revokes one"
        );

        let infos = list(&app_data).unwrap();
        assert!(!infos[0].enabled, "and the state file says so too");
        assert_eq!(infos[0].granted, info.granted);
    }

    /// Copies `fixture_dir()` into `scratch` and adds a `.git/` folder holding a file, so a
    /// test can install from a source that actually has one to skip rather than only
    /// asserting on the fixture, which never carries one. `scratch` is borrowed rather than
    /// created here so the caller keeps the guard alive for the length of the test.
    fn source_with_git_dir(scratch: &Path) -> PathBuf {
        let src = scratch.join("src");
        copy_dir(&fixture_dir(), &src).unwrap();
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join(".git/config"), "[core]").unwrap();
        src
    }

    #[test]
    fn install_from_folder_skips_a_git_directory_in_the_source() {
        let app_data = scratch_dir("skip-git");
        let src_scratch = scratch_dir("skip-git-src");
        let src = source_with_git_dir(&src_scratch);

        install_from_folder(&app_data, &src).unwrap();
        assert!(plugins_dir(&app_data).join("hello-world/main.js").is_file());
        assert!(!plugins_dir(&app_data).join("hello-world/.git").exists());
    }

    #[test]
    fn a_second_install_of_the_same_id_replaces_the_folder() {
        let app_data = scratch_dir("replace");
        install_from_folder(&app_data, &fixture_dir()).unwrap();

        // A stray file from the first install must not survive a reinstall.
        let stray = plugins_dir(&app_data).join("hello-world/stray.txt");
        std::fs::write(&stray, "leftover").unwrap();
        assert!(stray.exists());

        install_from_folder(&app_data, &fixture_dir()).unwrap();
        assert!(!stray.exists());
        assert!(plugins_dir(&app_data).join("hello-world/main.js").is_file());
        assert_eq!(list(&app_data).unwrap().len(), 1);
    }

    /// Builds a `.tar.gz` in memory holding `hello-plugin`'s files under one top-level
    /// directory, `hello-world-main/`, the way GitHub's own archives are shaped.
    fn build_fixture_archive() -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            builder
                .append_path_with_name(fixture_dir().join("atlas-plugin.json"), "hello-world-main/atlas-plugin.json")
                .unwrap();
            builder.append_path_with_name(fixture_dir().join("main.js"), "hello-world-main/main.js").unwrap();
            builder.finish().unwrap();
        }
        let mut gz_bytes = Vec::new();
        {
            let mut encoder = flate2::write::GzEncoder::new(&mut gz_bytes, flate2::Compression::default());
            encoder.write_all(&tar_bytes).unwrap();
            encoder.finish().unwrap();
        }
        gz_bytes
    }

    /// Serves `body` as the whole response to every connection on a background thread
    /// for as long as the test process lives. Returns the
    /// `http://127.0.0.1:<port>/archive.tar.gz` URL to fetch it from. Hand-rolled
    /// because the goal is exercising `download_capped` and `extract_tar_gz` against a
    /// real socket without reaching the internet.
    ///
    /// It answers every connection rather than one, and never panics on the serving
    /// thread: under load a client can open a connection the server never reads (a
    /// probe, a retry after a reset), and a one-shot server that spent its only accept on
    /// that one left the real request to time out.
    fn serve(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut discard = [0u8; 4096];
                let _ = std::io::Read::read(&mut stream, &mut discard);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        format!("http://127.0.0.1:{port}/archive.tar.gz")
    }

    #[test]
    fn archive_install_strips_the_top_level_directory_and_installs() {
        let app_data = scratch_dir("archive");
        let url = serve(build_fixture_archive());

        let info = install_from_archive_url(&app_data, &url, "https://github.com/acme/hello-world").unwrap();
        assert_eq!(info.id, "hello-world");
        assert!(!info.enabled, "a GitHub install lands disabled like any other");
        assert!(plugins_dir(&app_data).join("hello-world/atlas-plugin.json").is_file());
        assert!(plugins_dir(&app_data).join("hello-world/main.js").is_file());
        // No leftover top-level directory name inside the installed plugin's own folder.
        assert!(!plugins_dir(&app_data).join("hello-world/hello-world-main").exists());
    }

    /// Writes a raw tar header directly (bypassing `Builder::append_data`'s own path
    /// validation, which refuses a `..` or absolute path outright) so the archive can
    /// carry an entry a well-behaved builder would never produce, the way a hostile
    /// archive could.
    fn append_raw_entry<W: Write>(builder: &mut tar::Builder<W>, name: &[u8], entry_type: tar::EntryType, contents: &[u8]) {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(entry_type);
        if entry_type == tar::EntryType::Symlink {
            if let Some(gnu) = header.as_gnu_mut() {
                gnu.linkname[..contents.len()].copy_from_slice(contents);
            }
        }
        if let Some(gnu) = header.as_gnu_mut() {
            gnu.name[..name.len()].copy_from_slice(name);
        }
        header.set_cksum();
        builder.append(&header, contents).unwrap();
    }

    /// An archive shaped like `build_fixture_archive`'s, plus a `..`-traversal entry and a
    /// symlink entry whose target escapes the extraction directory, both nested under the
    /// legitimate top-level `hello-world-main/` so the single-top-level-directory
    /// invariant `single_top_level_dir` checks still holds.
    fn build_hostile_archive() -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            builder
                .append_path_with_name(fixture_dir().join("atlas-plugin.json"), "hello-world-main/atlas-plugin.json")
                .unwrap();
            builder.append_path_with_name(fixture_dir().join("main.js"), "hello-world-main/main.js").unwrap();
            append_raw_entry(
                &mut builder,
                b"hello-world-main/../../../escaped.txt",
                tar::EntryType::Regular,
                b"evil",
            );
            append_raw_entry(&mut builder, b"hello-world-main/escaped-link", tar::EntryType::Symlink, b"../../../etc");
            builder.finish().unwrap();
        }
        let mut gz_bytes = Vec::new();
        {
            let mut encoder = flate2::write::GzEncoder::new(&mut gz_bytes, flate2::Compression::default());
            encoder.write_all(&tar_bytes).unwrap();
            encoder.finish().unwrap();
        }
        gz_bytes
    }

    /// A `..`-traversal entry and an escaping symlink target inside the downloaded archive
    /// must not land anywhere outside the extraction scratch dir, and must not survive
    /// into the installed plugin folder either way (`tar`'s own `unpack` refuses a `..`
    /// path component, and `copy_dir` never follows a symlink since it checks
    /// `DirEntry::file_type()` for `is_dir()`/`is_file()`).
    #[test]
    fn archive_traversal_and_symlink_entries_do_not_escape_or_survive_install() {
        let app_data = scratch_dir("hostile-archive");
        let url = serve(build_hostile_archive());

        let info = install_from_archive_url(&app_data, &url, "https://github.com/acme/hello-world").unwrap();
        assert_eq!(info.id, "hello-world");
        assert!(plugins_dir(&app_data).join("hello-world/main.js").is_file());

        // Nothing escaped above the OS temp dir the extraction scratch dir lives under.
        assert!(!std::env::temp_dir().join("escaped.txt").exists());
        // Nothing from either hostile entry survived into the installed plugin folder.
        assert!(!plugins_dir(&app_data).join("hello-world/escaped.txt").exists());
        assert!(!plugins_dir(&app_data).join("hello-world/escaped-link").exists());
        assert!(!plugins_dir(&app_data).join("escaped.txt").exists());
    }

    #[test]
    fn parse_github_url_defaults_the_ref_to_head() {
        let (owner, repo, git_ref) = parse_github_url("https://github.com/acme/hello-world").unwrap();
        assert_eq!((owner.as_str(), repo.as_str(), git_ref.as_str()), ("acme", "hello-world", "HEAD"));
    }

    #[test]
    fn parse_github_url_reads_the_tree_ref() {
        let (owner, repo, git_ref) = parse_github_url("https://github.com/acme/hello-world/tree/v2").unwrap();
        assert_eq!((owner.as_str(), repo.as_str(), git_ref.as_str()), ("acme", "hello-world", "v2"));
    }

    #[test]
    fn parse_github_url_rejects_a_non_github_url() {
        assert!(parse_github_url("https://example.com/acme/hello-world").is_err());
    }

    /// Every part is interpolated into the codeload path, so a part that could move the
    /// fetch somewhere else is refused rather than escaped. `a/b/c` used to yield
    /// `repo = "b/c"`, and a `?` or `#` in a ref used to rewrite the query.
    #[test]
    fn parse_github_url_rejects_a_part_that_would_rewrite_the_codeload_path() {
        for bad in [
            "https://github.com/acme/hello/world",
            "https://github.com/acme/hello-world/tree/v2?x=1",
            "https://github.com/acme/hello-world/tree/v2#frag",
            "https://github.com/acme/hello-world/tree/../../etc",
            "https://github.com/acme/hello-world/tree/a/b",
        ] {
            assert!(parse_github_url(bad).is_err(), "accepted '{bad}'");
        }
    }
}
