use thiserror::Error;

#[derive(Debug, Error)]
pub enum LagoError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Lago API error ({status}): {body}")]
    ApiError { status: u16, body: String },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
