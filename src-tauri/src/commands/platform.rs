// How the host's own Rust code reaches the daemon. The commands themselves live in the
// sibling files (`window`, `about`, `notify`, `vault`, `update`); what is left here is
// the one place the daemon's address and token are resolved for them.

use atlas_client::daemon_ctl;
use atlas_client::remote::RemoteBackend;
use atlas_core::paths::AtlasPaths;

/// A `RemoteBackend` over the daemon `daemon.json` names, carrying its token (SEC-5),
/// resolved fresh rather than cached since both change across a restart: the same typed
/// client the CLI uses, so a host command never builds a URL or parses JSON by hand.
pub(crate) fn daemon_backend() -> Result<RemoteBackend, String> {
    let info = daemon_ctl::read_daemon_info(&AtlasPaths::discover()).ok_or_else(|| "The daemon is not running.".to_string())?;
    let token = info.token.ok_or_else(|| "The daemon wrote no token.".to_string())?;
    Ok(RemoteBackend::with_token(info.port, Some(token)))
}
