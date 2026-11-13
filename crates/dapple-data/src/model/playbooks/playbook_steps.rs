use crate::error::DataError;
use crate::model::playbooks::playbook_tasks::PlaybookTaskModel;
use crate::{Keyed, MergeOrdered};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::playbooks::PlaybookStep;

/// Represents a stage_steps row in the playbook context.
/// The `id` is stage_steps.id, used as the primary identity.
/// `playbook_stage_id` is carried for MergeOrdered grouping under stages.
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedPlaybookStepModel {
    pub id: Uuid,
    pub playbook_stage_id: Uuid,
    pub step_id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlaybookStepModel {
    pub id: Uuid,
    pub playbook_stage_id: Uuid,
    pub step_id: Uuid,
    pub created_at: OffsetDateTime,
    pub tasks: Vec<PlaybookTaskModel>,
}

impl From<InsertedPlaybookStepModel> for PlaybookStepModel {
    fn from(model: InsertedPlaybookStepModel) -> Self {
        Self {
            id: model.id,
            playbook_stage_id: model.playbook_stage_id,
            step_id: model.step_id,
            created_at: model.created_at,
            tasks: vec![],
        }
    }
}

impl MergeOrdered for PlaybookStepModel {
    type Parent = InsertedPlaybookStepModel;
    type ChildRow = PlaybookTaskModel;
    type Child = PlaybookTaskModel;
    type Final = PlaybookStepModel;
    type Error = DataError;

    #[inline]
    fn parent_id(p: &Self::Parent) -> &Uuid {
        &p.id // stage_steps.id
    }

    #[inline]
    fn child_id(c: &Self::ChildRow) -> &Uuid {
        &c.stage_step_id // must match parent's stage_steps.id
    }

    #[inline]
    fn finalize(parent: &Self::Parent, children: Vec<Self::Child>) -> Self::Final {
        PlaybookStepModel {
            id: parent.id,
            playbook_stage_id: parent.playbook_stage_id,
            step_id: parent.step_id,
            created_at: parent.created_at,
            tasks: children,
        }
    }
}

impl Keyed for PlaybookStepModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl From<PlaybookStepModel> for PlaybookStep {
    fn from(model: PlaybookStepModel) -> Self {
        Self {
            id: model.id,
            step: crate::reference::StepReference::from(model.step_id),
            created_at: model.created_at.into(),
            tasks: model.tasks.into_iter().map(|t| t.into()).collect(),
        }
    }
}
