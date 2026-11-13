//! Video generation service for Conduit (generate-only).
//!
//! The video analog of the image service: a backend trait, an env-loaded config,
//! and a service facade that routes per-request to the backend whose `supports`
//! claims it — the same "engines like the LLMs" dispatch `LlmService`/
//! `ImageService` use. The only generative operation is `generate` — text → a
//! fresh video clip. There is no edit path: a model never reprocesses existing
//! pixels (the inviolability principle). Deterministic A/V assembly of generated
//! clips into a timeline lives in the `conduit-oximedia` sub-service (OTIO).
//!
//! ## Opt-in / off by default
//!
//! Video generation is disabled unless a provider key is configured. `from_env`
//! returns `None` when no backend key is present, so `AppState.video_service`
//! stays `None` and video prompts fail cleanly with `video_unavailable`. That
//! env gate (`FAL_AUTH_KEY` for Fal, `OPENAI_API_KEY` for Sora) is the "flag" —
//! the same opt-in shape every other optional backend uses (image via
//! `FAL_AUTH_KEY`/`OPENAI_API_KEY`, TTS, embedding). Both are lightweight
//! `reqwest`-based paths, so — unlike the native-dep `oximedia` sub-service —
//! they need no cargo feature to keep heavy deps out of the build.

mod openai;
mod rath;
mod stub;

pub use openai::OpenAiVideoBackend;
pub use rath::RathVideoBackend;
pub use stub::StubVideoBackend;

use crate::error::ConduitError;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// A text→video generation request.
#[derive(Debug, Clone)]
pub struct VideoGenRequest {
    /// Which hosted engine to route to. `None`/empty, `"fal"`, `"ltx"`, and
    /// `"rath"` all route to the rath→Fal backend (LTX-Video on Fal's queue).
    /// Authored in the guide's `:::inference-profile` engine_body; mirrors the
    /// LLM/image `provider` selector.
    pub provider: Option<String>,
    /// The full video prompt.
    pub prompt: String,
    /// Provider model id: a Fal slug (e.g. `fal-ai/ltx-video`). Falls back to the
    /// service default.
    pub model: String,
    /// Clip length in seconds. Falls back to the service default when `None`.
    pub duration: Option<u32>,
    /// Output resolution `WIDTHxHEIGHT`, e.g. `1920x1080`. Falls back to the
    /// service default when `None`.
    pub resolution: Option<String>,
    /// Frames per second (LTX default 24).
    pub fps: Option<u32>,
    /// Whether to generate the synced audio track (LTX default true).
    pub generate_audio: Option<bool>,
    /// Optional generation seed — pinned for reproducibility.
    pub seed: Option<i64>,
    /// Optional negative prompt.
    pub negative_prompt: Option<String>,
    /// Optional camera motion hint (LTX: `dolly_in`, `jib_up`, `static`, …).
    pub camera_motion: Option<String>,
    /// Raw Fal-model `provider_config` passthrough, authored in the engine_body.
    /// The rath backend merges it verbatim into the Fal request payload (author
    /// wins) so per-model knobs — resolution, num_frames, aspect_ratio — need no
    /// conduit code. Ignored by the stub backend.
    pub provider_config: Option<Value>,
}

/// The output of a video generation.
#[derive(Debug, Clone)]
pub struct VideoGenResult {
    /// Encoded video bytes (MP4).
    pub bytes: Vec<u8>,
    /// MIME type of `bytes` (`video/mp4`).
    pub mime_type: String,
    /// Best-effort clip duration in seconds (the requested duration).
    pub duration_seconds: f32,
}

/// Trait for video backends (generation only).
#[async_trait]
pub trait VideoBackend: Send + Sync {
    /// Generate a fresh video from a text prompt.
    async fn generate(&self, request: &VideoGenRequest) -> Result<VideoGenResult, ConduitError>;

    /// True when this backend should handle the request. Routing is by
    /// `request.provider`, mirroring `ImageBackend::supports`.
    fn supports(&self, request: &VideoGenRequest) -> bool;

    /// Backend name for logging.
    fn name(&self) -> &str;
}

/// Video service facade over one or more backends, carrying defaults so callers
/// can omit model/resolution/duration.
pub struct VideoService {
    backends: Vec<Arc<dyn VideoBackend>>,
    fallback: Arc<dyn VideoBackend>,
    default_model: String,
    default_resolution: String,
    default_duration: u32,
}

impl VideoService {
    /// Create a service from explicit backends + defaults.
    pub fn new(
        backends: Vec<Arc<dyn VideoBackend>>,
        default_model: impl Into<String>,
        default_resolution: impl Into<String>,
        default_duration: u32,
    ) -> Self {
        Self {
            backends,
            fallback: Arc::new(StubVideoBackend::new()),
            default_model: default_model.into(),
            default_resolution: default_resolution.into(),
            default_duration,
        }
    }

    /// Assemble the video service from the environment, registering every hosted
    /// backend whose API key is present:
    ///
    /// - rath → Fal (`FAL_AUTH_KEY`) — LTX-Video on Fal's queue; the primary
    ///   path (claims the default, provider-less request).
    /// - OpenAI Sora (`OPENAI_API_KEY`) — secondary; fires only when a prompt
    ///   explicitly authors `provider = "openai"` (mirrors the image split).
    ///
    /// Returns `None` when no provider key is configured (video generation
    /// disabled, gracefully). This env gate is the opt-in "flag".
    pub fn from_env() -> Option<Self> {
        let mut backends: Vec<Arc<dyn VideoBackend>> = Vec::new();

        // rath → Fal first — `find(supports)` returns the first match and only
        // this backend claims an omitted provider.
        if let Some(backend) = RathVideoBackend::from_env() {
            log::info!("Fal (rath) video backend configured");
            backends.push(Arc::new(backend));
        }
        if let Some(backend) = OpenAiVideoBackend::from_env() {
            log::info!("OpenAI (Sora) video backend configured");
            backends.push(Arc::new(backend));
        }

        if backends.is_empty() {
            return None;
        }

        let default_model = std::env::var("VIDEO_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "fal-ai/ltx-video".to_string());
        let default_resolution = std::env::var("VIDEO_RESOLUTION")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "1920x1080".to_string());
        let default_duration = std::env::var("VIDEO_DURATION_SECS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|d| *d > 0)
            .unwrap_or(6);

        Some(Self::new(
            backends,
            default_model,
            default_resolution,
            default_duration,
        ))
    }

    /// Create a stub-backed service (for testing).
    pub fn new_stub() -> Self {
        Self {
            backends: Vec::new(),
            fallback: Arc::new(StubVideoBackend::new()),
            default_model: "stub-video".to_string(),
            default_resolution: "256x256".to_string(),
            default_duration: 2,
        }
    }

    /// Generate a video, routing to the backend that `supports` the request's
    /// provider. Empty model/resolution/duration fall back to the service
    /// defaults. Falls back to the stub when no registered backend claims it.
    pub async fn generate(
        &self,
        mut request: VideoGenRequest,
    ) -> Result<VideoGenResult, ConduitError> {
        if request.prompt.trim().is_empty() {
            return Err(ConduitError::InvalidRequest(
                "Cannot generate a video from an empty prompt".to_string(),
            ));
        }
        if request.model.trim().is_empty() {
            request.model = self.default_model.clone();
        }
        if request
            .resolution
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            request.resolution = Some(self.default_resolution.clone());
        }
        if request.duration.is_none() {
            request.duration = Some(self.default_duration);
        }

        let backend = self
            .backends
            .iter()
            .find(|b| b.supports(&request))
            .unwrap_or(&self.fallback);

        log::debug!(
            "Generating video: backend={}, provider={:?}, model={}, resolution={:?}, duration={:?}, seed={:?}",
            backend.name(),
            request.provider,
            request.model,
            request.resolution,
            request.duration,
            request.seed
        );
        backend.generate(&request).await
    }

    /// Names of the registered backends (for logging/diagnostics).
    pub fn backend_names(&self) -> Vec<&str> {
        self.backends.iter().map(|b| b.name()).collect()
    }
}
