use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConduitError {
    #[error("database error: {0}")]
    Database(#[from] dapple_data::error::DataError),

    #[error("template error: {0}")]
    Template(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("llm error: {0}")]
    LlmError(String),

    #[error("LLM rate limited: retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("LLM context too long: {tokens} tokens exceeds limit of {limit}")]
    ContextTooLong { tokens: u32, limit: u32 },

    #[error("No LLM backend configured for engine {maker_key}/{engine_key}")]
    NoBackendConfigured {
        maker_key: String,
        engine_key: String,
    },

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

    #[error("image error: {0}")]
    ImageError(String),

    #[error("video error: {0}")]
    VideoError(String),

    #[error("document render error: {0}")]
    DocumentRenderError(String),
}

impl IntoResponse for ConduitError {
    fn into_response(self) -> Response {
        let (status, error_message, retryable) = match &self {
            ConduitError::Database(e) => {
                log::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                    false,
                )
            }
            ConduitError::Template(msg) => {
                log::error!("Template error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Template rendering error".to_string(),
                    false,
                )
            }
            ConduitError::NotFound(msg) => {
                log::warn!("Not found: {}", msg);
                (StatusCode::NOT_FOUND, msg.clone(), false)
            }
            ConduitError::InvalidRequest(msg) => {
                log::warn!("Invalid request: {}", msg);
                (StatusCode::BAD_REQUEST, msg.clone(), false)
            }
            ConduitError::LlmError(msg) => {
                log::error!("LLM error: {}", msg);
                (
                    StatusCode::BAD_GATEWAY,
                    "LLM service error".to_string(),
                    true,
                )
            }
            ConduitError::RateLimited { retry_after_ms } => {
                log::warn!("LLM rate limited: retry after {}ms", retry_after_ms);
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("Rate limited, retry after {}ms", retry_after_ms),
                    true,
                )
            }
            ConduitError::ContextTooLong { tokens, limit } => {
                log::warn!(
                    "LLM context too long: {} tokens exceeds limit of {}",
                    tokens,
                    limit
                );
                (
                    StatusCode::BAD_REQUEST,
                    format!(
                        "Context too long: {} tokens exceeds limit of {}",
                        tokens, limit
                    ),
                    false,
                )
            }
            ConduitError::NoBackendConfigured {
                maker_key,
                engine_key,
            } => {
                log::error!("No LLM backend configured for {}/{}", maker_key, engine_key);
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("No backend available for {}/{}", maker_key, engine_key),
                    false,
                )
            }
            ConduitError::Internal(msg) => {
                log::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal error".to_string(),
                    false,
                )
            }
            ConduitError::EmbeddingError(msg) => {
                log::error!("Embedding error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Embedding service error".to_string(),
                    true,
                )
            }
            ConduitError::SummarizationError(msg) => {
                log::error!("Summarization error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Summarization service error".to_string(),
                    true,
                )
            }
            ConduitError::MlmError(msg) => {
                log::error!("MLM error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "MLM service error".to_string(),
                    true,
                )
            }
            ConduitError::TtsError(msg) => {
                log::error!("TTS error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "TTS service error".to_string(),
                    true,
                )
            }
            ConduitError::ImageError(msg) => {
                log::error!("Image error: {}", msg);
                // Conduit is a cluster-internal surface (called by flow/api, never
                // public), so surface the real cause in the body rather than a
                // generic string — the flow client copies this verbatim into the
                // nushell error, which is the only place the operator sees it.
                // Matches the SSE image path, which already returns `e.to_string()`.
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Image service error: {msg}"),
                    true,
                )
            }
            ConduitError::VideoError(msg) => {
                log::error!("Video error: {}", msg);
                // Same rationale as ImageError: internal surface, surface the real
                // cause. Video-provider failures (submit/poll/download) are
                // retryable transients.
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Video service error: {msg}"),
                    true,
                )
            }
            ConduitError::DocumentRenderError(msg) => {
                log::error!("Document render error: {}", msg);
                // Internal surface, surface the real cause (same rationale as
                // ImageError). Rendering is deterministic — retrying the same
                // markdown produces the same failure, so retryable: false.
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Document render error: {msg}"),
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
