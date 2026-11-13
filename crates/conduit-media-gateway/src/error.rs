use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

/// Errors for the conduit-media-gateway service.
#[derive(Error, Debug)]
pub enum MediaGatewayError {
    #[error("database error: {0}")]
    Database(#[from] dapple_data::error::DataError),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// Missing/invalid service configuration (e.g. Twitch creds unset). Fail loud.
    #[error("configuration error: {0}")]
    Config(String),

    /// Twitch Helix API failure (metadata/validation).
    #[error("twitch error: {0}")]
    Twitch(String),

    /// A bundled media binary (`ffmpeg` / `yt-dlp`) failed.
    #[error("media tool error: {0}")]
    MediaTool(String),

    /// ASR / transcription failure.
    #[error("transcription error: {0}")]
    Transcribe(String),

    /// Teaching-moment detection failure.
    #[error("detection error: {0}")]
    Detect(String),

    /// Clip cutting / export failure.
    #[error("clip error: {0}")]
    Cut(String),

    /// Transcript context / windowing failure.
    #[error("context error: {0}")]
    Context(String),

    /// Recap-reel assembly (card render / concat) failure.
    #[error("assemble error: {0}")]
    Assemble(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for MediaGatewayError {
    fn into_response(self) -> Response {
        // This service is cluster-internal (called by flow), so surface the real
        // cause in the body — the flow client copies it verbatim into the nushell
        // error, the only place the operator sees it.
        let (status, error_message, retryable) = match &self {
            MediaGatewayError::Database(e) => {
                log::error!("Database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                    false,
                )
            }
            MediaGatewayError::NotFound(msg) => {
                log::warn!("Not found: {msg}");
                (StatusCode::NOT_FOUND, msg.clone(), false)
            }
            MediaGatewayError::InvalidRequest(msg) => {
                log::warn!("Invalid request: {msg}");
                (StatusCode::BAD_REQUEST, msg.clone(), false)
            }
            MediaGatewayError::Config(msg) => {
                log::error!("Configuration error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Media gateway misconfigured: {msg}"),
                    false,
                )
            }
            MediaGatewayError::Twitch(msg) => {
                log::error!("Twitch error: {msg}");
                (
                    StatusCode::BAD_GATEWAY,
                    format!("Twitch error: {msg}"),
                    true,
                )
            }
            MediaGatewayError::MediaTool(msg) => {
                log::error!("Media tool error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Media tool error: {msg}"),
                    true,
                )
            }
            MediaGatewayError::Transcribe(msg) => {
                log::error!("Transcription error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Transcription error: {msg}"),
                    true,
                )
            }
            MediaGatewayError::Detect(msg) => {
                log::error!("Detection error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Detection error: {msg}"),
                    true,
                )
            }
            MediaGatewayError::Cut(msg) => {
                log::error!("Clip error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Clip error: {msg}"),
                    true,
                )
            }
            MediaGatewayError::Context(msg) => {
                log::error!("Context error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Context error: {msg}"),
                    true,
                )
            }
            MediaGatewayError::Assemble(msg) => {
                log::error!("Assemble error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Assemble error: {msg}"),
                    true,
                )
            }
            MediaGatewayError::Internal(msg) => {
                log::error!("Internal error: {msg}");
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
