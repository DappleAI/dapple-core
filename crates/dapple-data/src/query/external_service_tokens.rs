//! Queries for `rev.external_service_tokens`.
//!
//! Used by:
//! * the external API surface (single lookup by service_id during JWT
//!   exchange — see `select_external_service_token_by_service_id`)
//! * the internal admin REST endpoints
//!   (`POST /api/companies/{id}/external-service-tokens`,
//!   `GET /api/companies/{id}/external-service-tokens`,
//!   `DELETE /api/external-service-tokens/{id}`).

use crate::DbPool;
use crate::error::DataError;
use crate::model::external_service_tokens::{
    ExternalServiceTokenModel, ExternalServiceTokenWithCompanyModel,
    ExternalServiceTokenWithRevocationModel, InsertedExternalServiceTokenRevocationModel,
};
use sqlx::types::Uuid;

/// Register a new external service token for `company_id`. Caller MUST
/// validate the PEM before invoking; this function only writes. The
/// Migration 37 AFTER-INSERT trigger auto-creates the matching
/// `external_service_principals` row.
pub async fn insert_external_service_token(
    pool: &DbPool,
    company_id: Uuid,
    service_id: &str,
    name: &str,
    description: Option<&str>,
    public_key_pem: &str,
) -> Result<ExternalServiceTokenModel, DataError> {
    let row = sqlx::query_file_as!(
        ExternalServiceTokenModel,
        "assets/.sql/external_service_tokens/insert_external_service_token.sql",
        // Alphabetical bind order: company_id, description, name, public_key_pem, service_id
        company_id,
        description,
        name,
        public_key_pem,
        service_id,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(row)
}

/// List all external service tokens for a company. Used by
/// `GET /api/companies/{id}/external-service-tokens` and by the company
/// detail endpoint.
pub async fn select_external_service_tokens_by_company_id(
    pool: &DbPool,
    company_id: Uuid,
) -> Result<Vec<ExternalServiceTokenWithRevocationModel>, DataError> {
    let rows = sqlx::query_file_as!(
        ExternalServiceTokenWithRevocationModel,
        "assets/.sql/external_service_tokens/select_external_service_tokens_by_company_id.sql",
        company_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Look up a single external service token by id.
pub async fn select_external_service_token_by_id(
    pool: &DbPool,
    id: Uuid,
) -> Result<Option<ExternalServiceTokenWithRevocationModel>, DataError> {
    let row = sqlx::query_file_as!(
        ExternalServiceTokenWithRevocationModel,
        "assets/.sql/external_service_tokens/select_external_service_token_by_id.sql",
        id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Look up an external service token by its `service_id`, joined with its
/// owning company and revocation state. Returns `None` if no token with
/// that service_id exists.
///
/// Callers must check both `token_revoked_at` and `company_revoked_at`
/// before treating the token as live.
pub async fn select_external_service_token_by_service_id(
    pool: &DbPool,
    service_id: &str,
) -> Result<Option<ExternalServiceTokenWithCompanyModel>, DataError> {
    let row = sqlx::query_file_as!(
        ExternalServiceTokenWithCompanyModel,
        "assets/.sql/external_service_tokens/select_external_service_token_by_service_id.sql",
        service_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Revoke an external service token (append-only). Returns `None` if the
/// token does not exist; handler maps that to HTTP 404. The query is a
/// raw INSERT, so revoking an already-revoked token simply appends another
/// row to the audit log — the external extractor stops at the first
/// matching row.
pub async fn revoke_external_service_token(
    pool: &DbPool,
    id: Uuid,
    reason: Option<String>,
) -> Result<Option<InsertedExternalServiceTokenRevocationModel>, DataError> {
    let row = sqlx::query_file_as!(
        InsertedExternalServiceTokenRevocationModel,
        "assets/.sql/external_service_tokens/revoke_external_service_token.sql",
        // Alphabetical bind order: id, reason
        id,
        reason,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
