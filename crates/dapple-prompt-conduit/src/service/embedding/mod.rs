mod modernbert;
mod stub;

pub use modernbert::ModernBertBackend;
pub use stub::StubEmbeddingBackend;

use crate::error::ConduitError;
use async_trait::async_trait;
use std::sync::Arc;

/// Trait for embedding backends
#[async_trait]
pub trait EmbeddingBackend: Send + Sync {
    /// Generate embeddings for a batch of texts
    /// Returns a vector of embeddings, one per input text
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ConduitError>;

    /// Get the dimension of embeddings produced by this backend
    fn dimension(&self) -> usize;

    /// Get the backend name for logging
    fn name(&self) -> &str;

    /// Eagerly load heavy resources. Default is a no-op.
    async fn warmup(&self) -> Result<(), ConduitError> {
        Ok(())
    }
}

/// Configuration for embedding service
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// HuggingFace model ID (e.g., "lightonai/modernbert-embed-large")
    pub model_id: String,
    /// Embedding dimension (Matryoshka: 256, 512, 768, 1024)
    pub dimension: usize,
    /// Whether to fail startup if model loading fails
    pub required: bool,
    /// Optional HuggingFace token for private models
    pub hf_token: Option<String>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model_id: "lightonai/modernbert-embed-large".to_string(),
            dimension: 768,
            required: false,
            hf_token: None,
        }
    }
}

impl EmbeddingConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Option<Self> {
        // Only create config if explicitly enabled or model ID is set
        let model_id = std::env::var("EMBEDDING_MODEL_ID").ok();
        let enabled = std::env::var("EMBEDDING_ENABLED")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        if !enabled && model_id.is_none() {
            return None;
        }

        Some(Self {
            model_id: model_id.unwrap_or_else(|| "lightonai/modernbert-embed-large".to_string()),
            dimension: std::env::var("EMBEDDING_DIMENSION")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(768),
            required: std::env::var("EMBEDDING_REQUIRED")
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
            hf_token: std::env::var("HF_TOKEN").ok(),
        })
    }
}

/// Embedding service for generating text embeddings
pub struct EmbeddingService {
    backend: Arc<dyn EmbeddingBackend>,
}

impl EmbeddingService {
    /// Create a new embedding service with the given backend
    pub fn new(backend: Arc<dyn EmbeddingBackend>) -> Self {
        Self { backend }
    }

    /// Create an embedding service with the ModernBERT backend
    pub async fn with_modernbert(config: &EmbeddingConfig) -> Result<Self, ConduitError> {
        let backend = ModernBertBackend::new(config).await?;
        Ok(Self {
            backend: Arc::new(backend),
        })
    }

    /// Create an embedding service with the stub backend (for testing)
    pub fn new_stub(dimension: usize) -> Self {
        Self {
            backend: Arc::new(StubEmbeddingBackend::new(dimension)),
        }
    }

    /// Generate embeddings for a batch of texts
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ConduitError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        log::debug!(
            "Generating embeddings for {} texts using {}",
            texts.len(),
            self.backend.name()
        );

        self.backend.embed(texts).await
    }

    /// Generate embedding for a single text
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>, ConduitError> {
        let texts = vec![text.to_string()];
        let mut embeddings = self.embed(&texts).await?;
        embeddings.pop().ok_or_else(|| {
            ConduitError::EmbeddingError("No embedding returned for text".to_string())
        })
    }

    /// Get the dimension of embeddings produced by this service
    pub fn dimension(&self) -> usize {
        self.backend.dimension()
    }

    /// Get the backend name
    pub fn backend_name(&self) -> &str {
        self.backend.name()
    }

    /// Eagerly load the model so errors surface at startup.
    pub async fn warmup(&self) -> Result<(), ConduitError> {
        self.backend.warmup().await
    }
}
