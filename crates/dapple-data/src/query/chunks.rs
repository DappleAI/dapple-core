use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::chunks::{Chunk, ChunkModel, ChunkRangeModel, ChunkRangeRow};
use sqlx::types::Uuid;

// ============================================================================
// Chunk queries
// ============================================================================

/// Select all chunks for an asset (full content read)
pub async fn select_chunks_by_asset_object_id(
    pool: &DbPool,
    asset_object_id: Uuid,
) -> Result<Vec<Chunk>, DataError> {
    let models = sqlx::query_file_as!(
        ChunkModel,
        "assets/.sql/chunks/select_chunks_by_asset_object_id.sql",
        asset_object_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

/// Select chunks that overlap with a byte range
/// Returns chunks where any part falls within [start_byte, end_byte]
/// Application code extracts exact bytes from the returned chunks
pub async fn select_chunks_for_range(
    pool: &DbPool,
    asset_object_id: Uuid,
    start_byte: i64,
    end_byte: i64,
) -> Result<Vec<ChunkRangeModel>, DataError> {
    let rows = sqlx::query_file_as!(
        ChunkRangeRow,
        "assets/.sql/chunks/select_chunks_for_range.sql",
        asset_object_id,
        start_byte,
        end_byte,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// Read full asset content by concatenating all chunks
pub async fn read_asset_content(
    pool: &DbPool,
    asset_object_id: Uuid,
) -> Result<Vec<u8>, DataError> {
    let chunks = select_chunks_by_asset_object_id(pool, asset_object_id).await?;

    let mut content = Vec::new();
    for chunk in chunks {
        content.extend(chunk.data);
    }

    Ok(content)
}

/// Insert raw chunks into `rev.chunks` with content-addressable dedup.
///
/// Unlike `insert_draft_chunks`, this helper does not touch `rev.drafts` /
/// `rev.draft_chunks` — it is intended for callers that produce chunk data
/// directly (e.g. extraction text) and want to associate the resulting
/// `chunk_id`s with a non-draft parent (e.g. `rev.asset_extraction_chunks`).
///
/// Returns the chunk ids in the same order as the input data vector.
pub async fn insert_raw_chunks(pool: &DbPool, data: Vec<Vec<u8>>) -> Result<Vec<Uuid>, DataError> {
    if data.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    struct Row {
        #[allow(dead_code)]
        ord: i32,
        chunk_id: Uuid,
    }

    let rows = sqlx::query_file_as!(Row, "assets/.sql/chunks/insert_raw_chunks.sql", &data,)
        .fetch_all(pool.get_pool())
        .await?;

    Ok(rows.into_iter().map(|r| r.chunk_id).collect())
}

/// Read a byte range from an asset
/// Returns the exact bytes from start_byte to end_byte (inclusive)
pub async fn read_asset_range(
    pool: &DbPool,
    asset_object_id: Uuid,
    start_byte: i64,
    end_byte: i64,
) -> Result<Vec<u8>, DataError> {
    let chunk_models = select_chunks_for_range(pool, asset_object_id, start_byte, end_byte).await?;

    let mut content = Vec::new();
    for chunk in chunk_models {
        // Calculate which bytes to extract from this chunk
        let chunk_start = chunk.start_byte.max(start_byte);
        let chunk_end = chunk.end_byte.min(end_byte);

        // Convert to chunk-relative offsets
        let offset_start = (chunk_start - chunk.start_byte) as usize;
        let offset_end = (chunk_end - chunk.start_byte) as usize + 1;

        content.extend(&chunk.data[offset_start..offset_end]);
    }

    Ok(content)
}
