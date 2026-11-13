use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Reference to a fluent message within a prompt (includes role)
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct PromptFluentMessageReference {
    pub fluent_message_id: Uuid,
    pub role: Option<String>,
    pub ordinal: i32,
}

/// Public DTO for prompts
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Prompt {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub description: Option<String>,
    pub fluent_messages: Vec<PromptFluentMessageReference>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for prompt path
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct PromptPath {
    pub id: Uuid,
    pub name: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for Prompt {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
