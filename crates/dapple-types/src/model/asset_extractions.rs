use crate::{DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for an asset extraction record.
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct AssetExtraction {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub extractor_kind: String,
    pub extractor_version: i32,
    pub status: String,
    pub text_length: Option<i64>,
    pub page_count: Option<i32>,
    #[schemars(with = "crate::JsonObject")]
    pub metadata: serde_json::Value,
    pub error_message: Option<String>,
    pub is_current: bool,
    #[schemars(with = "String")]
    pub created_at: DateTime,
    #[schemars(with = "Option<String>")]
    pub completed_at: Option<DateTime>,
}

impl DataTransferObject for AssetExtraction {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
