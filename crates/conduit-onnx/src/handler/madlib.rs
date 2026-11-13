//! Handler for the /madlib endpoint

use crate::error::OnnxServiceError;
use crate::service::AppState;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MadlibRequest {
    pub text: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    1
}

#[derive(Debug, Serialize)]
pub struct TokenPredictionResponse {
    pub token: String,
    pub score: f32,
}

#[derive(Debug, Serialize)]
pub struct MaskPredictionResponse {
    pub position: usize,
    pub candidates: Vec<TokenPredictionResponse>,
}

#[derive(Debug, Serialize)]
pub struct MadlibResponse {
    pub filled_text: String,
    pub predictions: Vec<MaskPredictionResponse>,
}

pub async fn handle_madlib(
    State(state): State<AppState>,
    Json(req): Json<MadlibRequest>,
) -> Result<Json<MadlibResponse>, OnnxServiceError> {
    log::debug!(
        "Madlib request: text_len={}, top_k={}",
        req.text.len(),
        req.top_k
    );

    let svc = state.mlm_service.as_ref().ok_or_else(|| {
        OnnxServiceError::MlmError("MLM service not enabled. Set MLM_ENABLED=true".to_string())
    })?;

    if req.text.is_empty() {
        return Err(OnnxServiceError::InvalidRequest(
            "Text cannot be empty".to_string(),
        ));
    }

    if req.top_k == 0 {
        return Err(OnnxServiceError::InvalidRequest(
            "top_k must be at least 1".to_string(),
        ));
    }

    let result = svc.fill_masks(&req.text, req.top_k).await?;

    let predictions = result
        .predictions
        .into_iter()
        .map(|p| MaskPredictionResponse {
            position: p.position,
            candidates: p
                .candidates
                .into_iter()
                .map(|c| TokenPredictionResponse {
                    token: c.token,
                    score: c.score,
                })
                .collect(),
        })
        .collect();

    Ok(Json(MadlibResponse {
        filled_text: result.filled_text,
        predictions,
    }))
}
