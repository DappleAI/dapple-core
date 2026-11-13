mod modernbert;
mod stub;

pub use modernbert::ModernBertBackend;
pub use stub::StubEmbeddingBackend;

use crate::error::OnnxServiceError;
use async_trait::async_trait;
use std::sync::Arc;

/// The result of one embedding call: the vectors, plus how much input the
/// backend had to throw away to produce them.
///
/// `truncated_count` exists because truncation is otherwise **invisible to the
/// caller**. A backend that clips a text at its sequence cap returns a
/// perfectly well-formed vector that simply does not represent the text's tail,
/// and the only evidence is a WARN in this pod's log — not in the response, and
/// not in the ingest that committed the resulting index. Reporting the count
/// lets a caller that cannot tolerate lossy vectors (guide ingest) fail instead
/// of silently indexing a lie, while callers that legitimately clip (RAG query
/// text) ignore it.
#[derive(Debug, Clone)]
pub struct EmbedBatch {
    /// One embedding per input text, in order.
    pub embeddings: Vec<Vec<f32>>,
    /// How many of the input texts exceeded the sequence cap and were truncated.
    pub truncated_count: usize,
}

/// Trait for embedding backends
#[async_trait]
pub trait EmbeddingBackend: Send + Sync {
    /// Generate embeddings for a batch of texts, reporting how many were
    /// truncated. One embedding per input text, in order.
    async fn embed_batch(&self, texts: &[String]) -> Result<EmbedBatch, OnnxServiceError>;

    /// Vectors only — for callers where truncation is acceptable (e.g. a
    /// deliberately clamped RAG query). Anything that PERSISTS the vectors
    /// should call [`EmbeddingBackend::embed_batch`] and check the count.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, OnnxServiceError> {
        Ok(self.embed_batch(texts).await?.embeddings)
    }

    /// Get the dimension of embeddings produced by this backend
    fn dimension(&self) -> usize;

    /// Get the backend name for logging
    fn name(&self) -> &str;

    /// Eagerly load heavy resources. Default is a no-op.
    async fn warmup(&self) -> Result<(), OnnxServiceError> {
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
    /// Hard ceiling on tokens per sequence. Texts longer than this are
    /// TRUNCATED rather than embedded whole. See
    /// [`modernbert::MAX_SEQUENCE_TOKENS`] for why this must never be
    /// unbounded.
    pub max_sequence_tokens: usize,
    /// Ceiling on `group_size * max_len^2` for one ONNX run. Bounding the
    /// sequence length alone is NOT enough — see
    /// [`modernbert::MAX_BATCH_TOKEN_AREA`].
    pub max_batch_token_area: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model_id: "lightonai/modernbert-embed-large".to_string(),
            dimension: 768,
            required: false,
            hf_token: None,
            max_sequence_tokens: modernbert::MAX_SEQUENCE_TOKENS,
            max_batch_token_area: modernbert::MAX_BATCH_TOKEN_AREA,
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
            max_sequence_tokens: std::env::var("EMBEDDING_MAX_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|n| *n > 0)
                .unwrap_or(modernbert::MAX_SEQUENCE_TOKENS),
            max_batch_token_area: std::env::var("EMBEDDING_MAX_BATCH_AREA")
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|n| *n > 0)
                .unwrap_or(modernbert::MAX_BATCH_TOKEN_AREA),
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
    pub async fn with_modernbert(config: &EmbeddingConfig) -> Result<Self, OnnxServiceError> {
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

    /// Generate embeddings for a batch of texts, reporting truncation.
    pub async fn embed_batch(&self, texts: &[String]) -> Result<EmbedBatch, OnnxServiceError> {
        if texts.is_empty() {
            return Ok(EmbedBatch {
                embeddings: Vec::new(),
                truncated_count: 0,
            });
        }

        log::debug!(
            "Generating embeddings for {} texts using {}",
            texts.len(),
            self.backend.name()
        );

        self.backend.embed_batch(texts).await
    }

    /// Generate embeddings for a batch of texts, discarding the truncation
    /// count. Prefer [`EmbeddingService::embed_batch`] when the vectors are
    /// going to be persisted.
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, OnnxServiceError> {
        Ok(self.embed_batch(texts).await?.embeddings)
    }

    /// Generate embedding for a single text
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>, OnnxServiceError> {
        let texts = vec![text.to_string()];
        let mut embeddings = self.embed(&texts).await?;
        embeddings.pop().ok_or_else(|| {
            OnnxServiceError::EmbeddingError("No embedding returned for text".to_string())
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
    pub async fn warmup(&self) -> Result<(), OnnxServiceError> {
        self.backend.warmup().await
    }
}
