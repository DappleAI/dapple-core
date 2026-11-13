//! Models for `rev.external_service_tokens` and revocation state.
//!
//! Customer-tier service tokens are issued via the admin endpoint
//! `POST /api/companies/{id}/external-service-tokens` and consumed by the
//! external API surface during JWT exchange. The lookup flow needs the
//! token's public key, the owning company's slug, and a check that neither
//! the token nor the company is revoked.
//!
//! See `docs/plans/external-api/00-architecture.md` §3 Decision 2 and
//! `docs/books/_foundation/authentication/external-service-tokens.md`.

use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Internal model for `rev.external_service_tokens`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExternalServiceTokenModel {
    pub id: Uuid,
    pub service_id: String,
    pub company_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub public_key_pem: String,
    pub key_algorithm: String,
    pub created_at: OffsetDateTime,
    pub created_by_user_id: Option<i64>,
}

/// Row returned by `select_external_service_tokens_by_company_id` and
/// `select_external_service_token_by_id`: token metadata joined with the
/// auto-created `external_service_principals.principal_id` and the
/// earliest revocation timestamp (if any).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExternalServiceTokenWithRevocationModel {
    pub id: Uuid,
    pub service_id: String,
    pub company_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
    pub principal_id: Uuid,
}

/// Token + owning company + revocation status + the auto-created
/// external-service principal, joined into a single row for the external
/// auth flow. Saves per-request round-trips: the extractor only sees the
/// JWT (which we mint with everything it needs), and the JWT exchange
/// handler runs this lookup once.
///
/// `principal_id` is the `rev.principals.id` of the external_service
/// principal auto-created by the Migration 37 trigger on
/// `rev.external_service_tokens` insert. The JWT carries this UUID; the
/// external handlers stamp it directly onto journeys/dispatches as
/// `principal_id` (no per-request user_details resolution needed).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExternalServiceTokenWithCompanyModel {
    pub id: Uuid,
    pub service_id: String,
    pub public_key_pem: String,
    pub key_algorithm: String,
    pub company_id: Uuid,
    pub company_slug: String,
    pub company_revoked_at: Option<OffsetDateTime>,
    pub token_revoked_at: Option<OffsetDateTime>,
    pub principal_id: Uuid,
}

/// Row returned by the revoke INSERT.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedExternalServiceTokenRevocationModel {
    pub id: Uuid,
    pub external_service_token_id: Uuid,
    pub revoked_at: OffsetDateTime,
    pub revoked_by_user_id: Option<i64>,
    pub reason: Option<String>,
}
