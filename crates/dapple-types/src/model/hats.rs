use crate::reference::CategoryReference;
use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for hats
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Hat {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub description: String,
    pub created_at: DateTime,
    pub categories: Vec<CategoryReference>,
}

/// Public DTO for hat category association
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct HatCategory {
    pub id: Uuid,
    pub hat_id: Uuid,
    pub category_id: Uuid,
    pub created_at: DateTime,
}

impl DataTransferObject for Hat {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for HatCategory {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
