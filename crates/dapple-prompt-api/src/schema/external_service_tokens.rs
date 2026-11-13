//! Request and response schemas for the admin
//! `/api/companies/{id}/external-service-tokens` and
//! `/api/external-service-tokens/{id}` endpoints.
//!
//! IMPORTANT: the create handler never receives the private key. Clients
//! generate the keypair locally and POST only the public PEM; the
//! response carries the SHA-256 of the registered PEM so the CLI can
//! confirm the bytes the server stored match what was generated.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `POST /api/companies/{id}/external-service-tokens/` body.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateExternalServiceTokenSchema {
    /// Globally-unique service identifier (`^[a-z][a-z0-9_]{2,31}$`).
    pub service_id: String,
    /// Human-readable token name.
    pub name: String,
    /// Optional free-form description.
    pub description: Option<String>,
    /// PEM-encoded Ed25519 public key. The handler validates the PEM
    /// via `parse_ed25519_public_key`; bad PEM → HTTP 400.
    pub public_key_pem: String,
}

/// `POST /api/companies/{id}/external-service-tokens/` response.
/// Top-level fields (no `obj` wrapper) to mirror CreateApiTokenResponse.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateExternalServiceTokenResponse {
    pub status: String,
    pub id: Uuid,
    pub service_id: String,
    pub name: String,
    pub principal_id: Uuid,
    /// Hex SHA-256 of the registered `public_key_pem` (after Unicode-normalised
    /// CRLF→LF; same bytes the DB stored). Lets the CLI verify the server
    /// recorded what it intended to register.
    pub public_key_pem_sha256: String,
    pub created_at: String,
}

/// Element used inside index / detail responses.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct ExternalServiceTokenView {
    pub id: Uuid,
    pub service_id: String,
    pub name: String,
    pub principal_id: Uuid,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

/// `GET /api/companies/{id}/external-service-tokens/` response.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ListExternalServiceTokensResponse {
    pub status: String,
    pub results: usize,
    pub objs: Vec<ExternalServiceTokenView>,
}

/// `DELETE /api/external-service-tokens/{id}` body.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct RevokeExternalServiceTokenSchema {
    pub reason: Option<String>,
}

/// `DELETE /api/external-service-tokens/{id}` response.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct RevokeExternalServiceTokenResponse {
    pub status: String,
    pub message: String,
}
