use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for asset (no contents - use chunk APIs to retrieve content)
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Asset {
    pub id: Uuid,
    pub path_id: Uuid,
    pub asset_kind: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    /// Provenance: link to source draft (if created via draft workflow)
    pub draft_id: Option<Uuid>,
    /// Total content length in bytes
    pub content_length: i64,
    /// MIME type of the content
    pub mime_type: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for asset path
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct AssetPath {
    pub id: Uuid,
    pub asset_kind: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for Asset {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
