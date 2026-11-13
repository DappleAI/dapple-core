mod anthropic;
#[cfg(feature = "candle")]
mod mistral;
mod mistralrs_backend;
mod stub;

pub use anthropic::AnthropicBackend;
#[cfg(feature = "candle")]
pub use mistral::{MistralBackend, MistralConfig};
pub use mistralrs_backend::{MistralRsBackend, MistralRsConfig};
pub use stub::StubBackend;

use crate::error::ConduitError;
use crate::service::model::ModelConfig;
use crate::service::template::RenderedPrompt;
use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// A single item yielded by an LLM backend stream.
///
/// Real streaming backends emit a sequence of [`StreamChunk::Token`] text deltas
/// followed by exactly one [`StreamChunk::Final`] carrying the usage metadata once
/// the provider reports it at stream-end. The usage is delivered AT THE END of the
/// stream — it is not known up front. Backends that cannot report real counts still
/// emit a trailing `Final` with `UsageMetadata::default()` so consumers have a single,
/// uniform contract.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// A text delta from the assistant.
    Token(String),
    /// The final usage metadata, yielded once at stream-end.
    Final(UsageMetadata),
}

/// Type alias for an LLM backend stream.
///
/// Yields [`StreamChunk::Token`] text deltas, then a single trailing
/// [`StreamChunk::Final`] with usage metadata.
pub type TokenStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, ConduitError>> + Send>>;

/// Token usage metadata delivered at stream-end via [`StreamChunk::Final`].
///
/// Backends that report real provider counts (e.g. Anthropic via rig-native
/// streaming) populate every field. Truly streaming backends without provider
/// counts may leave fields as `None`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageMetadata {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    /// Provider-reported input tokens (may differ from our count due to caching, etc.)
    pub provider_input_tokens: Option<u32>,
    /// Provider-reported output tokens
    pub provider_output_tokens: Option<u32>,
    /// The model ID string as reported by the provider
    pub model_id: Option<String>,
}

/// Trait for LLM backends
#[async_trait]
pub trait LlmBackend: Send + Sync {
    /// Execute a rendered prompt and return a stream of [`StreamChunk`]s.
    ///
    /// The stream yields token deltas as [`StreamChunk::Token`], then a single
    /// trailing [`StreamChunk::Final`] carrying usage metadata once it is known
    /// at stream-end. The prompt handler uses the real counts when present,
    /// falling back to estimation otherwise.
    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        model: &ModelConfig,
    ) -> Result<TokenStream, ConduitError>;

    /// Check if this backend can handle the given model config
    fn supports(&self, model: &ModelConfig) -> bool;

    /// Eagerly initialize heavy resources (model weights, etc.).
    /// Default is a no-op. Backends with expensive lazy init should override.
    async fn warmup(&self) -> Result<(), ConduitError> {
        Ok(())
    }
}

/// Configuration for LLM service backends
#[derive(Debug, Clone, Default)]
pub struct LlmConfig {
    pub anthropic_api_key: Option<String>,
    pub anthropic_default_model: Option<String>,
    pub openai_api_key: Option<String>,
    pub hf_token: Option<String>,
    /// mistral.rs GGUF model repo (e.g. "bartowski/Mistral-7B-Instruct-v0.3-GGUF")
    /// When set, registers MistralRsBackend instead of legacy Candle backend
    pub mistralrs_model_id: Option<String>,
}

impl LlmConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            anthropic_default_model: std::env::var("ANTHROPIC_DEFAULT_MODEL").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            hf_token: std::env::var("HF_TOKEN").ok(),
            mistralrs_model_id: std::env::var("MISTRALRS_MODEL_ID").ok(),
        }
    }

    /// Check if any real backend is configured
    pub fn has_any_backend(&self) -> bool {
        self.anthropic_api_key.is_some()
            || self.openai_api_key.is_some()
            || self.hf_token.is_some()
            || self.mistralrs_model_id.is_some()
    }
}

/// LLM service for executing prompts
/// Routes requests to appropriate backends based on engine configuration
pub struct LlmService {
    backends: Vec<Box<dyn LlmBackend>>,
    fallback: Box<dyn LlmBackend>,
}

impl LlmService {
    /// Create a new LLM service with configured backends
    pub fn new(config: &LlmConfig) -> Self {
        let mut backends: Vec<Box<dyn LlmBackend>> = Vec::new();

        // Add Anthropic backend if API key is available
        if let Some(api_key) = &config.anthropic_api_key {
            log::info!("Anthropic backend configured");
            backends.push(Box::new(AnthropicBackend::new(api_key.clone())));
        }

        // Add local LLM backend: prefer mistral.rs (GGUF) over legacy Candle
        if config.mistralrs_model_id.is_some() {
            log::info!("mistral.rs backend configured (lazy-loaded)");
            backends.push(Box::new(MistralRsBackend::from_env()));
        }
        #[cfg(feature = "candle")]
        if config.mistralrs_model_id.is_none() && config.hf_token.is_some() {
            log::info!("Mistral Candle backend configured (lazy-loaded)");
            backends.push(Box::new(MistralBackend::from_env()));
        }

        // Stub is always the fallback
        let fallback = Box::new(StubBackend::new());

        Self { backends, fallback }
    }

    /// Create a stub-only LLM service for testing
    pub fn new_stub() -> Self {
        Self {
            backends: Vec::new(),
            fallback: Box::new(StubBackend::new()),
        }
    }

    /// Execute a rendered prompt and return a stream of [`StreamChunk`]s
    pub async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        model: &ModelConfig,
    ) -> Result<TokenStream, ConduitError> {
        // Find a backend that supports this model
        for backend in &self.backends {
            if backend.supports(model) {
                log::debug!(
                    "Using backend for model {} / {} @ {}",
                    model.lineage_key,
                    model.tier,
                    model.provider_key
                );
                return backend.execute_streaming(prompt, model).await;
            }
        }

        // Fall back to stub (useful for testing or when lineage_key = "stub")
        if model.lineage_key != "stub" {
            log::warn!(
                "No backend found for model {} / {} @ {}, using stub",
                model.lineage_key,
                model.tier,
                model.provider_key
            );
        }
        self.fallback.execute_streaming(prompt, model).await
    }

    /// Eagerly initialize all backends that have heavy startup costs.
    pub async fn warmup(&self) {
        for backend in &self.backends {
            if let Err(e) = backend.warmup().await {
                log::error!("Backend warmup failed: {}", e);
            }
        }
    }
}
