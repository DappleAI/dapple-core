mod modernbert;
mod stub;

pub use modernbert::ModernBertMlmBackend;
pub use stub::StubMlmBackend;

use crate::error::OnnxServiceError;
use async_trait::async_trait;
use std::sync::Arc;

/// A single token prediction with score
#[derive(Debug, Clone)]
pub struct TokenPrediction {
    /// The predicted token text
    pub token: String,
    /// The score (log probability or softmax probability)
    pub score: f32,
}

/// Prediction for a single mask position
#[derive(Debug, Clone)]
pub struct MaskPrediction {
    /// Position of the mask in the original text (character offset)
    pub position: usize,
    /// Top-k candidate tokens for this mask
    pub candidates: Vec<TokenPrediction>,
}

/// Result of filling masks in text
#[derive(Debug, Clone)]
pub struct MlmResult {
    /// The text with masks filled using best predictions
    pub filled_text: String,
    /// Predictions for each mask position
    pub predictions: Vec<MaskPrediction>,
}

/// Trait for masked language modeling backends
#[async_trait]
pub trait MlmBackend: Send + Sync {
    /// Fill mask tokens in the input text
    /// Returns the filled text and predictions for each mask
    async fn fill_masks(&self, text: &str, top_k: usize) -> Result<MlmResult, OnnxServiceError>;

    /// Get the mask token used by this backend (e.g., "[MASK]")
    fn mask_token(&self) -> &str;

    /// Get the backend name for logging
    fn name(&self) -> &str;

    /// Eagerly load heavy resources. Default is a no-op.
    async fn warmup(&self) -> Result<(), OnnxServiceError> {
        Ok(())
    }
}

/// Configuration for MLM service
#[derive(Debug, Clone)]
pub struct MlmConfig {
    /// HuggingFace model ID (e.g., "answerdotai/ModernBERT-base")
    pub model_id: String,
    /// Whether to fail startup if model loading fails
    pub required: bool,
    /// Optional HuggingFace token for private models
    pub hf_token: Option<String>,
}

impl Default for MlmConfig {
    fn default() -> Self {
        Self {
            model_id: "answerdotai/ModernBERT-base".to_string(),
            required: false,
            hf_token: None,
        }
    }
}

impl MlmConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Option<Self> {
        // Only create config if explicitly enabled or model ID is set
        let model_id = std::env::var("MLM_MODEL_ID").ok();
        let enabled = std::env::var("MLM_ENABLED")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        if !enabled && model_id.is_none() {
            return None;
        }

        Some(Self {
            model_id: model_id.unwrap_or_else(|| "answerdotai/ModernBERT-base".to_string()),
            required: std::env::var("MLM_REQUIRED")
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
            hf_token: std::env::var("HF_TOKEN").ok(),
        })
    }
}

/// MLM service for filling masked tokens
pub struct MlmService {
    backend: Arc<dyn MlmBackend>,
}

impl MlmService {
    /// Create a new MLM service with the given backend
    pub fn new(backend: Arc<dyn MlmBackend>) -> Self {
        Self { backend }
    }

    /// Create an MLM service with the ModernBERT backend
    pub async fn with_modernbert(config: &MlmConfig) -> Result<Self, OnnxServiceError> {
        let backend = ModernBertMlmBackend::new(config).await?;
        Ok(Self {
            backend: Arc::new(backend),
        })
    }

    /// Create an MLM service with the stub backend (for testing)
    pub fn new_stub() -> Self {
        Self {
            backend: Arc::new(StubMlmBackend::new()),
        }
    }

    /// Fill masks in the input text
    pub async fn fill_masks(
        &self,
        text: &str,
        top_k: usize,
    ) -> Result<MlmResult, OnnxServiceError> {
        if text.is_empty() {
            return Ok(MlmResult {
                filled_text: String::new(),
                predictions: Vec::new(),
            });
        }

        log::debug!(
            "Filling masks in text (len={}) using {} (top_k={})",
            text.len(),
            self.backend.name(),
            top_k
        );

        self.backend.fill_masks(text, top_k).await
    }

    /// Get the mask token used by this service
    pub fn mask_token(&self) -> &str {
        self.backend.mask_token()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_service() {
        let service = MlmService::new_stub();
        let result = service.fill_masks("Hello [MASK] world", 3).await.unwrap();
        assert!(!result.filled_text.contains("[MASK]"));
        assert_eq!(result.predictions.len(), 1);
    }

    #[tokio::test]
    async fn test_empty_input() {
        let service = MlmService::new_stub();
        let result = service.fill_masks("", 1).await.unwrap();
        assert_eq!(result.filled_text, "");
        assert!(result.predictions.is_empty());
    }

    #[tokio::test]
    async fn test_no_masks() {
        let service = MlmService::new_stub();
        let result = service.fill_masks("Hello world", 1).await.unwrap();
        assert_eq!(result.filled_text, "Hello world");
        assert!(result.predictions.is_empty());
    }
}
