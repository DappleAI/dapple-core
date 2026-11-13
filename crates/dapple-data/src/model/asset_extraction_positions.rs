use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// ============================================================================
// Asset Extraction Position Models
// ============================================================================

/// Full row from `rev.asset_extraction_positions`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetExtractionPositionModel {
    pub id: Uuid,
    pub extraction_id: Uuid,
    pub byte_offset: i64,
    pub byte_length: i64,
    pub source_position: serde_json::Value,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for asset extraction positions.
/// All vectors MUST be the same length.
#[derive(Clone, Debug, Default)]
pub struct InsertAssetExtractionPositionArgs {
    pub extraction_ids: Vec<Uuid>,
    pub byte_offsets: Vec<i64>,
    pub byte_lengths: Vec<i64>,
    pub source_positions: Vec<serde_json::Value>,
}
