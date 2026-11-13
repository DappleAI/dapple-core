use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::asset_dataset_extractions::{
    AssetDatasetExtractionModel, InsertAssetDatasetExtractionArgs,
};
use sqlx::types::Uuid;

// ============================================================================
// INSERT
// ============================================================================

/// Batch insert dataset extraction rows in the `pending` state.
pub async fn insert_asset_dataset_extractions(
    pool: &DbPool,
    args: InsertAssetDatasetExtractionArgs,
) -> Result<Vec<AssetDatasetExtractionModel>, DataError> {
    if args.asset_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.asset_ids.len() != args.extractor_versions.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "extractor_versions".into(),
            expected: args.asset_ids.len(),
            found: args.extractor_versions.len(),
        }
        .into());
    }

    let rows = sqlx::query_file_as!(
        AssetDatasetExtractionModel,
        "assets/.sql/asset_dataset_extractions/insert_asset_dataset_extraction.sql",
        &args.asset_ids,
        &args.extractor_versions,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// SELECT
// ============================================================================

/// Select the current (latest per asset) dataset extraction for an asset.
/// UUIDv7 `id` is creation-ordered, so this is the newest extraction row.
/// Returns None when no extraction exists yet. The caller inspects `.status`
/// to distinguish `ok` / pending / running / failed.
///
/// Shared by both bind handlers, the computed-output producer, and the
/// `dataset schema` command.
pub async fn select_current_asset_dataset_extraction(
    pool: &DbPool,
    asset_id: Uuid,
) -> Result<Option<AssetDatasetExtractionModel>, DataError> {
    let row = sqlx::query_file_as!(
        AssetDatasetExtractionModel,
        "assets/.sql/asset_dataset_extractions/select_current_asset_dataset_extraction.sql",
        asset_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Resolve the current dataset extraction id an operator/computed binding should
/// PIN for `asset_id`, requiring it to be `ok`. Shared by both bind-handler
/// surfaces so the "extraction pending / not a dataset" error state is uniform.
///
/// * `ok` extraction → its id.
/// * pending / running / failed → `InvalidInput` (surfaces a clear "still
///   processing" message; the client retries once the worker finishes).
/// * no extraction row → `InvalidInput`.
pub async fn resolve_bindable_dataset_extraction_id(
    pool: &DbPool,
    asset_id: Uuid,
) -> Result<Uuid, DataError> {
    match select_current_asset_dataset_extraction(pool, asset_id).await? {
        Some(ext) if ext.status == "ok" => Ok(ext.id),
        Some(ext) => Err(DataError::InvalidInput(format!(
            "dataset extraction for asset {asset_id} is not ready (status: {}); the \
             spreadsheet is still being processed — retry shortly",
            ext.status
        ))),
        None => Err(DataError::InvalidInput(format!(
            "no dataset extraction found for asset {asset_id}; it may still be \
             processing or the file is not a supported spreadsheet"
        ))),
    }
}

// ============================================================================
// State transitions
// ============================================================================

/// Attempt to transition an extraction from `pending` to `running`. Returns
/// `Ok(None)` if the extraction was not in the `pending` state.
pub async fn mark_asset_dataset_extraction_running(
    pool: &DbPool,
    id: Uuid,
) -> Result<Option<AssetDatasetExtractionModel>, DataError> {
    let row = sqlx::query_file_as!(
        AssetDatasetExtractionModel,
        "assets/.sql/asset_dataset_extractions/mark_asset_dataset_extraction_running.sql",
        id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Transition an extraction to `ok`, writing the schema envelope and counts.
/// This is the single point the write-once envelope/content columns are set.
pub async fn mark_asset_dataset_extraction_ok(
    pool: &DbPool,
    id: Uuid,
    envelope: serde_json::Value,
    table_count: Option<i32>,
    row_count: Option<i64>,
) -> Result<Option<AssetDatasetExtractionModel>, DataError> {
    let row = sqlx::query_file_as!(
        AssetDatasetExtractionModel,
        "assets/.sql/asset_dataset_extractions/mark_asset_dataset_extraction_ok.sql",
        id,
        envelope,
        table_count,
        row_count,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Transition an extraction to `failed` with an error message.
pub async fn mark_asset_dataset_extraction_failed(
    pool: &DbPool,
    id: Uuid,
    error_message: &str,
) -> Result<Option<AssetDatasetExtractionModel>, DataError> {
    let row = sqlx::query_file_as!(
        AssetDatasetExtractionModel,
        "assets/.sql/asset_dataset_extractions/mark_asset_dataset_extraction_failed.sql",
        id,
        error_message,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
