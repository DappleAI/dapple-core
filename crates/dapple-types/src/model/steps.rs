use crate::reference::{StepReference, TaskReference};
use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for step task association
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct StepTask {
    pub id: Uuid,
    pub step: StepReference,
    pub task: TaskReference,
    pub created_at: DateTime,
}

/// Public DTO for steps
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Step {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub description: String,
    pub created_at: DateTime,
    pub tasks: Vec<StepTask>,
}

impl DataTransferObject for StepTask {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for Step {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
