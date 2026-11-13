use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::asset_extraction_chunks::{
    AssetExtractionChunkModel, InsertAssetExtractionChunkArgs,
};
use sqlx::types::Uuid;

// ============================================================================
// INSERT
// ============================================================================

/// Batch insert asset extraction chunk associations. Returns the (extraction_id,
/// chunk_id, ordinal) triples in the order they were persisted.
pub async fn insert_asset_extraction_chunks(
    pool: &DbPool,
    args: InsertAssetExtractionChunkArgs,
) -> Result<Vec<(Uuid, Uuid, i32)>, DataError> {
    if args.extraction_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.extraction_ids.len() != args.chunk_ids.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "chunk_ids".into(),
            expected: args.extraction_ids.len(),
            found: args.chunk_ids.len(),
        }
        .into());
    }

    if args.extraction_ids.len() != args.ordinals.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "ordinals".into(),
            expected: args.extraction_ids.len(),
            found: args.ordinals.len(),
        }
        .into());
    }

    // The insert query returns (extraction_id, chunk_id, ordinal) — there is no
    // joined chunk data here, so we materialize into a lightweight tuple.
    struct Row {
        extraction_id: Uuid,
        chunk_id: Uuid,
        ordinal: i32,
    }

    let rows = sqlx::query_file_as!(
        Row,
        "assets/.sql/asset_extraction_chunks/insert_asset_extraction_chunks.sql",
        &args.extraction_ids,
        &args.chunk_ids,
        &args.ordinals,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| (r.extraction_id, r.chunk_id, r.ordinal))
        .collect())
}

// ============================================================================
// SELECT
// ============================================================================

/// Select all chunks for an extraction (with raw bytes joined from `rev.chunks`)
/// ordered by ordinal.
pub async fn select_asset_extraction_chunks_by_extraction(
    pool: &DbPool,
    extraction_id: Uuid,
) -> Result<Vec<AssetExtractionChunkModel>, DataError> {
    let rows = sqlx::query_file_as!(
        AssetExtractionChunkModel,
        "assets/.sql/asset_extraction_chunks/select_asset_extraction_chunks_by_extraction.sql",
        extraction_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}
