use crate::error::DataError;
use crate::{DateTime, InsertColumnArgs};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// ============================================================================
// Draft Models
// ============================================================================

/// Result from INSERT...RETURNING for draft
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedDraftModel {
    pub id: Uuid,
    pub asset_kind_id: i16,
    pub asset_kind_key: String,
    pub mime_type: String,
    pub created_at: OffsetDateTime,
}

/// Internal model for draft
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DraftModel {
    pub id: Uuid,
    pub asset_kind_id: i16,
    pub asset_kind_key: String,
    pub mime_type: String,
    pub created_at: OffsetDateTime,
}

impl crate::Keyed for DraftModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Public DTO for draft
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Draft {
    pub id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<DraftModel> for Draft {
    fn from(model: DraftModel) -> Self {
        Self {
            id: model.id,
            asset_kind: model.asset_kind_key,
            mime_type: model.mime_type,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedDraftModel> for Draft {
    fn from(model: InsertedDraftModel) -> Self {
        Self {
            id: model.id,
            asset_kind: model.asset_kind_key,
            mime_type: model.mime_type,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for Draft {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Draft Chunk Models
// ============================================================================

/// Raw row from INSERT...RETURNING for draft chunk (with nullable CTE fields)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedDraftChunkRow {
    pub id: Uuid,
    pub draft_id: Uuid,
    pub chunk_id: Uuid,
    pub byte_length: Option<i64>, // Nullable due to CTE
    pub ordinal: i32,
    pub created_at: OffsetDateTime,
}

/// Result from INSERT...RETURNING for draft chunk
/// Now includes chunk reference info since deduplication happens at write time
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedDraftChunkModel {
    pub id: Uuid,
    pub draft_id: Uuid,
    pub chunk_id: Uuid,
    pub byte_length: i64,
    pub ordinal: i32,
    pub created_at: OffsetDateTime,
}

impl TryFrom<InsertedDraftChunkRow> for InsertedDraftChunkModel {
    type Error = DataError;

    fn try_from(row: InsertedDraftChunkRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            draft_id: row.draft_id,
            chunk_id: row.chunk_id,
            byte_length: row.byte_length.unwrap_or(0),
            ordinal: row.ordinal,
            created_at: row.created_at,
        })
    }
}

/// Internal model for draft chunk
/// References the permanent chunk via chunk_id
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DraftChunkModel {
    pub id: Uuid,
    pub draft_id: Uuid,
    pub chunk_id: Uuid,
    pub data: Vec<u8>,
    pub ordinal: i32,
    pub created_at: OffsetDateTime,
}

/// Public DTO for draft chunk
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct DraftChunk {
    pub id: Uuid,
    pub draft_id: Uuid,
    pub chunk_id: Uuid,
    #[serde(skip_serializing)]
    #[schemars(skip)]
    pub data: Vec<u8>,
    pub byte_length: i64,
    pub ordinal: i32,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<DraftChunkModel> for DraftChunk {
    fn from(model: DraftChunkModel) -> Self {
        let byte_length = model.data.len() as i64;
        Self {
            id: model.id,
            draft_id: model.draft_id,
            chunk_id: model.chunk_id,
            data: model.data,
            byte_length,
            ordinal: model.ordinal,
            created_at: model.created_at.into(),
        }
    }
}

// ============================================================================
// Insert Args
// ============================================================================

/// Column-wise insert arguments for drafts
#[derive(Clone, Debug)]
pub struct InsertDraftArgs {
    pub asset_kind_keys: Vec<String>,
    pub mime_types: Vec<String>,
}

impl InsertColumnArgs for InsertDraftArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("asset_kind_keys", self.asset_kind_keys.len()),
            ("mime_types", self.mime_types.len()),
        ])
    }
}

/// Column-wise insert arguments for draft chunks
#[derive(Clone, Debug)]
pub struct InsertDraftChunkArgs {
    pub draft_ids: Vec<Uuid>,
    pub data: Vec<Vec<u8>>,
}

impl InsertColumnArgs for InsertDraftChunkArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("draft_ids", self.draft_ids.len()),
            ("data", self.data.len()),
        ])
    }
}

// ============================================================================
// Finalize Draft Result
// ============================================================================

/// Raw database row from finalize_draft query (with nullable aggregate fields)
/// Note: Many fields are Option because they come from CTEs/joins that SQLx infers as nullable
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FinalizedDraftRow {
    pub asset_id: Option<Uuid>,
    pub path_id: Option<Uuid>,
    pub draft_id: Option<Uuid>,
    pub content_length: Option<i64>,
    pub mime_type: Option<String>,
    pub chunk_count: Option<i64>,
    pub content_hash: Option<String>,
    pub created_at: Option<OffsetDateTime>,
}

/// Result from finalizing a draft into an asset
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FinalizedDraftModel {
    pub asset_id: Uuid,
    pub path_id: Uuid,
    pub draft_id: Uuid,
    pub content_length: i64,
    pub mime_type: String,
    pub chunk_count: i64,
    pub content_hash: String,
    pub created_at: OffsetDateTime,
}

impl TryFrom<FinalizedDraftRow> for FinalizedDraftModel {
    type Error = crate::error::DataError;

    fn try_from(row: FinalizedDraftRow) -> Result<Self, Self::Error> {
        use crate::error::{DataError, InsertArgColumnMismatch};
        let missing_field_error = || DataError::from(InsertArgColumnMismatch::EmptyColumnSet {});

        Ok(Self {
            asset_id: row.asset_id.ok_or_else(missing_field_error)?,
            path_id: row.path_id.ok_or_else(missing_field_error)?,
            draft_id: row.draft_id.ok_or_else(missing_field_error)?,
            content_length: row.content_length.unwrap_or(0),
            mime_type: row.mime_type.ok_or_else(missing_field_error)?,
            chunk_count: row.chunk_count.unwrap_or(0),
            content_hash: row.content_hash.ok_or_else(missing_field_error)?,
            created_at: row.created_at.ok_or_else(missing_field_error)?,
        })
    }
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl crate::traits::InsertedModel for InsertedDraftModel {}

impl crate::Keyed for InsertedDraftModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for InsertedDraftModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for DraftModel {
    fn id(&self) -> &Uuid {
        <Self as crate::Keyed>::id(self)
    }
}

impl crate::traits::PrincipalObject for DraftModel {
    fn display_name(&self) -> Option<&str> {
        Some(&self.mime_type)
    }
}

impl crate::traits::DataTransferObject for Draft {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
