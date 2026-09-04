// The `atlas-plugin` URI scheme, which serves a plugin's own files to the sandboxed
// iframe that runs it.
//
// A plugin document cannot be a `srcdoc` or a `blob:`: both inherit the app's CSP
// (`default-src 'self'`), which blocks the plugin's own script. Serving the document from
// its own scheme gives it its own origin and lets it carry a policy of its own instead:
// see `FRAME_CSP`, which pins the frame to files from that same scheme.
//
// URL shape: `atlas-plugin://localhost/<id>/<path>` on macOS and Linux, which Tauri maps
// to `http://atlas-plugin.localhost/<id>/<path>` on Windows. Both are parsed as a `Url`,
// so the path segments read the same on either.
//
// Two paths are generated rather than read off disk: `<id>/__frame` is the host document
// the iframe loads, and `<id>/__bridge.js` is the client script it pulls in first.

use std::path::{Path, PathBuf};

use tauri::{http, Manager, Runtime, UriSchemeContext, Url};

use super::registry::{self, PluginInfo};

/// The scheme name registered on the Tauri builder.
pub const SCHEME: &str = "atlas-plugin";

/// The client half of the bridge, served at `<id>/__bridge.js`. The same text lives in
/// `src/lib/plugins/bridge-client.ts` as `BRIDGE_CLIENT_JS`; `bridge-client.test.ts`
/// asserts the two are byte-identical.
pub const BRIDGE_CLIENT_JS: &str = include_str!("bridge_client.js");

/// Escapes a value going into a double-quoted HTML attribute. `Manifest::validate` already
/// refuses an absolute or `..`-bearing `main`, but a quote or an angle bracket is a legal
/// filename character on macOS and Linux and survives a repository tarball.
fn escape_attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The frame document's own content security policy. The frame may load scripts, styles,
/// images and fonts from the `atlas-plugin` scheme (its own files, under both the macOS
/// and Linux `atlas-plugin:` form and the Windows `http://atlas-plugin.localhost` one) and
/// nothing else: `connect-src 'none'` stops a plugin calling out to the network,
/// `frame-src 'none'` and `base-uri 'none'` stop it embedding or rebasing onto a remote
/// page, and `form-action 'none'` stops it navigating itself away by submitting a form.
///
/// This does not restrict `postMessage`, which CSP does not govern, so everything a
/// plugin is meant to do still works: the bridge is the only way out either way.
const FRAME_CSP: &str = "default-src 'none'; script-src atlas-plugin: http://atlas-plugin.localhost; style-src 'unsafe-inline' atlas-plugin: http://atlas-plugin.localhost; img-src atlas-plugin: http://atlas-plugin.localhost data:; font-src atlas-plugin: http://atlas-plugin.localhost data:; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'";

/// The generated frame document, `<id>/__frame`. `main` is the manifest's entry point,
/// resolved relative to the frame's own URL, so both scripts land back on this scheme.
///
/// The canvas is painted rather than left transparent: WKWebView draws an opaque white
/// behind an iframe whatever the document says, so a light-on-dark plugin was unreadable
/// under a dark app. `--bg-base`, `--text-primary` and `--color-scheme` arrive with the
/// theme the host sends, and each falls back to something harmless for the moment before
/// that lands.
fn frame_html(main: &str) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"{}\"><style>html{{margin:0;background:var(--bg-base, transparent);color:var(--text-primary, inherit);color-scheme:var(--color-scheme, normal);font-family:var(--font-ui, inherit)}}body{{margin:0}}</style><div id=\"root\"></div><script type=\"module\" src=\"./__bridge.js\"></script><script type=\"module\" src=\"./{}\"></script>",
        FRAME_CSP,
        escape_attribute(main)
    )
}

/// One answer from the protocol: a status, a content type and a body.
#[derive(Debug)]
pub struct Served {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Served {
    fn error(status: u16, message: String) -> Self {
        Served { status, content_type: "text/plain", body: message.into_bytes() }
    }
}

/// The content type for a served file, by extension. Anything not on the list is handed
/// over as an opaque download rather than guessed at.
pub fn content_type_for(path: &str) -> &'static str {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "js" => "text/javascript",
        "html" => "text/html",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// A path segment a plugin may name: no traversal, no encoding tricks, no separators.
/// The allow list is deliberately narrow, so `%2e%2e` and a backslash are both out by
/// virtue of the characters they contain rather than by a special case each.
fn safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Splits `atlas-plugin://localhost/<id>/<path>` (or the Windows
/// `http://atlas-plugin.localhost/<id>/<path>`) into the plugin id and the rest of the
/// path. Parsed as a URL rather than split on the host, since the two forms differ only
/// in where the scheme name sits.
pub fn split_target(uri: &str) -> Option<(String, String)> {
    let url = Url::parse(uri).ok()?;
    let mut segments = url.path_segments()?;
    let id = segments.next()?.to_string();
    let rest: Vec<&str> = segments.collect();
    if id.is_empty() || rest.is_empty() {
        return None;
    }
    Some((id, rest.join("/")))
}

/// The plugin `id`, if it is installed, compatible and enabled. Anything else is a status
/// and a sentence: an unknown plugin is a 404, a disabled or incompatible one a 403, since
/// the file may well exist but must not be served.
fn servable_plugin(app_data: &Path, id: &str) -> Result<PluginInfo, (u16, String)> {
    if !safe_segment(id) {
        return Err((403, format!("'{id}' is not a plugin id.")));
    }
    let info = registry::list(app_data)
        .map_err(|e| (500, e))?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| (404, format!("No plugin '{id}' is installed.")))?;
    if !info.compatible {
        return Err((403, info.reason.clone().unwrap_or_else(|| format!("'{id}' is not compatible."))));
    }
    if !info.enabled {
        return Err((403, format!("'{id}' is disabled.")));
    }
    Ok(info)
}

/// The file `rel` inside plugin `id`'s folder, checked against the path rules and against
/// a symlink that points back out of the folder. Shared by the protocol and by
/// [`super::plugin_read_file`], so both refuse exactly the same paths.
pub fn resolve_file(app_data: &Path, id: &str, rel: &str) -> Result<PathBuf, (u16, String)> {
    let info = servable_plugin(app_data, id)?;
    resolve_within(&info.dir, rel)
}

fn resolve_within(dir: &Path, rel: &str) -> Result<PathBuf, (u16, String)> {
    if rel.is_empty() || !rel.split('/').all(safe_segment) {
        return Err((403, format!("'{rel}' is not a path this plugin may serve.")));
    }
    let target = dir.join(rel);
    // `canonicalize` resolves every symlink on the way, so a link inside the folder that
    // points outside it lands outside `root` here and is refused.
    //
    // The frame reading these answers is untrusted, so what it gets back is the same fixed
    // sentence either way; the io error, which carries an absolute path off the user's
    // machine, goes to the log instead.
    let root = dir.canonicalize().map_err(|e| {
        log::warn!("could not resolve the plugin folder {}: {e}", dir.display());
        (404, format!("'{rel}' was not found."))
    })?;
    let resolved = target.canonicalize().map_err(|_| (404, format!("'{rel}' was not found.")))?;
    if !resolved.starts_with(&root) {
        return Err((403, format!("'{rel}' leaves the plugin's folder.")));
    }
    Ok(resolved)
}

/// Answers one request against the plugins installed under `app_data`.
pub fn serve(app_data: &Path, uri: &str) -> Served {
    let Some((id, rest)) = split_target(uri) else {
        return Served::error(404, format!("'{uri}' names no plugin file."));
    };

    let info = match servable_plugin(app_data, &id) {
        Ok(info) => info,
        Err((status, message)) => return Served::error(status, message),
    };

    if rest == "__bridge.js" {
        return Served { status: 200, content_type: "text/javascript", body: BRIDGE_CLIENT_JS.as_bytes().to_vec() };
    }

    if rest == "__frame" {
        // `compatible` is only ever true once the manifest parsed, so this is `Some` here.
        let Some(manifest) = info.manifest else {
            return Served::error(403, format!("'{id}' has no manifest."));
        };
        return Served { status: 200, content_type: "text/html", body: frame_html(&manifest.main).into_bytes() };
    }

    match resolve_within(&info.dir, &rest) {
        Ok(path) => match std::fs::read(&path) {
            Ok(body) => Served { status: 200, content_type: content_type_for(&rest), body },
            Err(e) => {
                log::warn!("could not read {}: {e}", path.display());
                Served::error(404, format!("'{rest}' could not be read."))
            }
        },
        Err((status, message)) => Served::error(status, message),
    }
}

/// The Tauri handler. Every answer carries `no-store` (a plugin's files change under the
/// user's feet on reinstall) and `Access-Control-Allow-Origin: *`, without which the
/// sandboxed frame's opaque origin could not fetch its own module scripts.
pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: http::Request<Vec<u8>>,
) -> http::Response<Vec<u8>> {
    let served = match ctx.app_handle().path().app_data_dir() {
        Ok(app_data) => serve(&app_data, &request.uri().to_string()),
        Err(e) => Served::error(500, e.to_string()),
    };
    http::Response::builder()
        .status(served.status)
        .header(http::header::CONTENT_TYPE, served.content_type)
        .header(http::header::CACHE_CONTROL, "no-store")
        .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(served.body)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::registry::{plugins_dir, record_install, SourceRef};

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "atlas-desktop-plugins-protocol-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An installed, enabled, compatible plugin with `main.js` and a `style.css`.
    fn installed(app_data: &Path, id: &str, enabled: bool) {
        let plugin_dir = plugins_dir(app_data).join(id);
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("atlas-plugin.json"),
            format!(
                r#"{{"id":"{id}","name":"Test","version":"1.0.0","description":"d","author":"a","api":">=1.0 <2","main":"main.js","permissions":[],"contributes":{{}}}}"#
            ),
        )
        .unwrap();
        std::fs::write(plugin_dir.join("main.js"), "// entry").unwrap();
        std::fs::write(plugin_dir.join("style.css"), ":root{}").unwrap();
        record_install(app_data, id, enabled, SourceRef { kind: "folder".into(), value: "x".into() }).unwrap();
    }

    #[test]
    fn splits_both_platform_url_shapes_the_same_way() {
        assert_eq!(
            split_target("atlas-plugin://localhost/hello-world/main.js"),
            Some(("hello-world".into(), "main.js".into()))
        );
        assert_eq!(
            split_target("http://atlas-plugin.localhost/hello-world/main.js"),
            Some(("hello-world".into(), "main.js".into()))
        );
        assert_eq!(
            split_target("atlas-plugin://localhost/hello-world/assets/logo.svg"),
            Some(("hello-world".into(), "assets/logo.svg".into()))
        );
        // An id on its own names no file.
        assert_eq!(split_target("atlas-plugin://localhost/hello-world"), None);
    }

    #[test]
    fn content_types_come_from_the_extension() {
        assert_eq!(content_type_for("main.js"), "text/javascript");
        assert_eq!(content_type_for("index.html"), "text/html");
        assert_eq!(content_type_for("theme.css"), "text/css");
        assert_eq!(content_type_for("data.json"), "application/json");
        assert_eq!(content_type_for("logo.svg"), "image/svg+xml");
        assert_eq!(content_type_for("shot.PNG"), "image/png");
        assert_eq!(content_type_for("body.woff2"), "font/woff2");
        assert_eq!(content_type_for("notes.txt"), "application/octet-stream");
        assert_eq!(content_type_for("noextension"), "application/octet-stream");
    }

    #[test]
    fn serves_a_file_from_an_enabled_plugin() {
        let app_data = scratch_dir("serve-file");
        installed(&app_data, "hello-world", true);

        let served = serve(&app_data, "atlas-plugin://localhost/hello-world/main.js");
        assert_eq!(served.status, 200);
        assert_eq!(served.content_type, "text/javascript");
        assert_eq!(String::from_utf8(served.body).unwrap(), "// entry");

        let css = serve(&app_data, "atlas-plugin://localhost/hello-world/style.css");
        assert_eq!(css.status, 200);
        assert_eq!(css.content_type, "text/css");
    }

    #[test]
    fn the_frame_document_names_the_bridge_and_the_manifest_main() {
        let app_data = scratch_dir("frame");
        installed(&app_data, "hello-world", true);

        let served = serve(&app_data, "atlas-plugin://localhost/hello-world/__frame");
        assert_eq!(served.status, 200);
        assert_eq!(served.content_type, "text/html");
        let html = String::from_utf8(served.body).unwrap();
        assert!(html.starts_with("<!doctype html>"), "{html}");
        assert!(html.contains(r#"<script type="module" src="./__bridge.js"></script>"#), "{html}");
        assert!(html.contains(r#"<script type="module" src="./main.js"></script>"#), "{html}");
        assert!(html.contains(r#"<div id="root"></div>"#), "{html}");
    }

    #[test]
    fn the_frame_document_paints_its_own_canvas_from_the_theme() {
        // WKWebView paints an opaque white behind an iframe, so the document has to set a
        // background of its own or a themed foreground colour is unreadable on it.
        let html = frame_html("main.js");
        assert!(html.contains("background:var(--bg-base, transparent)"), "{html}");
        assert!(html.contains("color:var(--text-primary, inherit)"), "{html}");
        assert!(html.contains("color-scheme:var(--color-scheme, normal)"), "{html}");
        assert!(html.contains("body{margin:0}"), "{html}");
    }

    #[test]
    fn the_frame_document_carries_its_own_csp() {
        let html = frame_html("main.js");
        assert!(
            html.contains(&format!(
                r#"<meta http-equiv="Content-Security-Policy" content="{FRAME_CSP}">"#
            )),
            "{html}"
        );
        // The two the policy exists for: no network calls out, no navigating to a remote
        // page. Both platform spellings of the plugin's own origin stay loadable.
        assert!(FRAME_CSP.contains("connect-src 'none'"), "{FRAME_CSP}");
        assert!(FRAME_CSP.contains("base-uri 'none'"), "{FRAME_CSP}");
        assert!(FRAME_CSP.contains("script-src atlas-plugin: http://atlas-plugin.localhost"), "{FRAME_CSP}");
        // The meta tag comes before either script, or it would not govern them.
        let csp_at = html.find("Content-Security-Policy").unwrap();
        assert!(csp_at < html.find("<script").unwrap(), "{html}");
    }

    #[test]
    fn the_frame_document_escapes_the_manifest_main() {
        let html = frame_html(r#"a"></script><script>alert(1)</script>x.js"#);
        assert!(!html.contains("alert(1)</script>"), "{html}");
        assert!(html.contains("&quot;&gt;&lt;/script&gt;"), "{html}");
    }

    #[test]
    fn the_bridge_path_serves_the_embedded_client() {
        let app_data = scratch_dir("bridge");
        installed(&app_data, "hello-world", true);

        let served = serve(&app_data, "atlas-plugin://localhost/hello-world/__bridge.js");
        assert_eq!(served.status, 200);
        assert_eq!(served.content_type, "text/javascript");
        assert_eq!(String::from_utf8(served.body).unwrap(), BRIDGE_CLIENT_JS);
    }

    #[test]
    fn a_disabled_plugin_is_refused() {
        let app_data = scratch_dir("disabled");
        installed(&app_data, "hello-world", false);

        let served = serve(&app_data, "atlas-plugin://localhost/hello-world/main.js");
        assert_eq!(served.status, 403);
        assert_eq!(String::from_utf8(served.body).unwrap(), "'hello-world' is disabled.");
        assert_eq!(serve(&app_data, "atlas-plugin://localhost/hello-world/__frame").status, 403);
    }

    #[test]
    fn an_unknown_plugin_is_a_404() {
        let app_data = scratch_dir("unknown");
        let served = serve(&app_data, "atlas-plugin://localhost/nope/main.js");
        assert_eq!(served.status, 404);
    }

    #[test]
    fn traversal_and_absolute_paths_are_refused() {
        let app_data = scratch_dir("traversal");
        installed(&app_data, "hello-world", true);
        std::fs::write(app_data.join("secret.txt"), "secret").unwrap();

        for uri in [
            "atlas-plugin://localhost/hello-world/../secret.txt",
            "atlas-plugin://localhost/hello-world/..%2Fsecret.txt",
            "atlas-plugin://localhost/hello-world/%2e%2e/secret.txt",
            "atlas-plugin://localhost/hello-world//etc/passwd",
            "atlas-plugin://localhost/hello-world/sub/../../secret.txt",
        ] {
            let served = serve(&app_data, uri);
            assert!(served.status >= 400, "{uri} was served with {}", served.status);
            assert_ne!(String::from_utf8(served.body).unwrap(), "secret", "{uri} leaked the file");
        }
    }

    #[test]
    fn a_symlink_out_of_the_plugin_folder_is_refused() {
        let app_data = scratch_dir("symlink");
        installed(&app_data, "hello-world", true);
        let outside = app_data.join("outside.js");
        std::fs::write(&outside, "// outside").unwrap();
        let link = plugins_dir(&app_data).join("hello-world").join("escape.js");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, &link).unwrap();

        let served = serve(&app_data, "atlas-plugin://localhost/hello-world/escape.js");
        assert_eq!(served.status, 403, "{}", String::from_utf8_lossy(&served.body));
    }

    #[test]
    fn a_missing_file_inside_an_enabled_plugin_is_a_404() {
        let app_data = scratch_dir("missing");
        installed(&app_data, "hello-world", true);
        assert_eq!(serve(&app_data, "atlas-plugin://localhost/hello-world/nothing.js").status, 404);
    }
}
