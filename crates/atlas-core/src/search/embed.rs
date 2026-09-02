use std::path::Path;
use crate::{AtlasError, Result};

pub trait Embedder: Send + Sync {
    fn name(&self) -> &str;
    fn dims(&self) -> usize;
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

pub struct NoopEmbedder;
impl Embedder for NoopEmbedder {
    fn name(&self) -> &str { "none" }
    fn dims(&self) -> usize { 0 }
    fn embed(&self, _t: &[String]) -> Result<Vec<Vec<f32>>> { Err(AtlasError::Embedding("no embedding model loaded".into())) }
}

pub struct FastEmbedder { inner: std::sync::Mutex<fastembed::TextEmbedding> }
impl FastEmbedder {
    pub const MODEL_NAME: &'static str = "BAAI/bge-small-en-v1.5";
    pub fn try_new(cache_dir: &Path) -> Result<Self> {
        use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
        std::fs::create_dir_all(cache_dir)?;
        let opts = InitOptions::new(EmbeddingModel::BGESmallENV15).with_cache_dir(cache_dir.to_path_buf()).with_show_download_progress(false);
        let model = TextEmbedding::try_new(opts).map_err(|e| AtlasError::Embedding(e.to_string()))?;
        Ok(Self { inner: std::sync::Mutex::new(model) })
    }
}
impl Embedder for FastEmbedder {
    fn name(&self) -> &str { Self::MODEL_NAME }
    fn dims(&self) -> usize { 384 }
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut m = self.inner.lock().map_err(|e| AtlasError::Other(e.to_string()))?;
        m.embed(texts, None).map_err(|e| AtlasError::Embedding(e.to_string()))
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() { return 0.0; }
    let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..a.len() { dot += (a[i] * b[i]) as f64; na += (a[i] * a[i]) as f64; nb += (b[i] * b[i]) as f64; }
    if na == 0.0 || nb == 0.0 { return 0.0; }
    dot / (na.sqrt() * nb.sqrt())
}

#[cfg(test)]
mod tests {
    use super::cosine;
    #[test]
    fn cosine_basics() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-9);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }
}
