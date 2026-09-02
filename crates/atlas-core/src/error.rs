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
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AtlasError>;

impl From<anyhow::Error> for AtlasError {
    fn from(e: anyhow::Error) -> Self { AtlasError::Other(e.to_string()) }
}
impl From<serde_json::Error> for AtlasError {
    fn from(e: serde_json::Error) -> Self { AtlasError::Other(e.to_string()) }
}
