use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for directive
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq, Eq)]
pub struct Directive {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub logic: serde_json::Value,
    pub max_attempts: i32,
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for directive path
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct DirectivePath {
    pub id: Uuid,
    pub name: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for directive reference (embedded in PlaybookStage/PlaybookStep)
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct DirectiveRef {
    pub id: Uuid,
}

/// Public DTO for stage directive attachment
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct PlaybookStageDirective {
    pub id: Uuid,
    pub playbook_stage_id: Uuid,
    pub directive: DirectiveRef,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for step directive attachment (from stage_step_directives)
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct PlaybookStepDirective {
    pub id: Uuid,
    pub stage_step_id: Uuid,
    pub directive: DirectiveRef,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for Directive {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
