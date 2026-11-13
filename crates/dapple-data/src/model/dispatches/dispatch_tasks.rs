use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::dispatches::DispatchTask;

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedDispatchTasksModel {
    pub id: Uuid,
    pub dispatch_step_id: Uuid,
    pub step_task_id: Uuid,
    pub task_id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DispatchTaskModel {
    pub id: Uuid,
    pub dispatch_step_id: Uuid,
    pub step_task_id: Uuid,
    pub task_id: Uuid,
    pub created_at: OffsetDateTime,
    pub status: Option<String>,
}

impl From<InsertedDispatchTasksModel> for DispatchTaskModel {
    fn from(model: InsertedDispatchTasksModel) -> Self {
        Self {
            id: model.id,
            dispatch_step_id: model.dispatch_step_id,
            step_task_id: model.step_task_id,
            task_id: model.task_id,
            created_at: model.created_at,
            status: None,
        }
    }
}

impl crate::traits::InsertedModel for InsertedDispatchTasksModel {}
impl crate::Keyed for InsertedDispatchTasksModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for InsertedDispatchTasksModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl crate::Keyed for DispatchTaskModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for DispatchTaskModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl From<DispatchTaskModel> for DispatchTask {
    fn from(model: DispatchTaskModel) -> Self {
        Self {
            id: model.id,
            dispatch_step: crate::reference::StepReference::from(model.dispatch_step_id),
            task: crate::reference::TaskReference::from(model.task_id),
            created_at: model.created_at.into(),
            status: model.status,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertDispatchTaskArgs {
    pub step_task_ids: Vec<Uuid>,
}

impl crate::traits::OwnedDependent for InsertedDispatchTasksModel {
    fn parent_id(&self) -> &sqlx::types::Uuid {
        &self.dispatch_step_id
    }
}
impl crate::traits::OwnedDependent for DispatchTaskModel {
    fn parent_id(&self) -> &sqlx::types::Uuid {
        &self.dispatch_step_id
    }
}
