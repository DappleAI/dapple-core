//! Fal video backend via rath-rs (generation only).
//!
//! Replaces the hand-rolled LTX backend. Video is reached through rath's
//! provider-agnostic `VideoClient` with Fal as the provider — the LTX-Video
//! model "moved over" to Fal's queue. rath's `generate_video` submits the job
//! and polls to completion (a clip takes minutes), so the calling handler holds
//! the SSE stream open for the poll. The model slug and all Fal-model-specific
//! knobs (resolution, num_frames, etc.) are authored in the engine_body
//! (`provider`/`model`/`provider_config` — see `handler/video.rs`).
//!
//! Generation-only: a model never reprocesses existing pixels (see
//! `service/video/mod.rs`).

use super::{VideoBackend, VideoGenRequest, VideoGenResult};
use crate::error::ConduitError;
use crate::service::rath_util;
use async_trait::async_trait;
use rath::video::{VideoOptions, VideoRequest};
use serde_json::{Map, Value};
use std::time::Duration;

/// rath-backed Fal video backend.
pub struct RathVideoBackend {
    http: reqwest::Client,
    poll_interval: Duration,
}

impl RathVideoBackend {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            poll_interval: Duration::from_secs(5),
        }
    }

    /// Build from the environment. `None` unless `FAL_AUTH_KEY` is set — the
    /// opt-in gate that keeps video off by default. `VIDEO_POLL_INTERVAL_SECS`
    /// tunes the Fal queue poll cadence (default 5s).
    pub fn from_env() -> Option<Self> {
        if !rath_util::fal_key_present() {
            return None;
        }
        let poll_interval = std::env::var("VIDEO_POLL_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(5);
        Some(Self {
            http: reqwest::Client::new(),
            poll_interval: Duration::from_secs(poll_interval),
        })
    }

    /// Assemble the Fal `provider_config`: a conduit-derived `seed` default with
    /// the author's engine_body `provider_config` merged on top (author wins —
    /// resolution, num_frames, and the rest are the specific Fal model's schema).
    fn provider_config(request: &VideoGenRequest) -> Option<Value> {
        let mut cfg = Map::new();
        if let Some(seed) = request.seed.filter(|s| *s >= 0) {
            cfg.insert("seed".to_string(), Value::from(seed));
        }
        if let Some(Value::Object(author)) = &request.provider_config {
            for (k, v) in author {
                cfg.insert(k.clone(), v.clone());
            }
        }
        (!cfg.is_empty()).then_some(Value::Object(cfg))
    }
}

impl Default for RathVideoBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VideoBackend for RathVideoBackend {
    async fn generate(&self, request: &VideoGenRequest) -> Result<VideoGenResult, ConduitError> {
        let url = rath_util::fal_model_url(&request.model);
        let req = VideoRequest {
            prompt: request.prompt.clone(),
            image_url: None,
            provider_config: Self::provider_config(request),
        };
        let client = VideoOptions {
            provider_config: None,
            poll_interval: self.poll_interval,
        }
        .create(&url)
        .map_err(rath_util::video_err)?;
        // `generate_video` submits then polls to completion (Fal queue).
        let response = client
            .generate_video(&req)
            .await
            .map_err(rath_util::video_err)?;
        let first = response.videos.first().ok_or_else(|| {
            ConduitError::VideoError("fal video response had no videos".to_string())
        })?;
        let (bytes, mime_type) = rath_util::video_bytes(&self.http, first).await?;
        Ok(VideoGenResult {
            bytes,
            mime_type,
            duration_seconds: request.duration.unwrap_or(0) as f32,
        })
    }

    /// Routes the Fal provider and the default (no provider); `ltx` is accepted
    /// too so a guide authored against the old direct backend still resolves
    /// here. The stub is the unconditional fallback.
    fn supports(&self, request: &VideoGenRequest) -> bool {
        matches!(
            request.provider.as_deref(),
            None | Some("") | Some("fal") | Some("ltx") | Some("rath")
        )
    }

    fn name(&self) -> &str {
        "fal-video"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(provider: Option<&str>) -> VideoGenRequest {
        VideoGenRequest {
            provider: provider.map(String::from),
            prompt: "a slow dolly over a market at dawn".to_string(),
            model: "fal-ai/ltx-video".to_string(),
            duration: Some(6),
            resolution: Some("1920x1080".to_string()),
            fps: Some(24),
            generate_audio: Some(true),
            seed: None,
            negative_prompt: None,
            camera_motion: None,
            provider_config: None,
        }
    }

    #[test]
    fn supports_fal_ltx_and_default() {
        let b = RathVideoBackend::new();
        assert!(b.supports(&req(None)));
        assert!(b.supports(&req(Some("fal"))));
        assert!(b.supports(&req(Some("ltx"))));
        assert!(b.supports(&req(Some("rath"))));
        let mut other = req(Some("openai"));
        other.provider = Some("openai".into());
        assert!(!b.supports(&other));
    }

    #[test]
    fn provider_config_seed_and_author_override() {
        let mut r = req(Some("fal"));
        r.seed = Some(7);
        r.provider_config = Some(json!({ "resolution": "720p", "num_frames": 121, "seed": 8 }));
        let cfg = RathVideoBackend::provider_config(&r).unwrap();
        assert_eq!(cfg["resolution"], json!("720p"));
        assert_eq!(cfg["num_frames"], json!(121));
        assert_eq!(cfg["seed"], json!(8), "author seed wins");
    }

    #[test]
    fn provider_config_none_when_empty() {
        let r = req(Some("fal"));
        // No seed, no author config.
        assert!(RathVideoBackend::provider_config(&r).is_none());
    }
}
