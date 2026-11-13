use super::EmbeddingBackend;
use crate::error::ConduitError;
use async_trait::async_trait;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Stub embedding backend for testing
/// Produces deterministic embeddings based on text hash
pub struct StubEmbeddingBackend {
    dimension: usize,
}

impl StubEmbeddingBackend {
    pub fn new(dimension: usize) -> Self {
        Self { dimension }
    }

    /// Generate a deterministic embedding from text using hash
    fn hash_to_embedding(&self, text: &str) -> Vec<f32> {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();

        // Generate pseudo-random but deterministic values from hash
        let mut embedding = Vec::with_capacity(self.dimension);
        let mut seed = hash;

        for _ in 0..self.dimension {
            // Simple LCG-style PRNG for deterministic values
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            // Convert to float in range [-1, 1]
            let value = ((seed >> 33) as f32 / (u32::MAX as f32)) * 2.0 - 1.0;
            embedding.push(value);
        }

        // L2 normalize the embedding
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut embedding {
                *v /= norm;
            }
        }

        embedding
    }
}

#[async_trait]
impl EmbeddingBackend for StubEmbeddingBackend {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ConduitError> {
        log::debug!("Stub backend generating {} embeddings", texts.len());
        Ok(texts.iter().map(|t| self.hash_to_embedding(t)).collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn name(&self) -> &str {
        "stub"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_deterministic() {
        let backend = StubEmbeddingBackend::new(768);

        let texts = vec!["hello world".to_string()];
        let emb1 = backend.embed(&texts).await.unwrap();
        let emb2 = backend.embed(&texts).await.unwrap();

        // Same text should produce same embedding
        assert_eq!(emb1, emb2);
    }

    #[tokio::test]
    async fn test_stub_normalized() {
        let backend = StubEmbeddingBackend::new(768);

        let texts = vec!["test text".to_string()];
        let embeddings = backend.embed(&texts).await.unwrap();

        // Embedding should be L2 normalized (norm ≈ 1)
        let norm: f32 = embeddings[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.0001);
    }

    #[tokio::test]
    async fn test_stub_different_texts() {
        let backend = StubEmbeddingBackend::new(768);

        let emb1 = backend.embed(&["hello".to_string()]).await.unwrap();
        let emb2 = backend.embed(&["world".to_string()]).await.unwrap();

        // Different texts should produce different embeddings
        assert_ne!(emb1[0], emb2[0]);
    }
}
