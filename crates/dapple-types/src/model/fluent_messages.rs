use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for fluent message localized body
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct FluentMessageLocalizedBody {
    pub locale: String,
    pub body: String,
}

/// Public DTO for fluent message engine body
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct FluentMessageEngineBody {
    pub locale: String,
    /// The lineage key (prompting paradigm)
    pub lineage_key: String,
    /// Provider key (where the model is hosted)
    pub provider_key: String,
    /// The tier within the lineage (e.g., sonnet, opus, ljspeech)
    pub tier: String,
    /// The release version (e.g., 20241022, v0.1)
    pub release: Option<String>,
    pub body: String,
}

/// Public DTO for fluent message
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct FluentMessage {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub localized_bodies: Vec<FluentMessageLocalizedBody>,
    pub engine_bodies: Vec<FluentMessageEngineBody>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for fluent message path
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct FluentMessagePath {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for FluentMessage {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
