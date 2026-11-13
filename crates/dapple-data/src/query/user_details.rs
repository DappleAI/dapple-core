use crate::error::DataError;
use crate::model::user_details::{FlatUserDetailModel, UserDetail, UserDetailModel};
use crate::{DbPool, FilterOptions};
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Result of user existence check - just the IDs needed to confirm user exists.
#[derive(Debug)]
pub struct UserExistsResult {
    pub user_id: i64,
    pub id: Uuid,
}

/// Reference to a user identity - mirrors JWTUserIdRef from dapple-prompt-api.
/// Used by resolve_jwt_user_id_ref to look up or create user_details.
#[derive(Debug, Clone)]
pub enum UserIdRef {
    /// Internal auth.users.id from NextAuth (i64)
    AuthUser(i64),
    /// External user_details.id (UUID)
    ObjectId(Uuid),
}

/// Internal model for upsert query result (with nullable columns from COALESCE).
#[derive(Debug)]
struct UpsertUserDetailRow {
    user_id: Option<i64>,
    id: Option<Uuid>,
    email: Option<String>,
    created_at: Option<OffsetDateTime>,
}

/// Resolve a UserIdRef to UserDetail, auto-creating if looking up by auth_user_id.
pub async fn resolve_user_id_ref(
    pool: &DbPool,
    user_ref: &UserIdRef,
) -> Result<UserDetail, DataError> {
    let (auth_user_id, object_id) = match user_ref {
        UserIdRef::AuthUser(id) => (Some(*id), None),
        UserIdRef::ObjectId(uuid) => (None, Some(*uuid)),
    };

    let row = sqlx::query_file_as!(
        UpsertUserDetailRow,
        "assets/.sql/user_details/upsert_user_detail_by_jwt_ref.sql",
        auth_user_id,
        object_id
    )
    .fetch_optional(pool.get_pool())
    .await?
    .ok_or_else(|| DataError::MissingIdLookup(format!("user not found for ref: {:?}", user_ref)))?;

    // Extract non-null values (query guarantees at least one path succeeds)
    let _user_id = row
        .user_id
        .ok_or_else(|| DataError::MissingIdLookup("upsert returned null user_id".to_string()))?;
    let id = row
        .id
        .ok_or_else(|| DataError::MissingIdLookup("upsert returned null id".to_string()))?;
    let created_at = row
        .created_at
        .ok_or_else(|| DataError::MissingIdLookup("upsert returned null created_at".to_string()))?;

    Ok(UserDetail {
        id,
        email: row.email.unwrap_or_default(),
        created_at: Some(created_at.into()),
    })
}

/// Check if a user exists by UserIdRef without auto-creating.
/// Returns Some(UserExistsResult) if found, None if not found.
/// Use this for auth validation where we don't want to auto-create users.
pub async fn check_user_exists(
    pool: &DbPool,
    user_ref: &UserIdRef,
) -> Result<Option<UserExistsResult>, DataError> {
    let (auth_user_id, object_id) = match user_ref {
        UserIdRef::AuthUser(id) => (Some(*id), None),
        UserIdRef::ObjectId(uuid) => (None, Some(*uuid)),
    };

    let row = sqlx::query_file_as!(
        UserExistsResult,
        "assets/.sql/user_details/check_user_exists_by_jwt_ref.sql",
        auth_user_id,
        object_id
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

pub async fn select_user_detail_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<UserDetail, DataError> {
    let model = select_user_detail_model_by_universal_id(pool, universal_id).await?;
    Ok(model.into())
}

pub(crate) async fn select_user_detail_model_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<UserDetailModel, DataError> {
    let objs = select_user_detail_models_by_universal_ids(
        pool,
        vec![universal_id],
        FilterOptions::default(),
    )
    .await?;

    let obj = objs
        .into_iter()
        .next()
        .ok_or(DataError::MissingIdLookup(format!(
            "user_detail_id {} not found",
            universal_id
        )))?;

    Ok(obj)
}

pub async fn select_user_details_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<UserDetail>, DataError> {
    let models =
        select_user_detail_models_by_universal_ids(pool, universal_ids, filter_ops).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_user_detail_models_by_universal_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<UserDetailModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatUserDetailModel,
        "assets/.sql/user_details/select_user_details_by_object_ids.sql",
        &object_ids,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatUserDetailModel::fold_models(models)
}

pub async fn select_user_details(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<UserDetail>, DataError> {
    let models = select_user_detail_models(pool, filter_ops).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub async fn select_user_detail_models(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<UserDetailModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatUserDetailModel,
        "assets/.sql/user_details/select_user_details.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatUserDetailModel::fold_models(models)
}
