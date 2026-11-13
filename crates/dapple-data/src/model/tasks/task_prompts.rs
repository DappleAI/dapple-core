use crate::{DateTime, Keyed};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedTaskPromptModel {
    pub id: Uuid,
    pub task_id: Uuid,
    pub prompt_id: Uuid,
    pub prompt_name: String, // From prompt_paths.name via join
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskPromptModel {
    pub id: Uuid,
    pub task_id: Uuid,
    pub prompt_id: Uuid,
    pub prompt_name: String, // From prompt_paths.name via join
    pub created_at: OffsetDateTime,
}

impl From<InsertedTaskPromptModel> for TaskPromptModel {
    fn from(model: InsertedTaskPromptModel) -> Self {
        Self {
            id: model.id,
            task_id: model.task_id,
            prompt_id: model.prompt_id,
            prompt_name: model.prompt_name,
            created_at: model.created_at,
        }
    }
}

impl Keyed for TaskPromptModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct TaskPrompt {
    pub id: Uuid,
    pub task: crate::reference::TaskReference,
    pub prompt: crate::reference::PromptReference,
    pub prompt_name: String, // From prompt_paths.name via join
    pub created_at: DateTime,
}

impl From<TaskPromptModel> for TaskPrompt {
    fn from(model: TaskPromptModel) -> Self {
        Self {
            id: model.id,
            task: crate::reference::TaskReference::from(model.task_id),
            prompt: crate::reference::PromptReference::from(model.prompt_id),
            prompt_name: model.prompt_name,
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for task_prompts
/// All arrays must be the same length - each tuple represents one task_prompt link
/// Note: prompt_name is derived from prompt_paths.name via SQL join, not passed here
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertTaskPromptsArgs {
    pub task_ids: Vec<Uuid>,
    pub prompt_ids: Vec<Uuid>,
    // No local_names - prompt_name comes from prompt_paths.name via join
}

impl_keyed!(InsertedTaskPromptModel, inserted);

impl crate::traits::OwnedDependent for InsertedTaskPromptModel {
    fn parent_id(&self) -> &Uuid {
        &self.task_id
    }
}

impl crate::traits::RelationalObject for TaskPromptModel {
    fn id(&self) -> &Uuid {
        <Self as crate::Keyed>::id(self)
    }
}

impl crate::traits::OwnedDependent for TaskPromptModel {
    fn parent_id(&self) -> &Uuid {
        &self.task_id
    }
}

impl crate::traits::DataTransferObject for TaskPrompt {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
