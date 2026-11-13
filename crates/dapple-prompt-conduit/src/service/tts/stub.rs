//! Deterministic stub TTS backend for tests and key-less environments.
//!
//! `synthesize` returns a tiny, valid-enough WAV header + prompt-derived bytes so
//! distinct inputs yield distinct output, without calling any external service.
//! It is the service's unconditional fallback.

use super::{TtsBackend, TtsGenRequest, TtsGenResult};
use crate::error::ConduitError;
use async_trait::async_trait;

/// Minimal RIFF/WAVE header (44 bytes, PCM mono 24 kHz) so the bytes are
/// recognizably a WAV container. Not a playable clip — the stub never touches a
/// real model; downstream code (asset persistence, mime tagging) just needs
/// non-empty, well-formed-enough bytes.
const WAV_HEADER: &[u8] = &[
    b'R', b'I', b'F', b'F', 0, 0, 0, 0, b'W', b'A', b'V', b'E', // RIFF chunk
    b'f', b'm', b't', b' ', 16, 0, 0, 0, 1, 0, 1, 0, // fmt: PCM, mono
    0xC0, 0x5D, 0x00, 0x00, 0x80, 0xBB, 0x00, 0x00, 2, 0, 16, 0, // 24000 Hz
    b'd', b'a', b't', b'a', 0, 0, 0, 0, // data chunk
];

pub struct StubTtsBackend;

impl StubTtsBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StubTtsBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TtsBackend for StubTtsBackend {
    async fn synthesize(&self, request: &TtsGenRequest) -> Result<TtsGenResult, ConduitError> {
        let mut bytes = WAV_HEADER.to_vec();
        // Append input-derived bytes so distinct inputs differ.
        let seed = request
            .input
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        bytes.extend_from_slice(&seed.to_le_bytes());
        Ok(TtsGenResult {
            bytes,
            mime_type: "audio/wav".to_string(),
            duration_seconds: 0.0,
            sample_rate: 24000,
        })
    }

    /// The stub is the service's unconditional fallback, so it claims anything.
    fn supports(&self, _request: &TtsGenRequest) -> bool {
        true
    }

    fn name(&self) -> &str {
        "stub-tts"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(input: &str) -> TtsGenRequest {
        TtsGenRequest {
            provider: None,
            input: input.to_string(),
            model: "stub-tts".to_string(),
            voice: None,
            format: None,
            speed: None,
            provider_config: None,
        }
    }

    #[tokio::test]
    async fn stub_returns_wav() {
        let backend = StubTtsBackend::new();
        let out = backend.synthesize(&req("Hello world")).await.unwrap();
        assert_eq!(out.mime_type, "audio/wav");
        assert_eq!(&out.bytes[0..4], b"RIFF");
        assert_eq!(&out.bytes[8..12], b"WAVE");
    }

    #[tokio::test]
    async fn stub_is_input_sensitive() {
        let backend = StubTtsBackend::new();
        let a = backend.synthesize(&req("alpha")).await.unwrap();
        let b = backend.synthesize(&req("beta")).await.unwrap();
        assert_ne!(a.bytes, b.bytes);
    }
}
