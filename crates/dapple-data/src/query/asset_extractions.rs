use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::asset_extractions::{AssetExtractionModel, InsertAssetExtractionArgs};
use sqlx::types::Uuid;

// ============================================================================
// INSERT
// ============================================================================

/// Batch insert asset extraction rows in the `pending` state.
pub async fn insert_asset_extractions(
    pool: &DbPool,
    args: InsertAssetExtractionArgs,
) -> Result<Vec<AssetExtractionModel>, DataError> {
    if args.asset_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.asset_ids.len() != args.extractor_kinds.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "extractor_kinds".into(),
            expected: args.asset_ids.len(),
            found: args.extractor_kinds.len(),
        }
        .into());
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
        AssetExtractionModel,
        "assets/.sql/asset_extractions/insert_asset_extractions.sql",
        &args.asset_ids,
        &args.extractor_kinds,
        &args.extractor_versions,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// SELECT
// ============================================================================

/// Select a single asset extraction by id. Returns None when no row matches.
pub async fn select_asset_extraction_by_id(
    pool: &DbPool,
    id: Uuid,
) -> Result<Option<AssetExtractionModel>, DataError> {
    let row = sqlx::query_file_as!(
        AssetExtractionModel,
        "assets/.sql/asset_extractions/select_asset_extraction_by_id.sql",
        id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Select the latest (is_current = TRUE) extraction for a given asset and
/// extractor kind.
pub async fn select_latest_asset_extraction(
    pool: &DbPool,
    asset_id: Uuid,
    extractor_kind: &str,
) -> Result<Option<AssetExtractionModel>, DataError> {
    let row = sqlx::query_file_as!(
        AssetExtractionModel,
        "assets/.sql/asset_extractions/select_latest_asset_extraction_by_asset_and_kind.sql",
        asset_id,
        extractor_kind,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

// ============================================================================
// State transitions
// ============================================================================

/// Attempt to transition an extraction from `pending` to `running`. Returns
/// `Ok(None)` if the extraction was not in the `pending` state.
pub async fn mark_asset_extraction_running(
    pool: &DbPool,
    id: Uuid,
) -> Result<Option<AssetExtractionModel>, DataError> {
    let row = sqlx::query_file_as!(
        AssetExtractionModel,
        "assets/.sql/asset_extractions/mark_asset_extraction_running.sql",
        id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Transition an extraction to the `ok` state with extracted metadata.
pub async fn mark_asset_extraction_ok(
    pool: &DbPool,
    id: Uuid,
    text_length: i64,
    page_count: Option<i32>,
    metadata: serde_json::Value,
) -> Result<Option<AssetExtractionModel>, DataError> {
    let row = sqlx::query_file_as!(
        AssetExtractionModel,
        "assets/.sql/asset_extractions/mark_asset_extraction_ok.sql",
        id,
        text_length,
        page_count,
        metadata,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Transition an extraction to the `failed` state with an error message.
pub async fn mark_asset_extraction_failed(
    pool: &DbPool,
    id: Uuid,
    error_message: &str,
) -> Result<Option<AssetExtractionModel>, DataError> {
    let row = sqlx::query_file_as!(
        AssetExtractionModel,
        "assets/.sql/asset_extractions/mark_asset_extraction_failed.sql",
        id,
        error_message,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
