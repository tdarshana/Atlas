use atlas_core::{backend::Backend, models::*, AtlasError, Result};
use uuid::Uuid;

#[derive(Clone)]
pub struct RemoteBackend { base: String, client: reqwest::Client }

impl RemoteBackend {
    pub fn new(port: u16) -> Self { Self { base: format!("http://127.0.0.1:{port}/api/v1"), client: reqwest::Client::new() } }
    async fn handle<T: serde::de::DeserializeOwned>(r: reqwest::Response) -> Result<T> {
        let status = r.status();
        if status.is_success() { return r.json::<T>().await.map_err(|e| AtlasError::Other(e.to_string())); }
        let msg = r.json::<serde_json::Value>().await.ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| status.to_string());
        Err(match status.as_u16() { 404 => AtlasError::NotFound(msg), 400 => AtlasError::Invalid(msg), _ => AtlasError::Other(msg) })
    }
    fn net(e: reqwest::Error) -> AtlasError { AtlasError::Other(format!("daemon unreachable: {e}")) }
}

#[async_trait::async_trait]
impl Backend for RemoteBackend {
    async fn status(&self) -> Result<StatusReport> { Self::handle(self.client.get(format!("{}/status", self.base)).send().await.map_err(Self::net)?).await }
    async fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory> { Self::handle(self.client.post(format!("{}/memories?actor={actor}", self.base)).json(&m).send().await.map_err(Self::net)?).await }
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>> { Self::handle(self.client.post(format!("{}/memories/search", self.base)).json(&q).send().await.map_err(Self::net)?).await }
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> { Self::handle(self.client.post(format!("{}/memories/{id}/forget?actor={actor}", self.base)).json(&serde_json::json!({"reason": reason})).send().await.map_err(Self::net)?).await }
    async fn get_memory(&self, id: Uuid) -> Result<Memory> { Self::handle(self.client.get(format!("{}/memories/{id}", self.base)).send().await.map_err(Self::net)?).await }
}
