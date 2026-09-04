// Installing a plugin from a local folder or a GitHub repository. Both paths end up
// validating the same `atlas-plugin.json` and copying the same way into
// `<plugins_dir>/<id>`; only where the files come from, and what gets recorded as the
// install's `source`, differs.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::manifest::{compatible, Manifest, ATLAS_API_VERSION};
use super::registry::{plugins_dir, record_install, PluginInfo, SourceRef};

/// A downloaded archive over this size is refused rather than extracted.
const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// A directory under the OS temp dir, removed on drop. Hand-rolled rather than pulling
/// in the `tempfile` crate as a normal dependency just for this one call site: extracting
/// a downloaded archive is production code, and `tempfile` is a dev-dependency here.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(label: &str) -> Result<Self, String> {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("atlas-plugin-{label}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Validates the manifest at `src`, copies `src` into `<plugins_dir>/<id>` (replacing an
/// existing install of the same id) and records `source` in the state file. Shared by
/// [`install_from_folder`] and [`install_from_archive_url`], which differ only in where
/// `src` came from and what `source` says about it.
fn install_prepared(app_data: &Path, src: &Path, source: SourceRef) -> Result<PluginInfo, String> {
    let manifest_path = src.join("atlas-plugin.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("could not read atlas-plugin.json: {e}"))?;
    let manifest = Manifest::parse(&text)?;
    manifest.validate(src)?;

    let dest = plugins_dir(app_data).join(&manifest.id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(plugins_dir(app_data)).map_err(|e| e.to_string())?;
    copy_dir(src, &dest)?;

    let is_compatible = compatible(&manifest.api);
    record_install(app_data, &manifest.id, is_compatible, source)?;

    let reason = (!is_compatible).then(|| {
        format!("'{}' needs API {} but this app provides {ATLAS_API_VERSION}.", manifest.id, manifest.api)
    });
    Ok(PluginInfo {
        id: manifest.id.clone(),
        manifest,
        enabled: is_compatible,
        compatible: is_compatible,
        reason,
        dir: dest,
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
        return Err(format!("the archive is larger than the {} MiB limit.", MAX_ARCHIVE_BYTES / (1024 * 1024)));
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
        _ => Err("the archive did not contain a single top-level directory.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::registry::list;
    use std::io::Write;
    use std::net::TcpListener;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "atlas-desktop-plugins-install-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello-plugin")
    }

    #[test]
    fn install_from_folder_copies_files_records_state_and_lists_enabled() {
        let app_data = scratch_dir("folder");
        let info = install_from_folder(&app_data, &fixture_dir()).unwrap();
        assert_eq!(info.id, "hello-world");
        assert!(info.compatible);
        assert!(info.enabled);
        assert!(plugins_dir(&app_data).join("hello-world/atlas-plugin.json").is_file());
        assert!(plugins_dir(&app_data).join("hello-world/main.js").is_file());

        let infos = list(&app_data).unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "hello-world");
        assert!(infos[0].enabled);
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

    /// Serves `body` once as the whole response to any request on a background thread,
    /// then stops. Returns the `http://127.0.0.1:<port>/archive.tar.gz` URL to fetch it
    /// from. Hand-rolled because the goal is exercising `download_capped` and
    /// `extract_tar_gz` against a real socket without reaching the internet.
    fn serve_once(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut discard = [0u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut discard);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        });
        format!("http://127.0.0.1:{port}/archive.tar.gz")
    }

    #[test]
    fn archive_install_strips_the_top_level_directory_and_installs() {
        let app_data = scratch_dir("archive");
        let url = serve_once(build_fixture_archive());

        let info = install_from_archive_url(&app_data, &url, "https://github.com/acme/hello-world").unwrap();
        assert_eq!(info.id, "hello-world");
        assert!(info.enabled);
        assert!(plugins_dir(&app_data).join("hello-world/atlas-plugin.json").is_file());
        assert!(plugins_dir(&app_data).join("hello-world/main.js").is_file());
        // No leftover top-level directory name inside the installed plugin's own folder.
        assert!(!plugins_dir(&app_data).join("hello-world/hello-world-main").exists());
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
}
