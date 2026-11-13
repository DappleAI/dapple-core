//! Request and response schemas for the service token endpoint.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request body for `POST /api/get-service-token`
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetServiceTokenRequest {
    /// Base64-encoded 128-byte signed token (without dps_ prefix).
    /// Format: signature (64 bytes) || message (64 bytes)
    /// Message: version (1) + valid_from (8) + valid_until (8) + service_id_len (1) + service_id (<=32) + padding
    pub token: String,

    /// Service identifier (e.g., "conduit_main", "flow_worker").
    /// Must match the service_id embedded in the signed token.
    pub service_id: String,

    /// Optional auth.users.id for creating delegated tokens.
    /// When provided, the returned JWT will be a "delegated" token with
    /// sub = { type: "auth_user", id: auth_user_id } instead of a "service" token.
    /// This allows services to make API calls on behalf of authenticated users.
    #[serde(default)]
    pub auth_user_id: Option<i64>,
}

/// Successful response from `POST /api/get-service-token`
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetServiceTokenResponse {
    /// Status indicator, always "ok" for successful responses
    pub status: String,

    /// The issued JWT for API access
    pub jwt: String,

    /// The service identity encoded in the JWT
    pub service_id: String,

    /// When the JWT expires (ISO 8601 format)
    pub expires_at: String,
}

/// Error response from `POST /api/get-service-token`
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ServiceTokenErrorResponse {
    /// Error code (e.g., "MALFORMED_TOKEN", "INVALID_SIGNATURE")
    pub error: String,

    /// Human-readable error message
    pub message: String,
}

impl ServiceTokenErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}
