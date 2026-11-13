use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OnnxServiceError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("embedding error: {0}")]
    EmbeddingError(String),

    #[error("summarization error: {0}")]
    SummarizationError(String),

    #[error("mlm error: {0}")]
    MlmError(String),

    #[error("tts error: {0}")]
    TtsError(String),
}

impl IntoResponse for OnnxServiceError {
    fn into_response(self) -> Response {
        let (status, error_message, retryable) = match &self {
            OnnxServiceError::InvalidRequest(msg) => {
                log::warn!("Invalid request: {}", msg);
                (StatusCode::BAD_REQUEST, msg.clone(), false)
            }
            OnnxServiceError::Internal(msg) => {
                log::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal error".to_string(),
                    false,
                )
            }
            OnnxServiceError::EmbeddingError(msg) => {
                log::error!("Embedding error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Embedding service error".to_string(),
                    true,
                )
            }
            OnnxServiceError::SummarizationError(msg) => {
                log::error!("Summarization error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Summarization service error".to_string(),
                    true,
                )
            }
            OnnxServiceError::MlmError(msg) => {
                log::error!("MLM error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "MLM service error".to_string(),
                    true,
                )
            }
            OnnxServiceError::TtsError(msg) => {
                log::error!("TTS error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "TTS service error".to_string(),
                    true,
                )
            }
        };

        let body = json!({
            "error": error_message,
            "code": status.as_u16(),
            "retryable": retryable
        });

        (status, axum::Json(body)).into_response()
    }
}
