use crate::DateTime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// ============================================================================
// Chunk Checksum Models
// ============================================================================

/// Chunk missing its checksum record (raw row from query)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChunkMissingChecksumRow {
    pub id: Uuid,
    pub byte_length: Option<i32>, // octet_length returns nullable int
    pub created_at: OffsetDateTime,
}

/// Chunk missing its checksum record
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChunkMissingChecksumModel {
    pub id: Uuid,
    pub byte_length: i64,
    pub created_at: OffsetDateTime,
}

impl From<ChunkMissingChecksumRow> for ChunkMissingChecksumModel {
    fn from(row: ChunkMissingChecksumRow) -> Self {
        Self {
            id: row.id,
            byte_length: row.byte_length.unwrap_or(0) as i64,
            created_at: row.created_at,
        }
    }
}

/// Raw row for chunk with mismatched checksum
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChunkChecksumMismatchRow {
    pub id: Uuid,
    pub stored_checksum: Vec<u8>,
    pub computed_checksum: Option<Vec<u8>>, // sha256 returns nullable
    pub byte_length: Option<i32>,
    pub created_at: OffsetDateTime,
}

/// Chunk with mismatched checksum (integrity violation)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChunkChecksumMismatchModel {
    pub id: Uuid,
    pub stored_checksum: Vec<u8>,
    pub computed_checksum: Vec<u8>,
    pub byte_length: i64,
    pub created_at: OffsetDateTime,
}

impl From<ChunkChecksumMismatchRow> for ChunkChecksumMismatchModel {
    fn from(row: ChunkChecksumMismatchRow) -> Self {
        Self {
            id: row.id,
            stored_checksum: row.stored_checksum,
            computed_checksum: row.computed_checksum.unwrap_or_default(),
            byte_length: row.byte_length.unwrap_or(0) as i64,
            created_at: row.created_at,
        }
    }
}

/// Duplicate checksum pointing to multiple chunks
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DuplicateChecksumModel {
    pub checksum: Vec<u8>,
    pub chunk_count: Option<i64>,
    pub chunk_ids: Option<Vec<Uuid>>,
}

/// Newly inserted checksum record
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedChecksumModel {
    pub id: Uuid,
    pub checksum: Vec<u8>,
    pub chunk_id: Uuid,
    pub created_at: OffsetDateTime,
}

/// Asset with integrity issues
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetIntegrityIssueModel {
    pub asset_id: Uuid,
    pub path_id: Uuid,
    pub content_length: i64,
    pub mime_type: String,
    pub created_at: OffsetDateTime,
    pub issue_type: Option<String>,
}

// ============================================================================
// Public DTOs
// ============================================================================

/// Summary of chunk integrity issues
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct IntegritySummary {
    pub chunks_missing_checksums: i64,
    pub chunks_with_mismatches: i64,
    pub duplicate_checksum_groups: i64,
    pub affected_assets: i64,
}

/// DTO for chunk missing checksum
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct ChunkMissingChecksum {
    pub id: Uuid,
    pub byte_length: i64,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<ChunkMissingChecksumModel> for ChunkMissingChecksum {
    fn from(model: ChunkMissingChecksumModel) -> Self {
        Self {
            id: model.id,
            byte_length: model.byte_length,
            created_at: model.created_at.into(),
        }
    }
}

/// DTO for checksum mismatch
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct ChunkChecksumMismatch {
    pub id: Uuid,
    pub byte_length: i64,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<ChunkChecksumMismatchModel> for ChunkChecksumMismatch {
    fn from(model: ChunkChecksumMismatchModel) -> Self {
        Self {
            id: model.id,
            byte_length: model.byte_length,
            created_at: model.created_at.into(),
        }
    }
}

/// DTO for asset with integrity issue
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct AssetIntegrityIssue {
    pub asset_id: Uuid,
    pub path_id: Uuid,
    pub content_length: i64,
    pub mime_type: String,
    pub issue_type: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<AssetIntegrityIssueModel> for AssetIntegrityIssue {
    fn from(model: AssetIntegrityIssueModel) -> Self {
        Self {
            asset_id: model.asset_id,
            path_id: model.path_id,
            content_length: model.content_length,
            mime_type: model.mime_type,
            issue_type: model.issue_type.unwrap_or_else(|| "unknown".to_string()),
            created_at: model.created_at.into(),
        }
    }
}
