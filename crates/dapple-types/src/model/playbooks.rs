use crate::reference::{CategoryReference, StepReference, TaskReference};
use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for playbook category
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PlaybookCategory {
    pub id: Uuid,
    pub playbook_id: Uuid,
    pub category_id: Uuid,
    pub created_at: DateTime,
}

/// Public DTO for playbook task (from step_tasks)
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PlaybookTask {
    pub id: Uuid,
    pub task: TaskReference,
    pub created_at: DateTime,
}

/// Public DTO for playbook step (from stage_steps)
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PlaybookStep {
    pub id: Uuid,
    pub step: StepReference,
    pub created_at: DateTime,
    pub tasks: Vec<PlaybookTask>,
}

/// Public DTO for playbook stage
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PlaybookStage {
    pub id: Uuid,
    pub playbook_id: Uuid,
    pub steps: Vec<PlaybookStep>,
    pub created_at: DateTime,
}

/// Public DTO for playbooks
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Playbook {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub description: String,
    pub created_at: DateTime,
    pub stages: Vec<PlaybookStage>,
    pub categories: Vec<CategoryReference>,
}

impl DataTransferObject for PlaybookCategory {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for PlaybookTask {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for PlaybookStep {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for PlaybookStage {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for Playbook {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
