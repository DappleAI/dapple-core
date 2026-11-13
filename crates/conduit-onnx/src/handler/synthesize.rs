//! Handler for the /synthesize endpoint

use crate::error::OnnxServiceError;
use crate::service::AppState;
use axum::{extract::State, Json};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SynthesizeRequest {
    pub text: String,
    #[serde(default)]
    pub voice: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SynthesizeResponse {
    pub audio_bytes: String,
    pub duration_seconds: f32,
    pub sample_rate: u32,
    pub content_length: i64,
    pub text: String,
}

pub async fn handle_synthesize(
    State(state): State<AppState>,
    Json(req): Json<SynthesizeRequest>,
) -> Result<Json<SynthesizeResponse>, OnnxServiceError> {
    log::debug!(
        "Synthesize request: text_len={}, voice={:?}",
        req.text.len(),
        req.voice,
    );

    let tts = state.tts_service.as_ref().ok_or_else(|| {
        OnnxServiceError::TtsError("TTS service not enabled. Set TTS_ENABLED=true".to_string())
    })?;

    if req.text.is_empty() {
        return Err(OnnxServiceError::InvalidRequest(
            "Text cannot be empty".to_string(),
        ));
    }

    let result = tts
        .synthesize_with_voice(&req.text, req.voice.as_deref())
        .await?;

    let content_length = result.audio_bytes.len() as i64;
    let audio_bytes_b64 = BASE64.encode(&result.audio_bytes);

    log::info!(
        "Synthesized {} chars -> {} seconds audio ({} bytes)",
        req.text.len(),
        result.duration_seconds,
        content_length
    );

    Ok(Json(SynthesizeResponse {
        audio_bytes: audio_bytes_b64,
        duration_seconds: result.duration_seconds,
        sample_rate: result.sample_rate,
        content_length,
        text: result.text,
    }))
}
