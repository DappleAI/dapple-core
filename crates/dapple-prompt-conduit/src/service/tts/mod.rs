//! Text-to-speech service for Conduit (generate-only, hosted).
//!
//! The audio analog of the image/video services: a backend trait, env-loaded
//! defaults, and a service facade that routes per-request to the backend whose
//! `supports` claims it — the same "engines like the LLMs" dispatch the
//! image/video services use. The only operation is `synthesize` — text → speech
//! audio bytes.
//!
//! ## Opt-in / off by default
//!
//! TTS is disabled unless a provider key is configured. `from_env` returns `None`
//! when no backend key is present, so `AppState.tts_service` stays `None` and TTS
//! prompts fail cleanly with `tts_unavailable`. The OpenAI-via-rath backend gates
//! on `OPENAI_API_KEY` (the same key the gpt-image image backend uses). rath is a
//! lightweight `reqwest`-based dep, so it needs no cargo feature.

mod rath;
mod stub;

pub use rath::RathTtsBackend;
pub use stub::StubTtsBackend;

use crate::error::ConduitError;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// A text→speech synthesis request.
#[derive(Debug, Clone)]
pub struct TtsGenRequest {
    /// Which hosted engine to route to. `None`/empty, `"openai"`, and `"rath"`
    /// all route to the OpenAI-via-rath backend (the only TTS provider).
    /// Authored in the guide's `:::inference-profile` engine_body; mirrors the
    /// image/video `provider` selector.
    pub provider: Option<String>,
    /// The text to speak.
    pub input: String,
    /// Provider model id (e.g. `gpt-4o-mini-tts`, `tts-1`). Falls back to the
    /// service default.
    pub model: String,
    /// Voice name (`alloy`/`echo`/`fable`/`onyx`/`nova`/`shimmer`). Falls back to
    /// the service default.
    pub voice: Option<String>,
    /// Output audio format (`mp3`/`opus`/`aac`/`flac`/`wav`/`pcm`). Maps to
    /// OpenAI's `response_format` and drives the result mime. Falls back to the
    /// service default.
    pub format: Option<String>,
    /// Optional speech-rate multiplier. rath has no typed field for it, so the
    /// backend folds it into `provider_config`.
    pub speed: Option<f32>,
    /// Raw provider `provider_config` passthrough, authored in the engine_body.
    /// The rath backend merges it verbatim into the request payload (author wins)
    /// so per-model knobs (e.g. gpt-4o-mini-tts `instructions`) need no conduit
    /// code. Ignored by the stub backend.
    pub provider_config: Option<Value>,
}

/// The output of a synthesis.
#[derive(Debug, Clone)]
pub struct TtsGenResult {
    /// Encoded audio bytes.
    pub bytes: Vec<u8>,
    /// MIME type of `bytes` (e.g. `audio/mpeg`).
    pub mime_type: String,
    /// Best-effort duration in seconds. Hosted TTS does not report it, so the rath
    /// backend derives it from the WAV header for the wav format (0.0 for other
    /// formats / when unknown). The asset event carries it, and the TTS handler
    /// uses it to detect runaway (degenerate, over-long) returns.
    pub duration_seconds: f32,
    /// Best-effort sample rate (Hz). Read from the WAV header for the wav format,
    /// 0 when unknown (hosted TTS).
    pub sample_rate: u32,
}

/// Derive `(duration_seconds, sample_rate)` from a RIFF/WAVE buffer, or `None` if
/// it is not recognizably WAV.
///
/// Hosted TTS does not report duration, and OpenAI's streamed `wav` output
/// declares a placeholder `data` size (`0xFFFFFFFF`), so we cannot trust the
/// header's length field — the real audio length is `min(declared, bytes present)`.
/// We walk the chunks (honoring the RIFF odd-length pad byte) to read the format
/// and locate the `data` body, then compute `data_bytes / (rate * channels *
/// bytes_per_sample)`. Pure header math — no full decode, no new deps.
pub(crate) fn wav_spec_duration(bytes: &[u8]) -> Option<(f32, u32)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut fmt: Option<(u16, u32, u16)> = None; // (channels, sample_rate, bits)
    let mut data: Option<(usize, usize)> = None; // (data_body_start, declared_len)
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= bytes.len() {
            // WAVEFORMAT: channels @2, sample_rate @4, bits_per_sample @14.
            let channels = u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]);
            let sample_rate = u32::from_le_bytes([
                bytes[body + 4],
                bytes[body + 5],
                bytes[body + 6],
                bytes[body + 7],
            ]);
            let bits = u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]);
            fmt = Some((channels, sample_rate, bits));
        }
        if id == b"data" {
            data = Some((body, len));
            break;
        }
        match body.checked_add(len + (len & 1)) {
            Some(next) => pos = next,
            None => break,
        }
    }
    let (channels, sample_rate, bits) = fmt?;
    let (data_start, declared) = data?;
    let bytes_per_sec = sample_rate as usize * channels as usize * (bits as usize / 8);
    if bytes_per_sec == 0 {
        return None;
    }
    let available = bytes.len().saturating_sub(data_start);
    let data_bytes = declared.min(available); // 0xFFFFFFFF placeholder -> clamp to present
    Some((data_bytes as f32 / bytes_per_sec as f32, sample_rate))
}

/// Trait for TTS backends (generation only).
#[async_trait]
pub trait TtsBackend: Send + Sync {
    /// Synthesize speech from text.
    async fn synthesize(&self, request: &TtsGenRequest) -> Result<TtsGenResult, ConduitError>;

    /// True when this backend should handle the request. Routing is by
    /// `request.provider`, mirroring `ImageBackend::supports`.
    fn supports(&self, request: &TtsGenRequest) -> bool;

    /// Backend name for logging.
    fn name(&self) -> &str;
}

/// TTS service facade over one or more backends, carrying defaults so callers can
/// omit model/voice/format.
pub struct TtsService {
    backends: Vec<Arc<dyn TtsBackend>>,
    fallback: Arc<dyn TtsBackend>,
    default_model: String,
    default_voice: String,
    default_format: String,
}

impl TtsService {
    /// Create a service from explicit backends + defaults.
    pub fn new(
        backends: Vec<Arc<dyn TtsBackend>>,
        default_model: impl Into<String>,
        default_voice: impl Into<String>,
        default_format: impl Into<String>,
    ) -> Self {
        Self {
            backends,
            fallback: Arc::new(StubTtsBackend::new()),
            default_model: default_model.into(),
            default_voice: default_voice.into(),
            default_format: default_format.into(),
        }
    }

    /// Assemble the TTS service from the environment, registering every hosted
    /// backend whose API key is present:
    ///
    /// - OpenAI via rath (`OPENAI_API_KEY`) — the only TTS provider.
    ///
    /// Returns `None` when no provider key is configured (TTS disabled,
    /// gracefully). This env gate is the opt-in "flag". `TTS_MODEL`/`TTS_VOICE`/
    /// `TTS_FORMAT` override the per-request defaults.
    pub fn from_env() -> Option<Self> {
        let mut backends: Vec<Arc<dyn TtsBackend>> = Vec::new();

        if let Some(backend) = RathTtsBackend::from_env() {
            log::info!("OpenAI (rath) TTS backend configured");
            backends.push(Arc::new(backend));
        }

        if backends.is_empty() {
            return None;
        }

        // Pinned to a dated snapshot (not the floating `gpt-4o-mini-tts` slug) so
        // an OpenAI TTS version change is a deliberate, visible edit here rather
        // than silent drift. Override per-deploy with `TTS_MODEL`.
        let default_model = std::env::var("TTS_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "gpt-4o-mini-tts-2025-12-15".to_string());
        let default_voice = std::env::var("TTS_VOICE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "alloy".to_string());
        let default_format = std::env::var("TTS_FORMAT")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "mp3".to_string());

        Some(Self::new(
            backends,
            default_model,
            default_voice,
            default_format,
        ))
    }

    /// Create a stub-backed service (for testing).
    pub fn new_stub() -> Self {
        Self {
            backends: Vec::new(),
            fallback: Arc::new(StubTtsBackend::new()),
            default_model: "stub-tts".to_string(),
            default_voice: "stub".to_string(),
            default_format: "wav".to_string(),
        }
    }

    /// Synthesize speech, routing to the backend that `supports` the request's
    /// provider. Empty model/voice/format fall back to the service defaults.
    /// Falls back to the stub when no registered backend claims it.
    pub async fn synthesize(
        &self,
        mut request: TtsGenRequest,
    ) -> Result<TtsGenResult, ConduitError> {
        if request.input.trim().is_empty() {
            return Err(ConduitError::InvalidRequest(
                "Cannot synthesize empty text".to_string(),
            ));
        }
        if request.model.trim().is_empty() {
            request.model = self.default_model.clone();
        }
        if request
            .voice
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            request.voice = Some(self.default_voice.clone());
        }
        if request
            .format
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            request.format = Some(self.default_format.clone());
        }

        let backend = self
            .backends
            .iter()
            .find(|b| b.supports(&request))
            .unwrap_or(&self.fallback);

        log::debug!(
            "Synthesizing speech: backend={}, provider={:?}, model={}, voice={:?}, format={:?}",
            backend.name(),
            request.provider,
            request.model,
            request.voice,
            request.format
        );
        backend.synthesize(&request).await
    }

    /// Names of the registered backends (for logging/diagnostics).
    pub fn backend_names(&self) -> Vec<&str> {
        self.backends.iter().map(|b| b.name()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_service_synthesizes() {
        let service = TtsService::new_stub();
        let result = service
            .synthesize(TtsGenRequest {
                provider: None,
                input: "Hello world".to_string(),
                model: String::new(),
                voice: None,
                format: None,
                speed: None,
                provider_config: None,
            })
            .await
            .unwrap();
        assert!(!result.bytes.is_empty());
        assert!(result.mime_type.starts_with("audio/"));
    }

    #[tokio::test]
    async fn empty_text_is_rejected() {
        let service = TtsService::new_stub();
        let result = service
            .synthesize(TtsGenRequest {
                provider: None,
                input: "   ".to_string(),
                model: String::new(),
                voice: None,
                format: None,
                speed: None,
                provider_config: None,
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn defaults_fill_empty_model_voice_format() {
        // The stub ignores them, but the fill-in path must not panic and must
        // reach the fallback backend.
        let service = TtsService::new_stub();
        let out = service
            .synthesize(TtsGenRequest {
                provider: Some("nonexistent".to_string()),
                input: "hi".to_string(),
                model: "  ".to_string(),
                voice: Some(String::new()),
                format: None,
                speed: None,
                provider_config: None,
            })
            .await
            .unwrap();
        assert!(!out.bytes.is_empty());
    }

    /// Build a canonical mono i16 WAV header for `n` samples at `sr`, with the
    /// `data` (and RIFF) size fields set to the streamed placeholder `0xFFFFFFFF`
    /// — exactly what OpenAI's TTS `wav` emits — followed by `n*2` zero bytes.
    fn placeholder_wav(n: usize, sr: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // channels
        b.extend_from_slice(&sr.to_le_bytes()); // sample rate
        b.extend_from_slice(&(sr * 2).to_le_bytes()); // byte rate
        b.extend_from_slice(&2u16.to_le_bytes()); // block align
        b.extend_from_slice(&16u16.to_le_bytes()); // bits
        b.extend_from_slice(b"data");
        b.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        b.resize(b.len() + n * 2, 0);
        b
    }

    #[test]
    fn wav_spec_duration_clamps_placeholder_size() {
        // 12000 samples @ 24 kHz mono = 0.5s, despite the 0xFFFFFFFF data size.
        let (dur, rate) = wav_spec_duration(&placeholder_wav(12_000, 24_000)).expect("duration");
        assert_eq!(rate, 24_000);
        assert!((dur - 0.5).abs() < 1e-3, "got {dur}");
    }

    #[test]
    fn wav_spec_duration_none_for_non_wav() {
        assert!(wav_spec_duration(b"not a wav at all").is_none());
        assert!(wav_spec_duration(&[]).is_none());
    }
}
