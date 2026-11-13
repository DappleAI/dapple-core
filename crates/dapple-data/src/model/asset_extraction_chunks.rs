use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;

// ============================================================================
// Asset Extraction Chunk Models
// ============================================================================

/// Row returned by `select_asset_extraction_chunks_by_extraction` — carries the
/// associative row plus the raw chunk bytes joined from `rev.chunks`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetExtractionChunkModel {
    pub extraction_id: Uuid,
    pub chunk_id: Uuid,
    pub ordinal: i32,
    pub data: Vec<u8>,
}

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for asset extraction chunks.
/// All vectors MUST be the same length.
#[derive(Clone, Debug, Default)]
pub struct InsertAssetExtractionChunkArgs {
    pub extraction_ids: Vec<Uuid>,
    pub chunk_ids: Vec<Uuid>,
    pub ordinals: Vec<i32>,
}
