use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::asset_extractions::*;

// ============================================================================
// Asset Extraction Models
// ============================================================================

/// Full asset extraction row from `rev.asset_extractions`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetExtractionModel {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub extractor_kind: String,
    pub extractor_version: i32,
    pub status: String,
    pub text_length: Option<i64>,
    pub page_count: Option<i32>,
    pub metadata: serde_json::Value,
    pub error_message: Option<String>,
    pub is_current: bool,
    pub created_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
}

impl From<AssetExtractionModel> for AssetExtraction {
    fn from(m: AssetExtractionModel) -> Self {
        Self {
            id: m.id,
            asset_id: m.asset_id,
            extractor_kind: m.extractor_kind,
            extractor_version: m.extractor_version,
            status: m.status,
            text_length: m.text_length,
            page_count: m.page_count,
            metadata: m.metadata,
            error_message: m.error_message,
            is_current: m.is_current,
            created_at: m.created_at.into(),
            completed_at: m.completed_at.map(Into::into),
        }
    }
}

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for asset extractions.
/// All vectors MUST be the same length.
#[derive(Clone, Debug, Default)]
pub struct InsertAssetExtractionArgs {
    pub asset_ids: Vec<Uuid>,
    pub extractor_kinds: Vec<String>,
    pub extractor_versions: Vec<i32>,
}
