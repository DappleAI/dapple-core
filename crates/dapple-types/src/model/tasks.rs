use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Reference to a prompt associated with a task (Associative Dependent)
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct TaskPromptRef {
    pub id: Uuid,
    pub prompt_id: Uuid,
    pub prompt_name: String,
}

/// Public DTO for tasks
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Task {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub description: String,
    pub function: String,
    pub function_input: serde_json::Value,
    pub prompts: Vec<TaskPromptRef>,
    pub created_at: DateTime,
}

impl DataTransferObject for TaskPromptRef {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for Task {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
