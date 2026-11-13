use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::asset_content_embeddings::*;

// ============================================================================
// Asset Content Embedding Models
// ============================================================================

/// Model returned from INSERT...RETURNING.
///
/// Uses `sqlx::FromRow` because this type is used with `sqlx::query_as()`
/// (runtime binding) rather than `query_file_as!()` due to pgvector type
/// handling. The raw vector column is not returned from the insert SQL.
#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct AssetContentEmbeddingModel {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub extraction_id: Uuid,
    pub chunk_index: i32,
    pub created_at: OffsetDateTime,
}

/// Search result row from a pgvector similarity query.
///
/// Uses `sqlx::FromRow` because this type is used with `sqlx::query_as()`
/// (runtime binding) rather than `query_file_as!()` due to pgvector type
/// handling.
#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct AssetEmbeddingSearchResult {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub extraction_id: Uuid,
    pub chunk_index: i32,
    pub chunk_text: String,
    pub source_position: serde_json::Value,
    pub metadata: serde_json::Value,
    pub similarity: f64,
}

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for asset content embeddings.
/// All vectors MUST be the same length.
#[derive(Clone, Debug, Default)]
pub struct InsertAssetContentEmbeddingArgs {
    pub asset_ids: Vec<Uuid>,
    pub extraction_ids: Vec<Uuid>,
    pub chunk_indices: Vec<i32>,
    pub chunk_texts: Vec<String>,
    pub embeddings: Vec<Vec<f32>>,
    pub source_positions: Vec<serde_json::Value>,
    pub metadatas: Vec<serde_json::Value>,
}

impl crate::InsertColumnArgs for InsertAssetContentEmbeddingArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, crate::error::DataError> {
        Ok(vec![
            ("asset_ids", self.asset_ids.len()),
            ("extraction_ids", self.extraction_ids.len()),
            ("chunk_indices", self.chunk_indices.len()),
            ("chunk_texts", self.chunk_texts.len()),
            ("embeddings", self.embeddings.len()),
            ("source_positions", self.source_positions.len()),
            ("metadatas", self.metadatas.len()),
        ])
    }
}
