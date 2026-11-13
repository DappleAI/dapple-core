use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::drafts::{
    Draft, DraftChunk, DraftChunkModel, DraftModel, FinalizedDraftModel, FinalizedDraftRow,
    InsertDraftArgs, InsertDraftChunkArgs, InsertedDraftChunkModel, InsertedDraftChunkRow,
    InsertedDraftModel,
};
use crate::{DbPool, get_single_row_by_id};
use sqlx::types::Uuid;

// ============================================================================
// Draft queries
// ============================================================================

/// Insert drafts for content under construction
pub async fn insert_drafts(pool: &DbPool, args: InsertDraftArgs) -> Result<Vec<Draft>, DataError> {
    if args.asset_kind_keys.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.asset_kind_keys.len() != args.mime_types.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "mime_types".into(),
            expected: args.asset_kind_keys.len(),
            found: args.mime_types.len(),
        }
        .into());
    }

    let inserted_models = sqlx::query_file_as!(
        InsertedDraftModel,
        "assets/.sql/drafts/insert_drafts.sql",
        &args.asset_kind_keys,
        &args.mime_types,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted_models.into_iter().map(|m| m.into()).collect())
}

/// Select drafts by object IDs
pub async fn select_drafts_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<Draft>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        DraftModel,
        "assets/.sql/drafts/select_drafts_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

/// Select a single draft by object ID
pub async fn select_draft_by_object_id(pool: &DbPool, object_id: Uuid) -> Result<Draft, DataError> {
    let drafts = select_drafts_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, drafts)
}

// ============================================================================
// Draft Chunk queries
// ============================================================================

/// Insert draft chunks (streaming writes with content-addressable dedup)
/// Creates permanent chunks if needed, reuses existing chunks for identical content
pub async fn insert_draft_chunks(
    pool: &DbPool,
    args: InsertDraftChunkArgs,
) -> Result<Vec<InsertedDraftChunkModel>, DataError> {
    if args.draft_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.draft_ids.len() != args.data.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "data".into(),
            expected: args.draft_ids.len(),
            found: args.data.len(),
        }
        .into());
    }

    let rows = sqlx::query_file_as!(
        InsertedDraftChunkRow,
        "assets/.sql/drafts/insert_draft_chunks.sql",
        &args.draft_ids,
        &args.data,
    )
    .fetch_all(pool.get_pool())
    .await?;

    rows.into_iter()
        .map(|r| r.try_into())
        .collect::<Result<Vec<_>, _>>()
}

/// Select draft chunks by draft object ID (for streaming reads during draft phase)
pub async fn select_draft_chunks_by_draft_object_id(
    pool: &DbPool,
    draft_object_id: Uuid,
) -> Result<Vec<DraftChunk>, DataError> {
    let models = sqlx::query_file_as!(
        DraftChunkModel,
        "assets/.sql/drafts/select_draft_chunks_by_draft_object_id.sql",
        draft_object_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Draft Finalization
// ============================================================================

/// Finalize a draft into an asset
/// Atomically: copy chunk refs from draft_chunks to asset_chunks, create asset
/// Note: chunks already exist in rev.chunks (created at draft write time with dedup)
pub async fn finalize_draft(
    pool: &DbPool,
    draft_object_id: Uuid,
    content_hash: &str,
) -> Result<FinalizedDraftModel, DataError> {
    let row = sqlx::query_file_as!(
        FinalizedDraftRow,
        "assets/.sql/drafts/finalize_draft.sql",
        draft_object_id,
        content_hash,
    )
    .fetch_one(pool.get_pool())
    .await?;

    row.try_into()
}
