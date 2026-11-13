//! Queries for `rev.companies`.
//!
//! The admin REST endpoints (`POST /api/companies`, `GET /api/companies`,
//! `GET /api/companies/{id}`, `DELETE /api/companies/{id}`) live in
//! `dapple-prompt-api::handler::companies` and call into the functions here.

use crate::DbPool;
use crate::error::DataError;
use crate::model::companies::CompanyModel;
use sqlx::types::Uuid;

/// Insert a single company (B2B partner). Slug format + uniqueness are
/// enforced at the DB layer; callers are expected to translate sqlx errors
/// into HTTP 400/409.
pub async fn insert_company(
    pool: &DbPool,
    slug: &str,
    name: &str,
) -> Result<CompanyModel, DataError> {
    let row = sqlx::query_file_as!(
        CompanyModel,
        "assets/.sql/companies/insert_company.sql",
        // Alphabetical bind order: name, slug
        name,
        slug,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(row)
}

/// List all companies. When `include_revoked` is true, returns revoked rows
/// as well; otherwise only live rows (revoked_at IS NULL) are returned.
pub async fn select_companies(
    pool: &DbPool,
    include_revoked: bool,
) -> Result<Vec<CompanyModel>, DataError> {
    let rows = sqlx::query_file_as!(
        CompanyModel,
        "assets/.sql/companies/select_companies.sql",
        include_revoked,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Look up a company by its UUID. Used by the external auth extractor's
/// pre-resolution step (JWT carries the UUID; the extractor resolves the
/// slug once per request) AND by the admin `GET /api/companies/{id}`
/// handler.
pub async fn select_company_by_id(
    pool: &DbPool,
    id: Uuid,
) -> Result<Option<CompanyModel>, DataError> {
    let row = sqlx::query_file_as!(
        CompanyModel,
        "assets/.sql/companies/select_company_by_id.sql",
        id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Look up a company by its slug. Used by admin tooling and by the
/// initial dapple-namespace bootstrap (`select_company_by_slug(pool, "dapple")`
/// resolves the system org's UUID at server startup).
pub async fn select_company_by_slug(
    pool: &DbPool,
    slug: &str,
) -> Result<Option<CompanyModel>, DataError> {
    let row = sqlx::query_file_as!(
        CompanyModel,
        "assets/.sql/companies/select_company_by_slug.sql",
        slug,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Mark a company revoked. Idempotent: if the company is already revoked,
/// `revoked_at` is unchanged. Returns `None` if the id does not exist;
/// handler maps that to HTTP 404.
pub async fn revoke_company(pool: &DbPool, id: Uuid) -> Result<Option<CompanyModel>, DataError> {
    let row = sqlx::query_file_as!(CompanyModel, "assets/.sql/companies/revoke_company.sql", id,)
        .fetch_optional(pool.get_pool())
        .await?;

    Ok(row)
}
