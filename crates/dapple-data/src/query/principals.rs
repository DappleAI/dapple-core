//! Queries against the principals model (Mig 37).
//!
//! Three flavors of lookup that handlers reach for at journey/dispatch
//! create time:
//!
//! - `select_human_principal_for_auth_user` — internal handlers translate
//!   the NextAuth `auth_user_id` from the JWT claim into the corresponding
//!   `human_principal.principal_id`.
//! - `select_external_service_principal_for_token` — the external token
//!   exchange handler resolves the external-service-token row to the
//!   `external_service_principal.principal_id` auto-created by the
//!   trigger on `rev.external_service_tokens` insert.
//! - `resolve_billable_entity` — implements the billing-resolution chain
//!   (subject's PBE → subject's CBE → principal's PBE → principal's CBE).
//!
//! See `docs/plans/principals-and-billing/00-architecture.md` §5.

use crate::DbPool;
use crate::error::DataError;
use sqlx::types::Uuid;

/// Resolve the `human_principal.principal_id` for a given NextAuth user.
/// Returns `Ok(None)` only if the user has no `human_principal` row, which
/// shouldn't happen after Mig 37's backfill but is treated as a soft error
/// by callers (404 / signal-up-the-stack).
pub async fn select_human_principal_for_auth_user(
    pool: &DbPool,
    auth_user_id: i64,
) -> Result<Option<Uuid>, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/principals/select_human_principal_for_auth_user.sql",
        auth_user_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;
    Ok(row)
}

/// Resolve the `human_principal.principal_id` for a given user_details
/// `object_id` (UUID). The internal API extractor surfaces a UUID, not the
/// BIGINT auth_user_id, so handlers in dapple-prompt-api reach for this
/// variant.
pub async fn select_human_principal_for_user_object_id(
    pool: &DbPool,
    user_object_id: Uuid,
) -> Result<Option<Uuid>, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/principals/select_human_principal_for_user_object_id.sql",
        user_object_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;
    Ok(row)
}

/// Resolve the billing target for a journey or dispatch about to be
/// created. Stamped into the row at create-time and never recomputed
/// (matches the auditor book's "no modifications" rule).
///
/// `subject_principal_id` is optional — pass `None` when the principal
/// acts on its own (Shape A or Shape C). When `Some`, the resolution
/// prefers the subject's billing target over the actor's.
///
/// Returns `Ok(None)` if no link is found anywhere in the chain. V1
/// accepts this (many users have no `billable_entity` yet); the handler
/// stamps the NULL into the journey row.
pub async fn resolve_billable_entity(
    pool: &DbPool,
    principal_id: Uuid,
    subject_principal_id: Option<Uuid>,
) -> Result<Option<Uuid>, DataError> {
    // The .sqlc binds `subject_principal_id` as a non-null UUID; passing
    // `Uuid::nil()` for "no subject" lets us reuse the same query path and
    // the SQL's COALESCE-with-LIMIT shape handles the no-match case
    // naturally (subject_pbe / subject_cbe return zero rows on nil).
    let subject = subject_principal_id.unwrap_or(Uuid::nil());
    let row = sqlx::query_file_scalar!(
        "assets/.sql/principals/resolve_billable_entity.sql",
        principal_id,
        subject,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row)
}
