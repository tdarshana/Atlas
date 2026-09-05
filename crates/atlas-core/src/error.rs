#[derive(Debug, thiserror::Error)]
pub enum AtlasError {
    #[error("database error: {0}")]
    Db(#[from] duckdb::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("embedding unavailable: {0}")]
    Embedding(String),
    /// A well-formed request the daemon is not configured to serve, answered with
    /// 409. `POST /ingest` uses it when extraction is off, so a caller can tell
    /// "switch it on" apart from "the request was wrong".
    #[error("{0}")]
    Conflict(String),
    /// A well-formed request carrying more than a route will accept, answered with
    /// 413. `ingest_transcript` uses it for a transcript over the character cap, so a
    /// caller can tell "too big" apart from "malformed".
    #[error("{0}")]
    TooLarge(String),
    #[error("{0}")]
    Other(String),
    /// A blocking-thread task panicked (a `tokio::task::JoinError`) while running
    /// synchronous Db, index, vector or embedder work off the async runtime.
    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, AtlasError>;

impl AtlasError {
    /// The variant's name as the daemon puts it on the wire (`{"error", "kind"}`), so a
    /// client can rebuild the variant with [`AtlasError::from_wire`] instead of guessing
    /// it from the status code, under which `Embedding`, `Internal`, `Db`, `Io` and
    /// `Other` all read as 500.
    pub fn kind(&self) -> &'static str {
        match self {
            AtlasError::Db(_) => "db",
            AtlasError::Io(_) => "io",
            AtlasError::NotFound(_) => "not_found",
            AtlasError::Invalid(_) => "invalid",
            AtlasError::Embedding(_) => "embedding",
            AtlasError::Conflict(_) => "conflict",
            AtlasError::TooLarge(_) => "too_large",
            AtlasError::Other(_) => "other",
            AtlasError::Internal(_) => "internal",
        }
    }

    /// The inverse of [`AtlasError::kind`] for a message that already went through
    /// `Display`: the variant's own prefix is stripped so the rebuilt error does not
    /// print it twice. A `db` error cannot be rebuilt (there is no `duckdb::Error` for
    /// a bare message), so it comes back as `Other` with its `database error:` prefix
    /// kept; an unknown kind is `Other` too.
    pub fn from_wire(kind: &str, message: String) -> Self {
        let strip = |prefix: &str| message.strip_prefix(prefix).map(str::to_string).unwrap_or_else(|| message.clone());
        match kind {
            "io" => AtlasError::Io(std::io::Error::other(strip("io error: "))),
            "not_found" => AtlasError::NotFound(strip("not found: ")),
            "invalid" => AtlasError::Invalid(strip("invalid input: ")),
            "embedding" => AtlasError::Embedding(strip("embedding unavailable: ")),
            "conflict" => AtlasError::Conflict(message),
            "too_large" => AtlasError::TooLarge(message),
            "internal" => AtlasError::Internal(strip("internal error: ")),
            _ => AtlasError::Other(message),
        }
    }
}

impl From<anyhow::Error> for AtlasError {
    fn from(e: anyhow::Error) -> Self { AtlasError::Other(e.to_string()) }
}
impl From<serde_json::Error> for AtlasError {
    fn from(e: serde_json::Error) -> Self { AtlasError::Other(e.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant a daemon can raise survives the HTTP hop as itself: the kind and
    /// the displayed message are what go on the wire, and the client gets back the
    /// same variant printing the same text, with no prefix doubled. `Db` is the one
    /// exception, and it keeps its text.
    #[test]
    fn every_variant_round_trips_through_kind_and_display() {
        let cases: Vec<AtlasError> = vec![
            AtlasError::Io(std::io::Error::other("disk full")),
            AtlasError::NotFound("task ATL-1".into()),
            AtlasError::Invalid("a task needs a title".into()),
            AtlasError::Embedding("model not loaded".into()),
            AtlasError::Conflict("someone else holds it".into()),
            AtlasError::TooLarge("transcript over the cap".into()),
            AtlasError::Other("plain".into()),
            AtlasError::Internal("worker panicked".into()),
        ];
        for original in cases {
            let rebuilt = AtlasError::from_wire(original.kind(), original.to_string());
            assert_eq!(rebuilt.kind(), original.kind(), "{original}");
            assert_eq!(rebuilt.to_string(), original.to_string());
        }
        let db = AtlasError::Db(duckdb::Error::InvalidQuery);
        let rebuilt = AtlasError::from_wire(db.kind(), db.to_string());
        assert_eq!(rebuilt.kind(), "other");
        assert_eq!(rebuilt.to_string(), db.to_string(), "the database prefix is kept for the reader");
        assert_eq!(AtlasError::from_wire("something-new", "x".into()).kind(), "other");
    }
}
