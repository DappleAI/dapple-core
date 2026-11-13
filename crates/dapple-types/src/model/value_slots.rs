use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for value slot
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct ValueSlot {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub value_kind: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub constraints: serde_json::Value,
    /// `"single"` (exactly one item, the default) or `"multi"` (a keep-N
    /// subset of N typed items of this slot's `value_kind`).
    pub cardinality: String,
    /// For `multi` slots: minimum items the operator must keep (NULL = no
    /// floor; `0` allows keeping none). Always NULL for `single`.
    pub min_selections: Option<i32>,
    /// For `multi` slots: maximum items the operator may keep (NULL =
    /// unbounded). Always NULL for `single`.
    pub max_selections: Option<i32>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for value slot path
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct ValueSlotPath {
    pub id: Uuid,
    pub name: String,
    pub value_kind: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for ValueSlot {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
