use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Reference to either a model release or variant
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type")]
pub enum ModelRef {
    #[serde(rename = "release")]
    Release {
        lineage_key: String,
        provider_key: String,
        tier: String,
        release: Option<String>,
    },
    #[serde(rename = "variant")]
    Variant {
        key: String,
        base_lineage_key: String,
        base_provider_key: String,
        base_tier: String,
        base_release: Option<String>,
    },
}

/// Public DTO for inference profile config
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct InferenceProfileConfig {
    pub prompt_path_id: Uuid,
    pub model: ModelRef,
    pub priority: i32,
}

/// Public DTO for inference profile
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct InferenceProfile {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub description: Option<String>,
    pub configs: Vec<InferenceProfileConfig>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Reference to an inference profile attached to a journey
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct JourneyInferenceProfileRef {
    pub inference_profile_id: Uuid,
}

impl DataTransferObject for InferenceProfileConfig {
    fn id(&self) -> &Uuid {
        &self.prompt_path_id
    }
}

impl DataTransferObject for InferenceProfile {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
