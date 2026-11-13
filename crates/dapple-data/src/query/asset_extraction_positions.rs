use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::asset_extraction_positions::{
    AssetExtractionPositionModel, InsertAssetExtractionPositionArgs,
};
use sqlx::types::Uuid;

// ============================================================================
// INSERT
// ============================================================================

/// Batch insert asset extraction positions.
pub async fn insert_asset_extraction_positions(
    pool: &DbPool,
    args: InsertAssetExtractionPositionArgs,
) -> Result<Vec<AssetExtractionPositionModel>, DataError> {
    if args.extraction_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.extraction_ids.len() != args.byte_offsets.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "byte_offsets".into(),
            expected: args.extraction_ids.len(),
            found: args.byte_offsets.len(),
        }
        .into());
    }

    if args.extraction_ids.len() != args.byte_lengths.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "byte_lengths".into(),
            expected: args.extraction_ids.len(),
            found: args.byte_lengths.len(),
        }
        .into());
    }

    if args.extraction_ids.len() != args.source_positions.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "source_positions".into(),
            expected: args.extraction_ids.len(),
            found: args.source_positions.len(),
        }
        .into());
    }

    let rows = sqlx::query_file_as!(
        AssetExtractionPositionModel,
        "assets/.sql/asset_extraction_positions/insert_asset_extraction_positions.sql",
        &args.extraction_ids,
        &args.byte_offsets,
        &args.byte_lengths,
        &args.source_positions,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// SELECT
// ============================================================================

/// Select all positions for a given extraction ordered by byte_offset.
pub async fn select_asset_extraction_positions_by_extraction(
    pool: &DbPool,
    extraction_id: Uuid,
) -> Result<Vec<AssetExtractionPositionModel>, DataError> {
    let rows = sqlx::query_file_as!(
        AssetExtractionPositionModel,
        "assets/.sql/asset_extraction_positions/select_asset_extraction_positions_by_extraction.sql",
        extraction_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}
