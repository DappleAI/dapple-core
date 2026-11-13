//! Request and response schemas for the auth token endpoint.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Request body for `POST /api/get-auth-token`
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetAuthTokenRequest {
    /// Base64-encoded 129-byte signed auth token.
    /// Format: signature (65 bytes) || message (64 bytes)
    /// Message: version (1) + valid_from (8) + valid_until (8) + padding (47)
    pub token: String,

    /// User UUID to include in the JWT `sub` claim.
    /// The web service provides this after authenticating the user.
    pub user_id: Uuid,
}

/// Successful response from `POST /api/get-auth-token`
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetAuthTokenResponse {
    /// Status indicator, always "ok" for successful responses
    pub status: String,

    /// The issued JWT for API access
    pub jwt: String,

    /// The recovered Ethereum address of the token signer (0x-prefixed)
    pub signer: String,

    /// When the JWT expires (ISO 8601 format)
    pub expires_at: String,
}

/// Error response from `POST /api/get-auth-token`
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct AuthTokenErrorResponse {
    /// Error code (e.g., "MALFORMED_TOKEN", "INVALID_SIGNATURE")
    pub error: String,

    /// Human-readable error message
    pub message: String,
}

impl AuthTokenErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}
