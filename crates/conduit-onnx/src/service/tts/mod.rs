mod stub;
mod vits;

pub use stub::StubTtsBackend;
pub use vits::VitsTtsBackend;

use crate::error::OnnxServiceError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Configuration parsed from TTS engine_body (fluent_message_engine_bodies)
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct TtsPromptConfig {
    /// Speaker ID for multi-speaker models (0-108 for VCTK)
    pub speaker_id: Option<i64>,
    /// Language code for multilingual models (e.g., "en", "zh")
    pub language: Option<String>,
    /// Speech rate multiplier (future use)
    pub speed: Option<f32>,
}

/// Request for text-to-speech synthesis
#[derive(Debug, Clone)]
pub struct SynthesizeRequest {
    /// The text to synthesize into speech
    pub text: String,
    /// Voice description (unused for VITS, kept for API compatibility)
    pub voice: Option<String>,
    /// Speaker ID for multi-speaker models (0-108 for VCTK)
    pub speaker_id: Option<i64>,
    /// Language code for multilingual models (e.g., "en", "zh")
    pub language: Option<String>,
}

impl SynthesizeRequest {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            voice: None,
            speaker_id: None,
            language: None,
        }
    }

    pub fn with_voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = Some(voice.into());
        self
    }

    pub fn with_speaker_id(mut self, speaker_id: i64) -> Self {
        self.speaker_id = Some(speaker_id);
        self
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }
}

/// Result of text-to-speech synthesis
#[derive(Debug, Clone)]
pub struct SynthesizeResult {
    /// WAV audio bytes
    pub audio_bytes: Vec<u8>,
    /// Sample rate of the audio (22050 Hz for most VITS models)
    pub sample_rate: u32,
    /// Duration of the audio in seconds
    pub duration_seconds: f32,
    /// The text that was synthesized
    pub text: String,
}

impl SynthesizeResult {
    pub fn new(audio_bytes: Vec<u8>, sample_rate: u32, text: String) -> Self {
        // WAV has 44-byte header, each sample is 2 bytes (i16), mono
        let data_bytes = audio_bytes.len().saturating_sub(44);
        let num_samples = data_bytes / 2;
        let duration_seconds = num_samples as f32 / sample_rate as f32;

        Self {
            audio_bytes,
            sample_rate,
            duration_seconds,
            text,
        }
    }
}

/// Trait for TTS backends
#[async_trait]
pub trait TtsBackend: Send + Sync {
    /// Synthesize speech from text
    async fn synthesize(
        &self,
        request: &SynthesizeRequest,
    ) -> Result<SynthesizeResult, OnnxServiceError>;

    /// Get the backend name for logging
    fn name(&self) -> &str;

    /// Get the default sample rate for this backend
    fn sample_rate(&self) -> u32;

    /// Eagerly load heavy resources. Default is a no-op.
    async fn warmup(&self) -> Result<(), OnnxServiceError> {
        Ok(())
    }
}

/// Configuration for TTS service
#[derive(Debug, Clone)]
pub struct TtsConfig {
    /// VITS model name (e.g., "vits-ljspeech", "vits-vctk", "vits-melo-zh-en")
    pub model_id: String,
    /// Maximum input text length in characters
    pub max_text_length: usize,
    /// Speaker ID for multi-speaker models (e.g., 0-108 for VCTK)
    pub speaker_id: Option<i64>,
    /// Language code for validation (e.g., "en", "zh")
    pub language: Option<String>,
    /// Default voice description (unused for VITS, kept for API compatibility)
    pub default_voice: String,
    /// Whether to fail startup if model loading fails
    pub required: bool,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            // Default to LJSpeech VITS (English, single speaker, lightweight)
            model_id: "vits-ljspeech".to_string(),
            max_text_length: 500,
            speaker_id: None,
            language: None,
            default_voice: String::new(),
            required: false,
        }
    }
}

impl TtsConfig {
    /// Load configuration from environment variables
    ///
    /// Environment variables:
    /// - `TTS_ENABLED` - Set to "true" or "1" to enable TTS
    /// - `TTS_MODEL` - VITS model name (default: "vits-ljspeech")
    /// - `TTS_SPEAKER_ID` - Speaker ID for multi-speaker models (e.g., 0-108 for VCTK)
    /// - `TTS_LANGUAGE` - Language code for validation (e.g., "en", "zh")
    /// - `TTS_MAX_TEXT_LENGTH` - Maximum input text length (default: 500)
    /// - `TTS_REQUIRED` - Fail startup if TTS initialization fails
    pub fn from_env() -> Option<Self> {
        // Only create config if explicitly enabled or model is set
        let model_id = std::env::var("TTS_MODEL").ok();
        let enabled = std::env::var("TTS_ENABLED")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        if !enabled && model_id.is_none() {
            return None;
        }

        Some(Self {
            model_id: model_id.unwrap_or_else(|| "vits-ljspeech".to_string()),
            max_text_length: std::env::var("TTS_MAX_TEXT_LENGTH")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500),
            speaker_id: std::env::var("TTS_SPEAKER_ID")
                .ok()
                .and_then(|s| s.parse().ok()),
            language: std::env::var("TTS_LANGUAGE").ok(),
            default_voice: String::new(),
            required: std::env::var("TTS_REQUIRED")
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
        })
    }
}

/// TTS service for synthesizing speech from text
pub struct TtsService {
    backend: Arc<dyn TtsBackend>,
    config: TtsConfig,
}

impl TtsService {
    /// Create a new TTS service with the given backend
    pub fn new(backend: Arc<dyn TtsBackend>, config: TtsConfig) -> Self {
        Self { backend, config }
    }

    /// Create a TTS service with the VITS ONNX backend
    pub async fn with_vits(config: &TtsConfig) -> Result<Self, OnnxServiceError> {
        let backend = VitsTtsBackend::new(config).await?;
        Ok(Self {
            backend: Arc::new(backend),
            config: config.clone(),
        })
    }

    /// Create a TTS service with the stub backend (for testing)
    pub fn new_stub() -> Self {
        Self {
            backend: Arc::new(StubTtsBackend::new()),
            config: TtsConfig::default(),
        }
    }

    /// Synthesize speech from text
    pub async fn synthesize(&self, text: &str) -> Result<SynthesizeResult, OnnxServiceError> {
        self.synthesize_with_voice(text, None).await
    }

    /// Synthesize speech from text with a specific voice description
    pub async fn synthesize_with_voice(
        &self,
        text: &str,
        voice: Option<&str>,
    ) -> Result<SynthesizeResult, OnnxServiceError> {
        // Validate input length
        if text.len() > self.config.max_text_length {
            return Err(OnnxServiceError::InvalidRequest(format!(
                "Text too long: {} characters exceeds limit of {}",
                text.len(),
                self.config.max_text_length
            )));
        }

        if text.trim().is_empty() {
            return Err(OnnxServiceError::InvalidRequest(
                "Cannot synthesize empty text".to_string(),
            ));
        }

        let request = SynthesizeRequest {
            text: text.to_string(),
            voice: voice
                .map(|v| v.to_string())
                .or_else(|| Some(self.config.default_voice.clone())),
            speaker_id: None,
            language: None,
        };

        log::debug!(
            "Synthesizing {} characters using {} backend",
            text.len(),
            self.backend.name()
        );

        self.backend.synthesize(&request).await
    }

    /// Synthesize speech from text with config from prompt engine_body
    pub async fn synthesize_with_config(
        &self,
        text: &str,
        config: &TtsPromptConfig,
    ) -> Result<SynthesizeResult, OnnxServiceError> {
        // Validate input length
        if text.len() > self.config.max_text_length {
            return Err(OnnxServiceError::InvalidRequest(format!(
                "Text too long: {} characters exceeds limit of {}",
                text.len(),
                self.config.max_text_length
            )));
        }

        if text.trim().is_empty() {
            return Err(OnnxServiceError::InvalidRequest(
                "Cannot synthesize empty text".to_string(),
            ));
        }

        // Use config values or fall back to service defaults
        let request = SynthesizeRequest {
            text: text.to_string(),
            voice: None,
            speaker_id: config.speaker_id.or(self.config.speaker_id),
            language: config
                .language
                .clone()
                .or_else(|| self.config.language.clone()),
        };

        log::debug!(
            "Synthesizing {} characters using {} backend (speaker_id={:?}, language={:?})",
            text.len(),
            self.backend.name(),
            request.speaker_id,
            request.language
        );

        self.backend.synthesize(&request).await
    }

    /// Get the default sample rate
    pub fn sample_rate(&self) -> u32 {
        self.backend.sample_rate()
    }

    /// Get the backend name
    pub fn backend_name(&self) -> &str {
        self.backend.name()
    }

    /// Get the configuration
    pub fn config(&self) -> &TtsConfig {
        &self.config
    }

    /// Eagerly load the model so errors surface at startup.
    pub async fn warmup(&self) -> Result<(), OnnxServiceError> {
        self.backend.warmup().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthesize_request() {
        let req = SynthesizeRequest::new("Hello world").with_voice("A calm speaker");
        assert_eq!(req.text, "Hello world");
        assert_eq!(req.voice, Some("A calm speaker".to_string()));
    }

    #[test]
    fn test_synthesize_result_duration() {
        // Create a minimal WAV header (44 bytes) + 48000 samples (2 bytes each)
        // At 24000 Hz, 48000 samples = 2 seconds
        let mut wav_bytes = vec![0u8; 44]; // header
        wav_bytes.extend(vec![0u8; 48000 * 2]); // 48000 samples * 2 bytes

        let result = SynthesizeResult::new(wav_bytes, 24000, "test".to_string());
        assert!((result.duration_seconds - 2.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_stub_service() {
        let service = TtsService::new_stub();
        let result = service.synthesize("Hello world").await.unwrap();
        assert!(!result.audio_bytes.is_empty());
        assert_eq!(result.sample_rate, 24000);
    }

    #[tokio::test]
    async fn test_empty_text_error() {
        let service = TtsService::new_stub();
        let result = service.synthesize("").await;
        assert!(result.is_err());
    }
}
