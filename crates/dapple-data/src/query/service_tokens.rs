//! Query functions for service tokens.

use crate::error::DataError;
use crate::model::service_tokens::{
    FlatServiceTokenModel, InsertServiceTokenArgs, InsertedServiceTokenModel,
    InsertedServiceTokenRevocationModel, ServiceToken, ServiceTokenModel,
    ServiceTokenValidationModel,
};
use crate::{DbPool, FilterOptions, InsertColumnArgs};
use log::debug;
use sqlx::types::Uuid;

/// Insert new service tokens.
pub async fn insert_service_tokens(
    pool: &DbPool,
    args: InsertServiceTokenArgs,
) -> Result<Vec<InsertedServiceTokenModel>, DataError> {
    args.validate_column_counts(None)?;

    debug!("Inserting service_tokens: {:?}", args);

    let rows = sqlx::query_file_as!(
        InsertedServiceTokenModel,
        "assets/.sql/service_tokens/insert_service_tokens.sql",
        &args.service_ids,
        &args.names,
        &args.descriptions as &[Option<String>],
        &args.public_key_pems,
        &args.key_algorithms,
        &args.created_by_user_ids as &[Option<Uuid>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    debug!("Inserted service_tokens: {:?}", rows);

    Ok(rows)
}

/// Select all service tokens with pagination.
pub async fn select_service_tokens(
    pool: &DbPool,
    filter_opts: FilterOptions,
) -> Result<Vec<ServiceToken>, DataError> {
    let models = select_service_token_models(pool, filter_opts).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

/// Select all service token models with pagination.
pub(crate) async fn select_service_token_models(
    pool: &DbPool,
    filter_opts: FilterOptions,
) -> Result<Vec<ServiceTokenModel>, DataError> {
    let (limit, offset) = filter_opts.sql_values();

    let rows = sqlx::query_file_as!(
        FlatServiceTokenModel,
        "assets/.sql/service_tokens/select_service_tokens.sql",
        limit,
        offset,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatServiceTokenModel::fold_models(rows)
}

/// Look up a service token by service_id for authentication.
pub async fn select_service_token_by_service_id(
    pool: &DbPool,
    service_id: &str,
) -> Result<Option<ServiceTokenValidationModel>, DataError> {
    let row = sqlx::query_file_as!(
        ServiceTokenValidationModel,
        "assets/.sql/service_tokens/select_service_token_by_service_id.sql",
        service_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Select a single service token by object_id.
pub async fn select_service_token_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<Option<ServiceTokenModel>, DataError> {
    let row = sqlx::query_file_as!(
        FlatServiceTokenModel,
        "assets/.sql/service_tokens/select_service_token_by_object_id.sql",
        object_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row.map(|r| ServiceTokenModel {
        id: r.id,
        service_id: r.service_id,
        name: r.name,
        description: r.description,
        public_key_pem: r.public_key_pem,
        key_algorithm: r.key_algorithm,
        created_at: r.created_at,
        created_by_user_id: r.created_by_user_id,
        revoked_at: r.revoked_at,
    }))
}

/// Revoke a service token by object_id.
pub async fn revoke_service_token(
    pool: &DbPool,
    object_id: Uuid,
    reason: Option<String>,
) -> Result<Option<InsertedServiceTokenRevocationModel>, DataError> {
    let row = sqlx::query_file_as!(
        InsertedServiceTokenRevocationModel,
        "assets/.sql/service_tokens/insert_service_token_revocation.sql",
        object_id,
        reason,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
