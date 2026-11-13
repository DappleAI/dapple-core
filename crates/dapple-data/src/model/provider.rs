//! Provider models for API/hosting endpoints
//!
//! Providers represent where models are hosted and how to access them.
//! The same lineage (e.g., Mistral-7B) can be hosted by different providers
//! (Mistral AI, self-hosted, NVIDIA NIM).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::DateTime;

/// Internal model for provider from database query
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderModel {
    pub id: i16,
    pub key: String,
    pub display_name: Option<String>,
    pub api_config: Option<serde_json::Value>,
    pub created_at: OffsetDateTime,
}

/// Public DTO for provider
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Provider {
    pub id: i16,
    pub key: String,
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_config: Option<serde_json::Value>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<ProviderModel> for Provider {
    fn from(model: ProviderModel) -> Self {
        Self {
            id: model.id,
            key: model.key,
            display_name: model.display_name,
            api_config: model.api_config,
            created_at: model.created_at.into(),
        }
    }
}
