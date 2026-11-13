use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

/// Errors for the conduit-oximedia service.
#[derive(Error, Debug)]
pub enum OximediaError {
    #[error("database error: {0}")]
    Database(#[from] dapple_data::error::DataError),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("av error: {0}")]
    AvError(String),

    #[error("audio analysis error: {0}")]
    AudioAnalysisError(String),
}

impl IntoResponse for OximediaError {
    fn into_response(self) -> Response {
        // This service is cluster-internal (called by flow), so surface the real
        // cause in the body — the flow client copies it verbatim into the nushell
        // error, the only place the operator sees it.
        let (status, error_message, retryable) = match &self {
            OximediaError::Database(e) => {
                log::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                    false,
                )
            }
            OximediaError::NotFound(msg) => {
                log::warn!("Not found: {}", msg);
                (StatusCode::NOT_FOUND, msg.clone(), false)
            }
            OximediaError::InvalidRequest(msg) => {
                log::warn!("Invalid request: {}", msg);
                (StatusCode::BAD_REQUEST, msg.clone(), false)
            }
            OximediaError::Internal(msg) => {
                log::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal error".to_string(),
                    false,
                )
            }
            OximediaError::AvError(msg) => {
                log::error!("AV error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("AV service error: {msg}"),
                    true,
                )
            }
            OximediaError::AudioAnalysisError(msg) => {
                log::error!("Audio analysis error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Audio analysis error: {msg}"),
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
