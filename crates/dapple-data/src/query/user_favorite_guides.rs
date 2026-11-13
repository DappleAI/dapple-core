//! Query functions for user favorite guides.

use crate::error::DataError;
use crate::model::user_favorite_guides::{
    InsertedUserFavoriteGuideModel, InsertedUserFavoriteGuideRemovedModel, IsFavoritedModel,
    UserFavoriteGuide, UserFavoriteGuideWithObjectIdModel,
};
use crate::{DbPool, FilterOptions};
use log::debug;
use sqlx::types::Uuid;

/// Add a guide to user's favorites.
/// Returns Some if the favorite was created, None if already favorited or guide not found.
pub async fn insert_favorite(
    pool: &DbPool,
    user_id: Uuid,
    guide_path_object_id: Uuid,
) -> Result<Option<InsertedUserFavoriteGuideModel>, DataError> {
    debug!(
        "Inserting favorite for user_id={}, guide_path_object_id={}",
        user_id, guide_path_object_id
    );

    let row = sqlx::query_file_as!(
        InsertedUserFavoriteGuideModel,
        "assets/.sql/user_favorite_guides/insert_favorite.sql",
        user_id,
        guide_path_object_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    debug!("Inserted favorite: {:?}", row);

    Ok(row)
}

/// Remove a guide from user's favorites.
/// Returns Some if the favorite was removed, None if not currently favorited.
pub async fn remove_favorite(
    pool: &DbPool,
    user_id: Uuid,
    guide_path_object_id: Uuid,
) -> Result<Option<InsertedUserFavoriteGuideRemovedModel>, DataError> {
    debug!(
        "Removing favorite for user_id={}, guide_path_object_id={}",
        user_id, guide_path_object_id
    );

    let row = sqlx::query_file_as!(
        InsertedUserFavoriteGuideRemovedModel,
        "assets/.sql/user_favorite_guides/remove_favorite.sql",
        user_id,
        guide_path_object_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    debug!("Removed favorite: {:?}", row);

    Ok(row)
}

/// Select all active favorites for a user.
pub async fn select_favorites_by_user(
    pool: &DbPool,
    user_id: Uuid,
    filter_opts: FilterOptions,
) -> Result<Vec<UserFavoriteGuide>, DataError> {
    let (limit, offset) = filter_opts.sql_values();

    let rows = sqlx::query_file_as!(
        UserFavoriteGuideWithObjectIdModel,
        "assets/.sql/user_favorite_guides/select_favorites_by_user.sql",
        user_id,
        limit,
        offset,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// Check if a specific guide is favorited by user.
pub async fn check_favorite(
    pool: &DbPool,
    user_id: Uuid,
    guide_path_object_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_as!(
        IsFavoritedModel,
        "assets/.sql/user_favorite_guides/check_favorite.sql",
        user_id,
        guide_path_object_id,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(row.is_favorited.unwrap_or(false))
}
