use crate::DateTime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// ============================================================================
// Chunk Models
// ============================================================================

/// Internal model for chunk (from SELECT)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChunkModel {
    pub id: Uuid,
    pub data: Vec<u8>,
    pub created_at: OffsetDateTime,
    pub ordinal: i32,
}

/// Raw database row for chunk range queries (with nullable window function results)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChunkRangeRow {
    pub id: Uuid,
    pub data: Vec<u8>,
    pub start_byte: Option<i64>,
    pub end_byte: Option<i64>,
    pub created_at: OffsetDateTime,
}

/// Chunk with byte position information (for range queries)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChunkRangeModel {
    pub id: Uuid,
    pub data: Vec<u8>,
    pub start_byte: i64,
    pub end_byte: i64,
    pub created_at: OffsetDateTime,
}

impl From<ChunkRangeRow> for ChunkRangeModel {
    fn from(row: ChunkRangeRow) -> Self {
        Self {
            id: row.id,
            data: row.data,
            start_byte: row.start_byte.unwrap_or(0),
            end_byte: row.end_byte.unwrap_or(0),
            created_at: row.created_at,
        }
    }
}

/// Public DTO for chunk (without data, just metadata)
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct ChunkMeta {
    pub id: Uuid,
    pub byte_length: i64,
    pub ordinal: i32,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<ChunkModel> for ChunkMeta {
    fn from(model: ChunkModel) -> Self {
        Self {
            id: model.id,
            byte_length: model.data.len() as i64,
            ordinal: model.ordinal,
            created_at: model.created_at.into(),
        }
    }
}

/// Full chunk data (used for streaming reads)
#[derive(Clone, Debug)]
pub struct Chunk {
    pub id: Uuid,
    pub data: Vec<u8>,
    pub ordinal: i32,
}

impl From<ChunkModel> for Chunk {
    fn from(model: ChunkModel) -> Self {
        Self {
            id: model.id,
            data: model.data,
            ordinal: model.ordinal,
        }
    }
}

// ============================================================================
// Asset Chunk Link Model
// ============================================================================

/// Model for asset_chunks join table result
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetChunkLinkModel {
    pub asset_id: Uuid,
    pub chunk_id: Uuid,
    pub ordinal: i32,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl crate::Keyed for ChunkModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for ChunkModel {
    fn id(&self) -> &Uuid {
        <Self as crate::Keyed>::id(self)
    }
}
