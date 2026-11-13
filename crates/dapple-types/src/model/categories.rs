use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for categories
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Category {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub description: String,
    pub created_at: DateTime,
}

impl DataTransferObject for Category {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
