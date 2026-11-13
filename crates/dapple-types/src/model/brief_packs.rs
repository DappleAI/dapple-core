use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Reference to a brief group within a pack
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefGroupReference {
    pub id: Uuid,
    pub ui_overrides: serde_json::Value,
}

/// Public DTO for brief pack
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefPack {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub pack_spec: serde_json::Value,
    pub groups: Vec<BriefGroupReference>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for brief pack path
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefPackPath {
    pub id: Uuid,
    pub name: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for BriefPack {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
