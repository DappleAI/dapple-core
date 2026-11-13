//! Query functions for API tokens.

use crate::error::DataError;
use crate::model::api_tokens::{
    ApiToken, ApiTokenModel, ApiTokenValidationModel, FlatApiTokenModel, InsertApiTokenArgs,
    InsertedApiTokenModel, InsertedApiTokenRevocationModel,
};
use crate::{DbPool, FilterOptions, InsertColumnArgs};
use log::debug;
use sqlx::types::Uuid;

/// Insert new API tokens.
pub async fn insert_api_tokens(
    pool: &DbPool,
    args: InsertApiTokenArgs,
) -> Result<Vec<InsertedApiTokenModel>, DataError> {
    args.validate_column_counts(None)?;

    debug!("Inserting api_tokens: {:?}", args);

    let rows = sqlx::query_file_as!(
        InsertedApiTokenModel,
        "assets/.sql/api_tokens/insert_api_tokens.sql",
        &args.user_ids,
        &args.names,
        &args.token_hashes,
        &args.token_prefixes,
        &args.expires_ats as &[Option<time::OffsetDateTime>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    debug!("Inserted api_tokens: {:?}", rows);

    Ok(rows)
}

/// Select all API tokens for a user.
pub async fn select_api_tokens_by_user(
    pool: &DbPool,
    user_id: Uuid,
    filter_opts: FilterOptions,
) -> Result<Vec<ApiToken>, DataError> {
    let models = select_api_token_models_by_user(pool, user_id, filter_opts).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

/// Select all API token models for a user.
pub(crate) async fn select_api_token_models_by_user(
    pool: &DbPool,
    user_id: Uuid,
    filter_opts: FilterOptions,
) -> Result<Vec<ApiTokenModel>, DataError> {
    let (limit, offset) = filter_opts.sql_values();

    let rows = sqlx::query_file_as!(
        FlatApiTokenModel,
        "assets/.sql/api_tokens/select_api_tokens_by_user.sql",
        user_id,
        limit,
        offset,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatApiTokenModel::fold_models(rows)
}

/// Look up an API token by its hash for authentication.
pub async fn select_api_token_by_hash(
    pool: &DbPool,
    token_hash: &[u8],
) -> Result<Option<ApiTokenValidationModel>, DataError> {
    let row = sqlx::query_file_as!(
        ApiTokenValidationModel,
        "assets/.sql/api_tokens/select_api_token_by_hash.sql",
        token_hash,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Select a single API token by object_id.
pub async fn select_api_token_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<Option<ApiTokenModel>, DataError> {
    let row = sqlx::query_file_as!(
        FlatApiTokenModel,
        "assets/.sql/api_tokens/select_api_token_by_object_id.sql",
        object_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row.map(|r| ApiTokenModel {
        id: r.id,
        user_id: r.user_id,
        name: r.name,
        token_hash: r.token_hash,
        token_prefix: r.token_prefix,
        expires_at: r.expires_at,
        created_at: r.created_at,
        revoked_at: r.revoked_at,
    }))
}

/// Revoke an API token by object_id.
pub async fn revoke_api_token(
    pool: &DbPool,
    object_id: Uuid,
    reason: Option<String>,
) -> Result<Option<InsertedApiTokenRevocationModel>, DataError> {
    let row = sqlx::query_file_as!(
        InsertedApiTokenRevocationModel,
        "assets/.sql/api_tokens/insert_api_token_revocation.sql",
        object_id,
        reason,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
