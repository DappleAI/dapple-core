//! Handler for the /summarize endpoint

use crate::error::OnnxServiceError;
use crate::service::AppState;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub min_length: Option<usize>,
    #[serde(default)]
    pub prompt_prefix: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SummarizeResponse {
    pub summary: String,
    pub input_length: usize,
    pub output_length: usize,
}

pub async fn handle_summarize(
    State(state): State<AppState>,
    Json(req): Json<SummarizeRequest>,
) -> Result<Json<SummarizeResponse>, OnnxServiceError> {
    log::debug!(
        "Summarize request: text_len={}, max_length={:?}, min_length={:?}, prompt_prefix={:?}",
        req.text.len(),
        req.max_length,
        req.min_length,
        req.prompt_prefix
    );

    let svc = state.summarization_service.as_ref().ok_or_else(|| {
        OnnxServiceError::SummarizationError(
            "Summarization service not enabled. Set SUMMARIZATION_ENABLED=true".to_string(),
        )
    })?;

    if req.text.is_empty() {
        return Err(OnnxServiceError::InvalidRequest(
            "Text cannot be empty".to_string(),
        ));
    }

    let input_length = req.text.len();
    let summary = svc
        .summarize_text_with_prefix(&req.text, req.prompt_prefix.as_deref())
        .await?;
    let output_length = summary.len();

    Ok(Json(SummarizeResponse {
        summary,
        input_length,
        output_length,
    }))
}
