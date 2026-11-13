//! Image generation service for Conduit (generate-only).
//!
//! Mirrors the embedding/TTS services: a backend trait, a config loaded from the
//! environment, and a service facade. The ONLY generative operation is
//! `generate` — text → a fresh image. There is deliberately no `edit`/inpaint:
//! a model never reprocesses an existing, composed, or uploaded image (the
//! inviolability principle). Assembly of generated pieces into a Wimmelbild is
//! pure, deterministic, local image processing — see [`composite`].
//!
//! The OpenAI backend talks to `/v1/images/generations` over direct HTTP
//! (`reqwest`) rather than through `rig`, because rig 0.28's OpenAI image impl
//! builds its body from only `model`/`prompt`/`size` and drops
//! `additional_params` — so `output_format`/`background` are unreachable through
//! it. This mirrors the direct-HTTP Anthropic backend in `service/llm`.

mod composite;
mod layout;
mod openai;
mod rath;
mod silhouette;
mod stub;
mod svg;

pub use composite::{compose_with_layout, composite_blocks, BlockPlacement, CompositeOpts};
pub use layout::{strategy_for, LayoutParams, LayoutStrategy, MeasuredPiece, Placement};
pub use openai::OpenAiImageBackend;
pub use rath::RathImageBackend;
pub use silhouette::{apply_silhouette_png, apply_silhouette_svg};
pub use stub::StubImageBackend;
pub use svg::compose_svg;

use crate::error::ConduitError;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// A text→image generation request.
#[derive(Debug, Clone)]
pub struct ImageGenRequest {
    /// Which hosted engine to route to. `None`/empty and `"fal"`/`"rath"` route
    /// to the rath→Fal backend (the primary image path); `"openai"` routes to the
    /// secondary gpt-image backend. Authored in the guide's `:::inference-profile`
    /// engine_body; mirrors how the LLM path selects a backend by
    /// `ModelConfig.provider_key`.
    pub provider: Option<String>,
    /// The full image prompt (style preamble + scene description).
    pub prompt: String,
    /// Provider model id: a Fal slug (e.g. `fal-ai/flux/schnell`) for the rath
    /// backend, or `gpt-image-1` for OpenAI. Falls back to the service default.
    pub model: String,
    /// Output size as `WIDTHxHEIGHT`, e.g. `1536x1024`. The rath/Fal backend maps
    /// it to Fal's `image_size` ({width,height}); OpenAI passes it through.
    pub size: String,
    /// Optional background mode (`transparent` / `opaque`). OpenAI passes it
    /// straight through. For the rath/Fal backend, native transparency is a
    /// per-model concern authored via `provider_config` (LayerDiffuse emits RGBA
    /// directly), so this field is advisory there. Ignored by models that don't
    /// take it.
    pub background: Option<String>,
    /// Optional render quality for the gpt-image family (`low` / `medium` /
    /// `high` / `auto`). When `None`, the provider's own default is used (which
    /// is `auto`); set `high` to maximize the per-image detail budget. Ignored
    /// by backends/models that don't take it.
    pub quality: Option<String>,
    /// Optional generation seed. Pinned so a render is reproducible (one of the
    /// "responsible-to-ship" invariants). Backends that accept a seed forward it;
    /// others ignore it.
    pub seed: Option<i64>,
    /// Optional negative prompt. Authored in `provider_config` for Fal models
    /// that support one (OpenAI gpt-image has no negative-prompt field). Ignored
    /// by backends that don't take one.
    pub negative_prompt: Option<String>,
    /// Raw Fal-model `provider_config` passthrough, authored in the engine_body.
    /// The rath backend merges it verbatim into the Fal request payload (author
    /// wins over conduit-derived defaults) so per-model knobs — LayerDiffuse
    /// transparency, guidance scale, image_size overrides — need no conduit code.
    /// Ignored by the OpenAI/stub backends.
    pub provider_config: Option<Value>,
}

/// Trait for image backends (generation only).
#[async_trait]
pub trait ImageBackend: Send + Sync {
    /// Generate a fresh image from a text prompt. Returns PNG bytes.
    async fn generate(&self, request: &ImageGenRequest) -> Result<Vec<u8>, ConduitError>;

    /// True when this backend should handle the request. Routing is by
    /// `request.provider`, mirroring `LlmBackend::supports(&ModelConfig)`.
    fn supports(&self, request: &ImageGenRequest) -> bool;

    /// Backend name for logging.
    fn name(&self) -> &str;
}

/// Configuration for the image service, loaded from the environment.
#[derive(Debug, Clone)]
pub struct ImageConfig {
    /// Provider API key (OpenAI).
    pub api_key: String,
    /// Base URL for the images API (default `https://api.openai.com/v1`).
    pub base_url: String,
    /// Default model when a prompt's engine_body doesn't specify one.
    pub default_model: String,
    /// Default output size (`WIDTHxHEIGHT`).
    pub default_size: String,
}

impl ImageConfig {
    /// Load configuration from the environment.
    ///
    /// Returns `None` (image generation disabled, gracefully) when
    /// `OPENAI_API_KEY` is absent — mirrors `EmbeddingConfig::from_env`.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())?;

        Some(Self {
            api_key,
            base_url: std::env::var("OPENAI_IMAGE_BASE_URL")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            default_model: std::env::var("IMAGE_MODEL")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "gpt-image-1".to_string()),
            default_size: std::env::var("IMAGE_SIZE")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "1536x1024".to_string()),
        })
    }
}

/// Image service facade over one or more backends, carrying defaults for
/// model/size so callers can omit them.
///
/// Backends are selected per-request by `ImageBackend::supports` — the same
/// "engines like the LLMs" dispatch `LlmService` uses. The stub is the
/// unconditional fallback.
pub struct ImageService {
    backends: Vec<Arc<dyn ImageBackend>>,
    fallback: Arc<dyn ImageBackend>,
    default_model: String,
    default_size: String,
}

impl ImageService {
    /// Create a service from explicit backends + defaults.
    pub fn new(
        backends: Vec<Arc<dyn ImageBackend>>,
        default_model: impl Into<String>,
        default_size: impl Into<String>,
    ) -> Self {
        Self {
            backends,
            fallback: Arc::new(StubImageBackend::new()),
            default_model: default_model.into(),
            default_size: default_size.into(),
        }
    }

    /// Create a service backed by the OpenAI images API.
    pub fn with_openai(config: &ImageConfig) -> Self {
        let backend = OpenAiImageBackend::new(config.api_key.clone(), config.base_url.clone());
        Self::new(
            vec![Arc::new(backend)],
            config.default_model.clone(),
            config.default_size.clone(),
        )
    }

    /// Assemble the image service from the environment, registering every hosted
    /// backend whose API key is present:
    ///
    /// - rath → Fal (`FAL_AUTH_KEY`) — the primary path (LayerDiffuse/flux).
    /// - OpenAI (`OPENAI_API_KEY`) — the secondary gpt-image family backend.
    ///
    /// Returns `None` when no provider key is configured (image generation
    /// disabled, gracefully).
    pub fn from_env() -> Option<Self> {
        let mut backends: Vec<Arc<dyn ImageBackend>> = Vec::new();

        // rath → Fal is the primary/default image path (it claims the default,
        // provider-less request), so register it first — `find(supports)` returns
        // the first match and only this backend claims an omitted provider.
        if let Some(backend) = RathImageBackend::from_env() {
            log::info!("Fal (rath) image backend configured");
            backends.push(Arc::new(backend));
        }
        // OpenAI gpt-image stays as a secondary backend, used only when a prompt
        // explicitly authors `provider = "openai"`.
        if let Some(cfg) = ImageConfig::from_env() {
            log::info!("OpenAI image backend configured (base={})", cfg.base_url);
            backends.push(Arc::new(OpenAiImageBackend::new(cfg.api_key, cfg.base_url)));
        }

        if backends.is_empty() {
            return None;
        }

        // Default model/size when a prompt's engine_body omits them. Fal is the
        // primary path, so the default is a Fal slug; an OpenAI prompt authors its
        // own `gpt-image-*` model explicitly.
        let default_model = std::env::var("IMAGE_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "fal-ai/flux/schnell".to_string());
        let default_size = std::env::var("IMAGE_SIZE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "1536x1024".to_string());

        Some(Self::new(backends, default_model, default_size))
    }

    /// Create a service backed by the deterministic stub (for testing).
    pub fn new_stub() -> Self {
        Self {
            backends: Vec::new(),
            fallback: Arc::new(StubImageBackend::new()),
            default_model: "stub-image".to_string(),
            default_size: "1024x1024".to_string(),
        }
    }

    /// Generate an image, routing to the backend that `supports` the request's
    /// provider. Empty `model`/`size` fall back to the service defaults. Falls
    /// back to the stub when no registered backend claims the request.
    pub async fn generate(&self, mut request: ImageGenRequest) -> Result<Vec<u8>, ConduitError> {
        if request.prompt.trim().is_empty() {
            return Err(ConduitError::InvalidRequest(
                "Cannot generate an image from an empty prompt".to_string(),
            ));
        }
        if request.model.trim().is_empty() {
            request.model = self.default_model.clone();
        }
        if request.size.trim().is_empty() {
            request.size = self.default_size.clone();
        }

        let backend = self
            .backends
            .iter()
            .find(|b| b.supports(&request))
            .unwrap_or(&self.fallback);

        log::debug!(
            "Generating image: backend={}, provider={:?}, model={}, size={}, seed={:?}, quality={:?}",
            backend.name(),
            request.provider,
            request.model,
            request.size,
            request.seed,
            request.quality
        );
        backend.generate(&request).await
    }

    pub fn default_model(&self) -> &str {
        &self.default_model
    }

    pub fn default_size(&self) -> &str {
        &self.default_size
    }

    /// Names of the registered backends (for logging/diagnostics).
    pub fn backend_names(&self) -> Vec<&str> {
        self.backends.iter().map(|b| b.name()).collect()
    }
}
