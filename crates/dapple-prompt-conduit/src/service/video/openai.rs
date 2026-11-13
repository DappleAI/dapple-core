//! OpenAI (Sora) video backend (direct HTTP, generation only).
//!
//! Talks to the async videos API: `POST /v1/videos` submits a job, `GET
//! /v1/videos/{id}` polls it to completion, `GET /v1/videos/{id}/content`
//! downloads the MP4. Direct `reqwest` (not `rig`/`rath`) mirrors
//! `service/image/openai.rs` — the job-poll shape and the `seconds`/`size`
//! vocabulary are Sora-specific. There is no edit/remix path: a model never
//! reprocesses existing pixels (see `service/video/mod.rs`).
//!
//! Sora accepts a fixed clip-length vocabulary (`seconds` ∈ {4, 8, 12}) and a
//! fixed size vocabulary per model. The requested `duration` is clamped UP to
//! the nearest allowed length (a story beat needs at least its requested
//! footage); unsupported `resolution` values are mapped by orientation to the
//! nearest supported size. Authors that need an exact provider value set it in
//! `provider_config`, which is merged into the request body verbatim (author
//! wins).

use super::{VideoBackend, VideoGenRequest, VideoGenResult};
use crate::error::ConduitError;
use crate::service::rath_util;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

/// Clip lengths Sora accepts, ascending.
const SORA_SECONDS: [u32; 3] = [4, 8, 12];

/// Sizes Sora accepts (sora-2; sora-2-pro additionally supports the 1024/1792
/// pair, validated provider-side).
const SORA_LANDSCAPE: &str = "1280x720";
const SORA_PORTRAIT: &str = "720x1280";

#[derive(Debug, Deserialize)]
struct VideoJob {
    id: String,
    status: String,
    #[serde(default)]
    error: Option<VideoJobError>,
}

#[derive(Debug, Deserialize)]
struct VideoJobError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Direct-HTTP OpenAI Sora video backend.
pub struct OpenAiVideoBackend {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    poll_interval: Duration,
    poll_timeout: Duration,
}

impl OpenAiVideoBackend {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            // Trim a trailing slash so `{base}/videos/...` never doubles up.
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            poll_interval: Duration::from_secs(5),
            poll_timeout: Duration::from_secs(900),
        }
    }

    /// Build from the environment. `None` unless `OPENAI_API_KEY` is set — the
    /// same opt-in gate the image/TTS OpenAI backends use.
    /// `OPENAI_VIDEO_BASE_URL` overrides the API base (default
    /// `https://api.openai.com/v1`); `VIDEO_POLL_INTERVAL_SECS` tunes the job
    /// poll cadence (default 5s, shared with the Fal backend);
    /// `VIDEO_POLL_TIMEOUT_SECS` caps the total poll wait (default 900s).
    pub fn from_env() -> Option<Self> {
        if !rath_util::openai_key_present() {
            return None;
        }
        let api_key = std::env::var(rath_util::OPENAI_API_KEY_ENV).ok()?;
        let base_url = std::env::var("OPENAI_VIDEO_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let mut backend = Self::new(api_key, base_url);
        if let Some(secs) = std::env::var("VIDEO_POLL_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| *s > 0)
        {
            backend.poll_interval = Duration::from_secs(secs);
        }
        if let Some(secs) = std::env::var("VIDEO_POLL_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| *s > 0)
        {
            backend.poll_timeout = Duration::from_secs(secs);
        }
        Some(backend)
    }

    /// Clamp a requested duration UP to the nearest Sora clip length (4/8/12).
    /// `None` and anything past 12 land on 12; a request never receives less
    /// footage than it asked for below the ceiling.
    fn clamp_seconds(duration: Option<u32>) -> u32 {
        let requested = duration.unwrap_or(SORA_SECONDS[0]);
        SORA_SECONDS
            .iter()
            .copied()
            .find(|s| *s >= requested)
            .unwrap_or(SORA_SECONDS[2])
    }

    /// Map a requested `WIDTHxHEIGHT` onto Sora's size vocabulary. Exact
    /// supported values pass through; anything else maps by orientation to the
    /// nearest supported size (with a log line, so a silently-adjusted guide is
    /// diagnosable). Authors needing sora-2-pro's larger sizes set them
    /// verbatim (they pass through) or via `provider_config.size`.
    fn map_size(resolution: Option<&str>) -> String {
        let requested = match resolution {
            Some(r) if !r.trim().is_empty() => r.trim(),
            _ => return SORA_LANDSCAPE.to_string(),
        };
        match requested {
            "1280x720" | "720x1280" | "1024x1792" | "1792x1024" => requested.to_string(),
            other => {
                let mapped = match other
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
                {
                    Some((w, h)) if h > w => SORA_PORTRAIT,
                    _ => SORA_LANDSCAPE,
                };
                log::info!("sora: mapped unsupported resolution {other} -> {mapped}");
                mapped.to_string()
            }
        }
    }

    /// Read a job response body, surfacing provider errors as `VideoError`.
    fn parse_job(body: &str) -> Result<VideoJob, ConduitError> {
        serde_json::from_str(body)
            .map_err(|e| ConduitError::VideoError(format!("failed to parse video job: {e}")))
    }

    fn job_error(job: &VideoJob) -> String {
        match &job.error {
            Some(e) => format!(
                "video job {} failed: {} {}",
                job.id,
                e.code.as_deref().unwrap_or("unknown"),
                e.message.as_deref().unwrap_or("")
            ),
            None => format!("video job {} failed", job.id),
        }
    }
}

#[async_trait]
impl VideoBackend for OpenAiVideoBackend {
    async fn generate(&self, request: &VideoGenRequest) -> Result<VideoGenResult, ConduitError> {
        let seconds = Self::clamp_seconds(request.duration);
        let mut body = json!({
            "model": request.model,
            "prompt": request.prompt,
            // The API takes the clip length as a string enum ("4"|"8"|"12").
            "seconds": seconds.to_string(),
            "size": Self::map_size(request.resolution.as_deref()),
        });

        // Author `provider_config` merges verbatim on top (author wins) so
        // provider knobs need no conduit code — mirrors the rath backends.
        if let Some(Value::Object(author)) = &request.provider_config {
            for (k, v) in author {
                body[k] = v.clone();
            }
        }

        // 1. Submit the job.
        let resp = self
            .client
            .post(format!("{}/videos", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| ConduitError::VideoError(format!("video request failed: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ConduitError::VideoError(format!("failed to read video response: {e}")))?;
        if !status.is_success() {
            return Err(ConduitError::VideoError(format!(
                "video provider error {status}: {text}"
            )));
        }
        let mut job = Self::parse_job(&text)?;

        // 2. Poll to completion (a clip takes minutes; the SSE handler holds
        //    the stream open, same as the Fal queue path).
        let deadline = std::time::Instant::now() + self.poll_timeout;
        while job.status != "completed" {
            if job.status == "failed" {
                return Err(ConduitError::VideoError(Self::job_error(&job)));
            }
            if std::time::Instant::now() >= deadline {
                return Err(ConduitError::VideoError(format!(
                    "video job {} timed out after {}s (status {})",
                    job.id,
                    self.poll_timeout.as_secs(),
                    job.status
                )));
            }
            tokio::time::sleep(self.poll_interval).await;

            let resp = self
                .client
                .get(format!("{}/videos/{}", self.base_url, job.id))
                .bearer_auth(&self.api_key)
                .send()
                .await
                .map_err(|e| ConduitError::VideoError(format!("video poll failed: {e}")))?;
            let status = resp.status();
            let text = resp.text().await.map_err(|e| {
                ConduitError::VideoError(format!("failed to read video poll response: {e}"))
            })?;
            if !status.is_success() {
                return Err(ConduitError::VideoError(format!(
                    "video poll error {status}: {text}"
                )));
            }
            job = Self::parse_job(&text)?;
        }

        // 3. Download the MP4.
        let resp = self
            .client
            .get(format!("{}/videos/{}/content", self.base_url, job.id))
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| ConduitError::VideoError(format!("video download failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ConduitError::VideoError(format!(
                "video download error {status}: {text}"
            )));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ConduitError::VideoError(format!("failed to read video bytes: {e}")))?
            .to_vec();

        Ok(VideoGenResult {
            bytes,
            mime_type: "video/mp4".to_string(),
            duration_seconds: seconds as f32,
        })
    }

    /// Fires only when a prompt explicitly authors `provider = "openai"` —
    /// the Fal/rath backend keeps the default (no provider) route, mirroring
    /// the image-backend split.
    fn supports(&self, request: &VideoGenRequest) -> bool {
        matches!(request.provider.as_deref(), Some("openai"))
    }

    fn name(&self) -> &str {
        "openai-video"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_duration_up_to_sora_lengths() {
        assert_eq!(OpenAiVideoBackend::clamp_seconds(None), 4);
        assert_eq!(OpenAiVideoBackend::clamp_seconds(Some(1)), 4);
        assert_eq!(OpenAiVideoBackend::clamp_seconds(Some(4)), 4);
        assert_eq!(OpenAiVideoBackend::clamp_seconds(Some(5)), 8);
        assert_eq!(OpenAiVideoBackend::clamp_seconds(Some(8)), 8);
        assert_eq!(OpenAiVideoBackend::clamp_seconds(Some(9)), 12);
        assert_eq!(OpenAiVideoBackend::clamp_seconds(Some(60)), 12);
    }

    #[test]
    fn maps_resolution_onto_sora_sizes() {
        assert_eq!(OpenAiVideoBackend::map_size(None), "1280x720");
        assert_eq!(OpenAiVideoBackend::map_size(Some("")), "1280x720");
        assert_eq!(OpenAiVideoBackend::map_size(Some("1280x720")), "1280x720");
        assert_eq!(OpenAiVideoBackend::map_size(Some("720x1280")), "720x1280");
        assert_eq!(OpenAiVideoBackend::map_size(Some("1792x1024")), "1792x1024");
        // Unsupported sizes map by orientation.
        assert_eq!(OpenAiVideoBackend::map_size(Some("1920x1080")), "1280x720");
        assert_eq!(OpenAiVideoBackend::map_size(Some("1080x1920")), "720x1280");
        assert_eq!(OpenAiVideoBackend::map_size(Some("512x512")), "1280x720");
        assert_eq!(OpenAiVideoBackend::map_size(Some("garbage")), "1280x720");
    }

    #[test]
    fn supports_only_explicit_openai_provider() {
        let b = OpenAiVideoBackend::new("k".into(), "https://api.openai.com/v1".into());
        let mut r = VideoGenRequest {
            provider: Some("openai".into()),
            prompt: "p".into(),
            model: "sora-2".into(),
            duration: Some(8),
            resolution: None,
            fps: None,
            generate_audio: None,
            seed: None,
            negative_prompt: None,
            camera_motion: None,
            provider_config: None,
        };
        assert!(b.supports(&r));
        r.provider = None;
        assert!(!b.supports(&r));
        r.provider = Some("fal".into());
        assert!(!b.supports(&r));
    }

    #[test]
    fn parses_job_status_and_error() {
        let job = OpenAiVideoBackend::parse_job(
            r#"{"id":"video_123","status":"in_progress","progress":42}"#,
        )
        .unwrap();
        assert_eq!(job.id, "video_123");
        assert_eq!(job.status, "in_progress");

        let failed = OpenAiVideoBackend::parse_job(
            r#"{"id":"video_9","status":"failed","error":{"code":"moderation_blocked","message":"nope"}}"#,
        )
        .unwrap();
        assert_eq!(failed.status, "failed");
        let msg = OpenAiVideoBackend::job_error(&failed);
        assert!(msg.contains("moderation_blocked"));
        assert!(msg.contains("nope"));
    }
}
