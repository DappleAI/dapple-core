use crate::{DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for an asset content embedding row (no raw vector).
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct AssetContentEmbedding {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub extraction_id: Uuid,
    pub chunk_index: i32,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for AssetContentEmbedding {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Public DTO for a single asset search hit.
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct AssetEmbeddingSearchResult {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub extraction_id: Uuid,
    pub chunk_index: i32,
    pub chunk_text: String,
    #[schemars(with = "crate::JsonObject")]
    pub source_position: serde_json::Value,
    #[schemars(with = "crate::JsonObject")]
    pub metadata: serde_json::Value,
    pub similarity: f64,
}
