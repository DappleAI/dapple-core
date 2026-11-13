//! Request and response schemas for API tokens.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Request body for `POST /api/api-tokens/`.
/// Creates a new API token for the authenticated user.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateApiTokenSchema {
    /// Human-readable name for the token (e.g., "CLI tool", "CI/CD")
    pub name: String,

    /// Optional expiration time in seconds from now.
    /// If not provided, the token never expires.
    pub expires_in_secs: Option<u64>,
}

/// Response for `POST /api/api-tokens/`.
/// Contains the plaintext token (shown only once).
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateApiTokenResponse {
    pub status: String,
    pub id: Uuid,
    pub name: String,
    /// The plaintext API token. Store this securely - it cannot be retrieved again.
    pub token: String,
    /// First 8 characters of the token for identification.
    pub token_prefix: String,
    /// When the token expires (ISO 8601), or null if it never expires.
    pub expires_at: Option<String>,
    /// When the token was created (ISO 8601).
    pub created_at: String,
}

/// Response for `GET /api/api-tokens/`.
/// Lists all tokens for the authenticated user.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ListApiTokensResponse {
    pub status: String,
    pub results: usize,
    pub tokens: Vec<ApiTokenInfo>,
}

/// Token metadata (without the secret).
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ApiTokenInfo {
    pub id: Uuid,
    pub name: String,
    /// First 8 characters of the token for identification.
    pub token_prefix: String,
    /// When the token expires (ISO 8601), or null if it never expires.
    pub expires_at: Option<String>,
    /// When the token was created (ISO 8601).
    pub created_at: String,
    /// When the token was revoked (ISO 8601), or null if still active.
    pub revoked_at: Option<String>,
}

/// Request body for `DELETE /api/api-tokens/:object_id`.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct RevokeApiTokenSchema {
    /// Optional reason for revocation (for audit purposes).
    pub reason: Option<String>,
}

/// Response for `DELETE /api/api-tokens/:object_id`.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct RevokeApiTokenResponse {
    pub status: String,
    pub message: String,
}

/// Error response for API token operations.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ApiTokenErrorResponse {
    pub error: String,
    pub message: String,
}

impl ApiTokenErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}
