//! Query functions for the guide render cache.

use crate::DbPool;
use crate::error::DataError;
use crate::model::guide_renders::GuideRenderModel;
use sqlx::types::Uuid;

/// Check the render cache for an existing render of a guide with a specific profile.
pub async fn select_guide_render_by_guide_and_profile(
    pool: &DbPool,
    guide_id: Uuid,
    render_profile_id: i16,
) -> Result<Option<GuideRenderModel>, DataError> {
    let row = sqlx::query_file_as!(
        GuideRenderModel,
        "assets/.sql/guide_renders/select_guide_render_by_guide_and_profile.sql",
        guide_id,
        render_profile_id
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
