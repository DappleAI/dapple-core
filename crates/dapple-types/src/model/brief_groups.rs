use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Reference to a brief within a group
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefReference {
    pub id: Uuid,
}

/// Public DTO for brief group
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefGroup {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub briefs: Vec<BriefReference>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for brief group path
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefGroupPath {
    pub id: Uuid,
    pub name: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for BriefGroup {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
