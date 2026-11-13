//! Request/response schemas for `POST /api/get-service-token` on the
//! external surface.
//!
//! Distinct from the internal surface's schemas: external requests do NOT
//! carry an `auth_user_id` (no delegation) and the response payload includes
//! `company_slug` so customer SDKs can sanity-check which company the JWT
//! is bound to.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request body for `POST /api/get-service-token` on the external surface.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetExternalServiceTokenRequest {
    /// Base64-encoded 128-byte signed token. Format identical to internal
    /// service tokens (see `docs/books/_foundation/authentication/service-tokens.md`):
    /// signature (64) || version (1) || valid_from (8) || valid_until (8) ||
    /// service_id_len (1) || service_id (≤32) || padding.
    pub token: String,

    /// Service identifier registered in `rev.external_service_tokens`.
    /// Must match the service_id embedded in the signed token.
    pub service_id: String,
}

/// Successful response from `POST /api/get-service-token` on the external
/// surface.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetExternalServiceTokenResponse {
    /// Status indicator, always "ok" for successful responses.
    pub status: String,

    /// The issued JWT for external API access.
    pub jwt: String,

    /// The service identity encoded in the JWT.
    pub service_id: String,

    /// The slug of the company that owns this token. Included for client-side
    /// sanity checks; the JWT itself carries the same value as a claim.
    pub company_slug: String,

    /// When the JWT expires (RFC 3339 / ISO 8601).
    pub expires_at: String,
}

/// Error response from `POST /api/get-service-token` on the external surface.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ExternalServiceTokenErrorResponse {
    /// Error code (e.g., "MALFORMED_TOKEN", "INVALID_SIGNATURE",
    /// "COMPANY_REVOKED", "SERVICE_TOKEN_REVOKED").
    pub error: String,

    /// Human-readable error message.
    pub message: String,
}

impl ExternalServiceTokenErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}
