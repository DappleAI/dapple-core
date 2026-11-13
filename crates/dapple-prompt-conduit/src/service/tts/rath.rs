//! OpenAI TTS backend via rath-rs (generation only).
//!
//! Reaches OpenAI's `/audio/speech` through rath's provider-agnostic
//! `TtsClient`. This is the conduit's only TTS provider — rath has no Fal TTS,
//! and this replaces the removed VITS/ONNX path. The model slug, voice, format,
//! and any model-specific knobs (e.g. gpt-4o-mini-tts `instructions`, `speed`)
//! are authored in the guide's `:::inference-profile` engine_body
//! (`provider`/`model`/`voice`/`format`/`provider_config` — see `handler/tts.rs`).
//!
//! rath's `TtsResponse` returns raw audio bytes (`data: Vec<u8>`) plus a
//! `mime_type`, so — unlike the image/video URL-or-base64 results — there is no
//! resolver step.

use super::{TtsBackend, TtsGenRequest, TtsGenResult};
use crate::error::ConduitError;
use crate::service::rath_util;
use async_trait::async_trait;
use rath::audio::tts::{TtsOptions, TtsRequest};
use serde_json::{Map, Value};

/// rath-backed OpenAI TTS backend.
pub struct RathTtsBackend;

impl RathTtsBackend {
    pub fn new() -> Self {
        Self
    }

    /// Build from the environment. `None` unless `OPENAI_API_KEY` is set — the
    /// same key rath resolves via `api_key_env`, so registering without it would
    /// only fail at synth time.
    pub fn from_env() -> Option<Self> {
        rath_util::openai_key_present().then(Self::new)
    }

    /// Assemble the request `provider_config`: a conduit-derived `speed` default
    /// with the author's engine_body `provider_config` merged on top (author wins
    /// — they know the model's exact schema, e.g. gpt-4o-mini-tts `instructions`).
    fn provider_config(request: &TtsGenRequest) -> Option<Value> {
        let mut cfg = Map::new();
        if let Some(speed) = request.speed.filter(|s| *s > 0.0) {
            cfg.insert("speed".to_string(), Value::from(speed));
        }
        if let Some(Value::Object(author)) = &request.provider_config {
            for (k, v) in author {
                cfg.insert(k.clone(), v.clone());
            }
        }
        (!cfg.is_empty()).then_some(Value::Object(cfg))
    }
}

impl Default for RathTtsBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TtsBackend for RathTtsBackend {
    async fn synthesize(&self, request: &TtsGenRequest) -> Result<TtsGenResult, ConduitError> {
        let url = rath_util::openai_model_url(&request.model);
        let req = TtsRequest {
            input: request.input.clone(),
            voice: request.voice.clone(),
            // The model is carried in the URL path; leave `model` None so we
            // don't double-specify it.
            model: None,
            format: request.format.clone(),
            provider_config: Self::provider_config(request),
        };
        let client = TtsOptions {
            provider_config: None,
        }
        .create(&url)
        .map_err(rath_util::tts_err)?;
        let response = client
            .synthesize_speech(&req)
            .await
            .map_err(rath_util::tts_err)?;
        if response.data.is_empty() {
            return Err(ConduitError::TtsError(
                "openai tts returned 0 bytes".to_string(),
            ));
        }
        // Derive duration/rate from the WAV header (0.0/0 for non-wav formats).
        // Powers the handler's runaway-return guard and fills the asset event's
        // previously-always-zero duration.
        let (duration_seconds, sample_rate) =
            super::wav_spec_duration(&response.data).unwrap_or((0.0, 0));
        Ok(TtsGenResult {
            bytes: response.data,
            mime_type: response.mime_type,
            duration_seconds,
            sample_rate,
        })
    }

    /// Routes the OpenAI provider — and the default (no provider), since OpenAI
    /// is the only TTS provider. The stub is the unconditional fallback.
    fn supports(&self, request: &TtsGenRequest) -> bool {
        matches!(
            request.provider.as_deref(),
            None | Some("") | Some("openai") | Some("rath")
        )
    }

    fn name(&self) -> &str {
        "openai-tts"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(provider: Option<&str>) -> TtsGenRequest {
        TtsGenRequest {
            provider: provider.map(String::from),
            input: "Hello from the narrator.".to_string(),
            model: "gpt-4o-mini-tts".to_string(),
            voice: Some("alloy".to_string()),
            format: Some("mp3".to_string()),
            speed: None,
            provider_config: None,
        }
    }

    #[test]
    fn supports_openai_and_default_not_other() {
        let b = RathTtsBackend::new();
        assert!(b.supports(&req(None)));
        assert!(b.supports(&req(Some(""))));
        assert!(b.supports(&req(Some("openai"))));
        assert!(b.supports(&req(Some("rath"))));
        assert!(!b.supports(&req(Some("fal"))));
    }

    #[test]
    fn provider_config_speed_and_author_override() {
        let mut r = req(Some("openai"));
        r.speed = Some(1.25);
        r.provider_config = Some(json!({ "instructions": "cheerful", "speed": 0.9 }));
        let cfg = RathTtsBackend::provider_config(&r).unwrap();
        assert_eq!(cfg["instructions"], json!("cheerful"));
        assert_eq!(cfg["speed"], json!(0.9), "author speed wins");
    }

    #[test]
    fn provider_config_none_when_empty() {
        let r = req(Some("openai"));
        assert!(RathTtsBackend::provider_config(&r).is_none());
    }

    /// Live smoke test: hits OpenAI's `/audio/speech` through rath and asserts
    /// non-empty audio bytes. `#[ignore]` by default (needs `OPENAI_API_KEY` +
    /// network). Run with:
    ///   `cargo test -p dapple-prompt-conduit -- --ignored openai_tts_live`
    #[tokio::test]
    #[ignore = "requires OPENAI_API_KEY and network"]
    async fn openai_tts_live() {
        let backend = RathTtsBackend::from_env()
            .expect("OPENAI_API_KEY must be set to run the live TTS smoke test");
        let out = backend
            .synthesize(&req(Some("openai")))
            .await
            .expect("openai tts synthesis failed");
        assert!(!out.bytes.is_empty(), "expected non-empty audio bytes");
        assert!(
            out.mime_type.starts_with("audio/"),
            "expected an audio/* mime, got {}",
            out.mime_type
        );
    }
}
