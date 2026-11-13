use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LlmServiceError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("llm error: {0}")]
    LlmError(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for LlmServiceError {
    fn into_response(self) -> Response {
        let (status, error_message, retryable) = match &self {
            LlmServiceError::InvalidRequest(msg) => {
                log::warn!("Invalid request: {}", msg);
                (StatusCode::BAD_REQUEST, msg.clone(), false)
            }
            LlmServiceError::LlmError(msg) => {
                log::error!("LLM error: {}", msg);
                (
                    StatusCode::BAD_GATEWAY,
                    "LLM service error".to_string(),
                    true,
                )
            }
            LlmServiceError::Internal(msg) => {
                log::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal error".to_string(),
                    false,
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
