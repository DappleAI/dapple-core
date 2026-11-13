use crate::Keyed;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::steps::StepTask;

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedStepTaskModel {
    pub id: Uuid,
    pub step_id: Uuid,
    pub task_id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StepTaskModel {
    pub id: Uuid,
    pub step_id: Uuid,
    pub task_id: Uuid,
    pub created_at: OffsetDateTime,
}

impl From<InsertedStepTaskModel> for StepTaskModel {
    fn from(model: InsertedStepTaskModel) -> Self {
        Self {
            id: model.id,
            step_id: model.step_id,
            task_id: model.task_id,
            created_at: model.created_at,
        }
    }
}

impl Keyed for StepTaskModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl From<StepTaskModel> for StepTask {
    fn from(model: StepTaskModel) -> Self {
        Self {
            id: model.id,
            step: crate::reference::StepReference::from(model.step_id),
            task: crate::reference::TaskReference::from(model.task_id),
            created_at: model.created_at.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertStepTaskArgs {
    pub task_ids: Vec<Uuid>,
}

impl crate::traits::InsertedModel for InsertedStepTaskModel {}
impl crate::Keyed for InsertedStepTaskModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for InsertedStepTaskModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for StepTaskModel {
    fn id(&self) -> &sqlx::types::Uuid {
        <Self as crate::Keyed>::id(self)
    }
}
impl crate::traits::OwnedDependent for InsertedStepTaskModel {
    fn parent_id(&self) -> &sqlx::types::Uuid {
        &self.step_id
    }
}

impl crate::traits::OwnedDependent for StepTaskModel {
    fn parent_id(&self) -> &sqlx::types::Uuid {
        &self.step_id
    }
}
