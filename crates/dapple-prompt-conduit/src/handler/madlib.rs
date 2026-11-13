//! Handler for the /madlib endpoint

use crate::error::ConduitError;
use crate::service::AppState;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

/// Request to fill mask tokens in text
#[derive(Debug, Deserialize)]
pub struct MadlibRequest {
    /// The text containing [MASK] tokens to fill
    pub text: String,
    /// Number of top predictions to return per mask (default: 1)
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    1
}

/// A single token prediction with score
#[derive(Debug, Serialize)]
pub struct TokenPredictionResponse {
    /// The predicted token text
    pub token: String,
    /// The score (probability)
    pub score: f32,
}

/// Prediction for a single mask position
#[derive(Debug, Serialize)]
pub struct MaskPredictionResponse {
    /// Position of the mask in the original text (character offset)
    pub position: usize,
    /// Top-k candidate tokens for this mask
    pub candidates: Vec<TokenPredictionResponse>,
}

/// Response from mask filling
#[derive(Debug, Serialize)]
pub struct MadlibResponse {
    /// The text with masks filled using best predictions
    pub filled_text: String,
    /// Predictions for each mask position
    pub predictions: Vec<MaskPredictionResponse>,
}

/// Handle a madlib (mask filling) request
pub async fn handle_madlib(
    State(state): State<AppState>,
    Json(req): Json<MadlibRequest>,
) -> Result<Json<MadlibResponse>, ConduitError> {
    log::debug!(
        "Madlib request: text_len={}, top_k={}",
        req.text.len(),
        req.top_k
    );

    if req.text.is_empty() {
        return Err(ConduitError::InvalidRequest(
            "Text cannot be empty".to_string(),
        ));
    }

    if req.top_k == 0 {
        return Err(ConduitError::InvalidRequest(
            "top_k must be at least 1".to_string(),
        ));
    }

    // Prefer onnx_client (sub-service) over local MLM
    if let Some(ref client) = state.onnx_client {
        let resp = client
            .madlib(crate::service::onnx_client::MadlibRequest {
                text: req.text.clone(),
                top_k: Some(req.top_k),
            })
            .await?;

        let predictions = resp
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

        return Ok(Json(MadlibResponse {
            filled_text: resp.filled_text,
            predictions,
        }));
    }

    // Fall back to local MLM service
    let svc = state.mlm_service.as_ref().ok_or_else(|| {
        ConduitError::MlmError("MLM service not enabled. Set MLM_ENABLED=true".to_string())
    })?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_deserialization() {
        let json = r#"{"text": "Hello [MASK] world"}"#;
        let req: MadlibRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "Hello [MASK] world");
        assert_eq!(req.top_k, 1); // default
    }

    #[test]
    fn test_request_with_top_k() {
        let json = r#"{"text": "Hello [MASK] world", "top_k": 5}"#;
        let req: MadlibRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "Hello [MASK] world");
        assert_eq!(req.top_k, 5);
    }

    #[test]
    fn test_response_serialization() {
        let response = MadlibResponse {
            filled_text: "Hello beautiful world".to_string(),
            predictions: vec![MaskPredictionResponse {
                position: 6,
                candidates: vec![
                    TokenPredictionResponse {
                        token: "beautiful".to_string(),
                        score: 0.8,
                    },
                    TokenPredictionResponse {
                        token: "wonderful".to_string(),
                        score: 0.15,
                    },
                ],
            }],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("filled_text"));
        assert!(json.contains("predictions"));
        assert!(json.contains("candidates"));
        assert!(json.contains("beautiful"));
    }
}
