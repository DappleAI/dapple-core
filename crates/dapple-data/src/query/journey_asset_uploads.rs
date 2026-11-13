//! Query functions for staged operator uploads on a journey.

use crate::DbPool;
use crate::error::DataError;
use crate::model::journey_asset_uploads::{
    InsertJourneyAssetUploadArgs, JourneyAssetUploadModel, RemovedJourneyAssetUploadModel,
};
use sqlx::types::Uuid;

/// Stage uploads for a journey (batch). Returns the inserted rows with asset
/// metadata joined in. An unknown `journey_id` inserts nothing and returns an
/// empty vec (the caller maps that to 404).
pub async fn insert_journey_asset_uploads(
    pool: &DbPool,
    args: InsertJourneyAssetUploadArgs,
) -> Result<Vec<JourneyAssetUploadModel>, DataError> {
    assert_eq!(args.asset_ids.len(), args.file_names.len());
    assert_eq!(args.asset_ids.len(), args.input_keys.len());

    let rows = sqlx::query_file_as!(
        JourneyAssetUploadModel,
        "assets/.sql/journey_asset_uploads/insert_journey_asset_uploads.sql",
        &args.asset_ids,
        &args.file_names,
        &args.input_keys,
        args.journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Active (non-removed) staged uploads for a journey, creation order
/// (UUIDv7 `ORDER BY id` — latest per input_key is the last matching entry).
pub async fn select_journey_asset_uploads_by_journey(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<JourneyAssetUploadModel>, DataError> {
    let rows = sqlx::query_file_as!(
        JourneyAssetUploadModel,
        "assets/.sql/journey_asset_uploads/select_journey_asset_uploads_by_journey.sql",
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Soft-remove one staged upload. Returns `Some` if an active row belonging to
/// the journey was removed, `None` otherwise (unknown, foreign, or already
/// removed — the caller maps that to 404).
pub async fn remove_journey_asset_upload(
    pool: &DbPool,
    journey_id: Uuid,
    upload_id: Uuid,
) -> Result<Option<RemovedJourneyAssetUploadModel>, DataError> {
    let row = sqlx::query_file_as!(
        RemovedJourneyAssetUploadModel,
        "assets/.sql/journey_asset_uploads/remove_journey_asset_upload.sql",
        journey_id,
        upload_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
