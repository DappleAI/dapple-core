//! Deterministic stub video backend for tests and key-less environments.
//!
//! `generate` returns a tiny, valid-enough MP4 header + prompt-derived bytes so
//! distinct prompts yield distinct output, without calling any external service.
//! Generate-only: there is no `edit` (see `service/video/mod.rs`).

use super::{VideoBackend, VideoGenRequest, VideoGenResult};
use crate::error::ConduitError;
use async_trait::async_trait;

/// Minimal MP4 `ftyp` box so the bytes are recognizably a video container.
/// Enough for downstream code (asset persistence, mime tagging) to treat it as
/// an mp4; it is NOT a playable clip — the stub never touches a real model.
const FTYP_BOX: &[u8] = &[
    0x00, 0x00, 0x00, 0x18, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0x00, 0x00, 0x02, 0x00,
    b'i', b's', b'o', b'm', b'i', b's', b'o', b'2',
];

pub struct StubVideoBackend;

impl StubVideoBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StubVideoBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VideoBackend for StubVideoBackend {
    async fn generate(&self, request: &VideoGenRequest) -> Result<VideoGenResult, ConduitError> {
        let mut bytes = FTYP_BOX.to_vec();
        // Append prompt-derived bytes so distinct prompts differ.
        let seed = request
            .prompt
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        bytes.extend_from_slice(&seed.to_le_bytes());
        Ok(VideoGenResult {
            bytes,
            mime_type: "video/mp4".to_string(),
            duration_seconds: request.duration.unwrap_or(2) as f32,
        })
    }

    /// The stub is the service's unconditional fallback, so it claims anything.
    fn supports(&self, _request: &VideoGenRequest) -> bool {
        true
    }

    fn name(&self) -> &str {
        "stub-video"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(prompt: &str) -> VideoGenRequest {
        VideoGenRequest {
            provider: None,
            prompt: prompt.to_string(),
            model: "stub-video".to_string(),
            duration: Some(3),
            resolution: Some("256x256".to_string()),
            fps: None,
            generate_audio: None,
            seed: None,
            negative_prompt: None,
            camera_motion: None,
            provider_config: None,
        }
    }

    #[tokio::test]
    async fn stub_generate_returns_mp4() {
        let backend = StubVideoBackend::new();
        let out = backend.generate(&req("a sweeping vista")).await.unwrap();
        assert_eq!(out.mime_type, "video/mp4");
        assert_eq!(out.duration_seconds, 3.0);
        assert!(out.bytes.starts_with(&FTYP_BOX[..8]));
    }

    #[tokio::test]
    async fn stub_generate_is_prompt_sensitive() {
        let backend = StubVideoBackend::new();
        let a = backend.generate(&req("alpha")).await.unwrap();
        let b = backend.generate(&req("beta")).await.unwrap();
        assert_ne!(a.bytes, b.bytes);
    }
}
