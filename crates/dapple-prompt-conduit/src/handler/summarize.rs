//! Handler for the /summarize endpoint

use crate::error::ConduitError;
use crate::service::AppState;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

/// Request to summarize text
#[derive(Debug, Deserialize)]
pub struct SummarizeRequest {
    /// The text to summarize
    pub text: String,
    /// Maximum output length in tokens (default: 150)
    #[serde(default)]
    pub max_length: Option<usize>,
    /// Minimum output length in tokens (default: 20)
    #[serde(default)]
    pub min_length: Option<usize>,
    /// Direct prompt prefix for T5 (e.g., "summarize:", "compress:")
    /// The prefix is resolved in dapple-prompt-flow from prompt_data metadata
    #[serde(default)]
    pub prompt_prefix: Option<String>,
}

/// Response from summarization
#[derive(Debug, Serialize)]
pub struct SummarizeResponse {
    /// The generated summary
    pub summary: String,
    /// Length of input text in characters
    pub input_length: usize,
    /// Length of summary in characters
    pub output_length: usize,
}

/// Handle a summarization request
pub async fn handle_summarize(
    State(state): State<AppState>,
    Json(req): Json<SummarizeRequest>,
) -> Result<Json<SummarizeResponse>, ConduitError> {
    log::debug!(
        "Summarize request: text_len={}, max_length={:?}, min_length={:?}, prompt_prefix={:?}",
        req.text.len(),
        req.max_length,
        req.min_length,
        req.prompt_prefix
    );

    if req.text.is_empty() {
        return Err(ConduitError::InvalidRequest(
            "Text cannot be empty".to_string(),
        ));
    }

    // Prefer onnx_client (sub-service) over local summarization
    if let Some(ref client) = state.onnx_client {
        let resp = client
            .summarize(crate::service::onnx_client::SummarizeRequest {
                text: req.text.clone(),
                max_length: req.max_length,
                min_length: req.min_length,
                prompt_prefix: req.prompt_prefix.clone(),
            })
            .await?;

        return Ok(Json(SummarizeResponse {
            summary: resp.summary,
            input_length: resp.input_length,
            output_length: resp.output_length,
        }));
    }

    // Fall back to local summarization service
    let svc = state.summarization_service.as_ref().ok_or_else(|| {
        ConduitError::SummarizationError(
            "Summarization service not enabled. Set SUMMARIZATION_ENABLED=true".to_string(),
        )
    })?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_deserialization() {
        let json = r#"{"text": "Hello world"}"#;
        let req: SummarizeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "Hello world");
        assert!(req.max_length.is_none());
        assert!(req.min_length.is_none());
        assert!(req.prompt_prefix.is_none());
    }

    #[test]
    fn test_request_with_options() {
        let json = r#"{"text": "Hello world", "max_length": 100, "min_length": 20}"#;
        let req: SummarizeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "Hello world");
        assert_eq!(req.max_length, Some(100));
        assert_eq!(req.min_length, Some(20));
        assert!(req.prompt_prefix.is_none());
    }

    #[test]
    fn test_request_with_prompt_prefix() {
        let json = r#"{"text": "Hello world", "prompt_prefix": "compress:"}"#;
        let req: SummarizeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "Hello world");
        assert_eq!(req.prompt_prefix, Some("compress:".to_string()));
    }

    #[test]
    fn test_request_without_prompt() {
        let json = r#"{"text": "Hello world"}"#;
        let req: SummarizeRequest = serde_json::from_str(json).unwrap();
        assert!(req.prompt_prefix.is_none());
    }

    #[test]
    fn test_response_serialization() {
        let response = SummarizeResponse {
            summary: "Summary text".to_string(),
            input_length: 100,
            output_length: 12,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("summary"));
        assert!(json.contains("input_length"));
        assert!(json.contains("output_length"));
    }
}
