use crate::Keyed;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::playbooks::PlaybookTask;

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedPlaybookTasksModel {
    pub id: Uuid,
    pub stage_step_id: Uuid,
    pub task_id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlaybookTaskModel {
    pub id: Uuid,
    pub stage_step_id: Uuid,
    pub task_id: Uuid,
    pub created_at: OffsetDateTime,
}

impl From<InsertedPlaybookTasksModel> for PlaybookTaskModel {
    fn from(model: InsertedPlaybookTasksModel) -> Self {
        Self {
            id: model.id,
            stage_step_id: model.stage_step_id,
            task_id: model.task_id,
            created_at: model.created_at,
        }
    }
}

impl Keyed for PlaybookTaskModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl From<PlaybookTaskModel> for PlaybookTask {
    fn from(model: PlaybookTaskModel) -> Self {
        Self {
            id: model.id,
            task: crate::reference::TaskReference::from(model.task_id),
            created_at: model.created_at.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertPlaybookTaskArgs {
    pub playbook_task_ids: Vec<Uuid>,
}

impl crate::traits::InsertedModel for InsertedPlaybookTasksModel {}
impl crate::Keyed for InsertedPlaybookTasksModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for InsertedPlaybookTasksModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for PlaybookTaskModel {
    fn id(&self) -> &sqlx::types::Uuid {
        <Self as crate::Keyed>::id(self)
    }
}
impl crate::traits::OwnedDependent for InsertedPlaybookTasksModel {
    fn parent_id(&self) -> &sqlx::types::Uuid {
        &self.stage_step_id
    }
}
impl crate::traits::OwnedDependent for PlaybookTaskModel {
    fn parent_id(&self) -> &sqlx::types::Uuid {
        &self.stage_step_id
    }
}
