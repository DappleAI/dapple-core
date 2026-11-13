//! Persist raw bytes as an asset via the draft → chunks → finalize pipeline.
//!
//! A local copy of the main conduit's `persist_media_asset` (this service holds
//! its own DB access), so a produced song map / OTIO timeline lands as a
//! retrievable asset and the caller gets back an id.

use dapple_data::model::drafts::{InsertDraftArgs, InsertDraftChunkArgs};
use dapple_data::query::drafts::{finalize_draft, insert_draft_chunks, insert_drafts};
use dapple_data::DbPool;
use uuid::Uuid;

use crate::error::OximediaError;

/// Maximum chunk size in bytes (256KB) for asset creation.
const MAX_CHUNK_SIZE: usize = 262_144;

/// Persist `bytes` as an asset of `asset_kind` (one of the seeded
/// `rev.asset_kinds` keys) with `mime_type`. Returns `(asset_id, content_length)`.
pub async fn persist_media_asset(
    pool: &DbPool,
    bytes: &[u8],
    asset_kind: &str,
    mime_type: &str,
) -> Result<(Uuid, i64), OximediaError> {
    if bytes.is_empty() {
        return Err(OximediaError::Internal(format!(
            "refusing to persist a 0-byte {asset_kind} asset (empty output)"
        )));
    }

    let drafts = insert_drafts(
        pool,
        InsertDraftArgs {
            asset_kind_keys: vec![asset_kind.to_string()],
            mime_types: vec![mime_type.to_string()],
        },
    )
    .await?;
    let draft = drafts
        .into_iter()
        .next()
        .ok_or_else(|| OximediaError::Internal("no draft created for asset".to_string()))?;

    for chunk in bytes.chunks(MAX_CHUNK_SIZE) {
        insert_draft_chunks(
            pool,
            InsertDraftChunkArgs {
                draft_ids: vec![draft.id],
                data: vec![chunk.to_vec()],
            },
        )
        .await?;
    }

    let content_hash = dapple_data::hash::hash_asset(bytes);
    let finalized = finalize_draft(pool, draft.id, content_hash.as_str()).await?;
    Ok((finalized.asset_id, finalized.content_length))
}
