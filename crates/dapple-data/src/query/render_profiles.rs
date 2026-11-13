//! Query functions for render profiles.

use crate::DbPool;
use crate::error::DataError;
use crate::model::render_profiles::RenderProfileModel;

/// Select all render profiles.
pub async fn select_render_profiles(pool: &DbPool) -> Result<Vec<RenderProfileModel>, DataError> {
    let rows = sqlx::query_file_as!(
        RenderProfileModel,
        "assets/.sql/render_profiles/select_render_profiles.sql"
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Select a render profile by its unique key.
pub async fn select_render_profile_by_key(
    pool: &DbPool,
    key: &str,
) -> Result<Option<RenderProfileModel>, DataError> {
    let row = sqlx::query_file_as!(
        RenderProfileModel,
        "assets/.sql/render_profiles/select_render_profile_by_key.sql",
        key
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
