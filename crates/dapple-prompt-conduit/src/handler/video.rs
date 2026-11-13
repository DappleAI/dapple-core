//! Video generation handler.
//!
//! `handle_video_prompt` (SSE) is the video analog of `handle_image_prompt`:
//! routed from `handle_prompt` when a prompt's `maker_keys` contain
//! `VIDEO_MAKER_KEY`. It resolves config from the prompt's `engine_body`,
//! generates via the video service (LTX-2: submit → poll → download), persists
//! the MP4 as a `video` asset, and emits `PromptEvent::asset`. Generation is the
//! ONLY model call; there is no edit path.
//!
//! NOTE: LTX generation is asynchronous and takes minutes. This handler holds
//! the SSE stream open while the backend polls, so callers (the flow client)
//! must use a correspondingly long read timeout for video prompts.

use axum::response::sse::{Event, Sse};
use dapple_data::query::prompts::select_prompt_engine_body_for_maker;
use futures::stream::BoxStream;
use serde::Deserialize;
use serde_json::Value;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::error::ConduitError;
use crate::event::PromptEvent;
use crate::handler::image::persist_media_asset;
use crate::handler::prompt::PromptRequest;
use crate::service::video::VideoGenRequest;
use crate::service::AppState;

/// Type alias for the SSE stream used by prompt handlers (matches `prompt.rs`).
type PromptStream = BoxStream<'static, Result<Event, Infallible>>;

/// Lineage/maker key that routes a prompt to the video generator. A guide's
/// `:::inference-profile` declares `.lineage = "video"`; the model id
/// (a Fal slug, e.g. `fal-ai/ltx-video`), resolution, duration, and provider live
/// in the engine_body JSON. (Maker key == lineage key — see `select_prompt_lineage_keys`.)
pub const VIDEO_MAKER_KEY: &str = "video";

/// Config parsed from a video prompt's `engine_body` (JSON).
#[derive(Debug, Default, Deserialize)]
struct VideoPromptConfig {
    /// Which hosted engine to route to (`ltx`). Selects the backend like the
    /// LLM/image `provider`.
    provider: Option<String>,
    /// Provider model id; falls back to the video service default.
    model: Option<String>,
    /// Clip length in seconds; falls back to the service default.
    duration: Option<u32>,
    /// Output resolution `WIDTHxHEIGHT`; falls back to the service default.
    resolution: Option<String>,
    /// Frames per second (default 24).
    fps: Option<u32>,
    /// Whether to generate the synced audio track (default true).
    generate_audio: Option<bool>,
    /// Optional generation seed — pinned for reproducibility.
    seed: Option<i64>,
    /// Optional negative prompt.
    negative_prompt: Option<String>,
    /// Optional camera motion hint.
    camera_motion: Option<String>,
    /// Optional style preamble prepended to the (LLM-assembled) description.
    style: Option<String>,
    /// Raw Fal-model `provider_config` passthrough (resolution, num_frames, …).
    /// Forwarded verbatim to the rath backend so per-model knobs need no conduit
    /// code.
    provider_config: Option<Value>,
}

/// Pull the video description from the request (preferred) or the attempt
/// context. Accepts any of `prompt`/`description`/`text`/`input` (mirrors the
/// image handler).
fn extract_video_prompt(req_context: &Option<Value>, attempt_context: &Value) -> Option<String> {
    let pick = |v: &Value| {
        v.get("prompt")
            .or_else(|| v.get("description"))
            .or_else(|| v.get("text"))
            .or_else(|| v.get("input"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
    };
    if let Some(rc) = req_context {
        if let Some(s) = pick(rc) {
            if !s.trim().is_empty() {
                return Some(s);
            }
        }
    }
    pick(attempt_context).filter(|s| !s.trim().is_empty())
}

/// Generate a fresh video from a prompt and persist it. SSE so it shares the
/// prompt-stream shape (Started → Asset → Done / Error).
pub async fn handle_video_prompt(
    state: AppState,
    req: PromptRequest,
    request_id: Uuid,
    start: Instant,
) -> Result<Sse<PromptStream>, ConduitError> {
    log::info!(
        "Handling video prompt: request_id={}, prompt_id={}",
        request_id,
        req.prompt_id
    );

    let pool = Arc::clone(&state.pool);

    let stream = async_stream::stream! {
        let started = PromptEvent::started(request_id, "video", req.prompt_id);
        yield Ok(Event::default().data(started.to_sse_data()));

        // 1. Video service availability (off unless FAL_AUTH_KEY configured)
        let video_service = match &state.video_service {
            Some(svc) => svc.clone(),
            None => {
                let error = PromptEvent::error("video_unavailable", "Video service is not configured", false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 2. Fetch attempt context (description fallback)
        let attempt_context = match state.context_service
            .fetch_context_for_attempt(req.attempt_id)
            .await
        {
            Ok(ctx) => ctx,
            Err(e) => {
                let error = PromptEvent::error("context_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 3. Resolve config from the prompt's engine_body
        let engine_body = select_prompt_engine_body_for_maker(&pool, req.prompt_id, VIDEO_MAKER_KEY)
            .await
            .ok()
            .flatten();
        let cfg: VideoPromptConfig = engine_body
            .as_deref()
            .and_then(|b| serde_json::from_str(b).ok())
            .unwrap_or_default();

        // 4. Assemble the video prompt: style preamble + LLM-assembled description
        let description = match extract_video_prompt(&req.context_data, &attempt_context.context_data) {
            Some(d) => d,
            None => {
                let error = PromptEvent::error(
                    "input_error",
                    "No video prompt/description in context_data (expected one of prompt/description/text/input)",
                    false,
                );
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };
        let prompt_text = match cfg.style.as_deref() {
            Some(style) if !style.trim().is_empty() => format!("{}\n\n{}", style.trim(), description),
            _ => description,
        };

        // 5. Generate (submit → poll → download; may take minutes)
        let gen_request = VideoGenRequest {
            provider: cfg.provider.clone(),
            prompt: prompt_text.clone(),
            model: cfg.model.clone().unwrap_or_default(),
            duration: cfg.duration,
            resolution: cfg.resolution.clone(),
            fps: cfg.fps,
            generate_audio: cfg.generate_audio,
            seed: cfg.seed,
            negative_prompt: cfg.negative_prompt.clone(),
            camera_motion: cfg.camera_motion.clone(),
            provider_config: cfg.provider_config.clone(),
        };
        let result = match video_service.generate(gen_request).await {
            Ok(r) => r,
            Err(e) => {
                // Video-provider failures are retryable (rate limits, transient
                // 5xx, poll timeouts).
                let error = PromptEvent::error("video_error", e.to_string(), true);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        if result.bytes.is_empty() {
            let error = PromptEvent::error("video_error", "video provider returned 0 bytes (empty video)", true);
            yield Ok(Event::default().data(error.to_sse_data()));
            return;
        }

        // 6. Persist as a video asset
        let (asset_id, content_length) = match persist_media_asset(&pool, &result.bytes, "video", &result.mime_type).await {
            Ok(v) => v,
            Err(e) => {
                let error = PromptEvent::error("persist_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        log::info!(
            "Video asset created: asset_id={}, content_length={}, duration={}s",
            asset_id,
            content_length,
            result.duration_seconds
        );

        // 7. Asset + Done events
        let asset_event = PromptEvent::asset(asset_id, "video", result.mime_type.clone(), content_length, result.duration_seconds, 0, prompt_text);
        yield Ok(Event::default().data(asset_event.to_sse_data()));

        let latency_ms = start.elapsed().as_millis() as u64;
        let done = PromptEvent::done(0, 0, latency_ms, "asset_created");
        yield Ok(Event::default().data(done.to_sse_data()));
    };

    Ok(Sse::new(Box::pin(stream)))
}
