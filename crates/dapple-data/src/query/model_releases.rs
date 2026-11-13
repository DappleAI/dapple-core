//! Query functions for model releases
//!
//! Model releases are specific model configurations at a specific provider.
//! This replaces the legacy engines.rs query functions with the new taxonomy.

use crate::DbPool;
use crate::error::DataError;
use crate::model::model_release::{ModelRelease, ModelReleaseModel};

/// Select model releases by lineage key (e.g., "vits", "flan-t5", "claude-3-5")
pub async fn select_model_releases_by_lineage(
    pool: &DbPool,
    lineage_key: &str,
) -> Result<Vec<ModelRelease>, DataError> {
    let models = sqlx::query_file_as!(
        ModelReleaseModel,
        "assets/.sql/model_releases/select_model_releases_by_lineage.sql",
        lineage_key
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

/// Select a single model release by lineage, tier, and optionally release version
pub async fn select_model_release(
    pool: &DbPool,
    lineage_key: &str,
    tier: &str,
    release: Option<&str>,
) -> Result<Option<ModelRelease>, DataError> {
    let model = sqlx::query_file_as!(
        ModelReleaseModel,
        "assets/.sql/model_releases/select_model_release.sql",
        lineage_key,
        tier,
        release
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(model.map(|m| m.into()))
}
