use crate::error::DataError;
use crate::{DateTime, Keyed, MergeOrdered};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::dispatches::DispatchStage;

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedDispatchStageModel {
    // Returned columns from insert_dispatch_stages.sql (objs.* plus joins)
    pub id: Uuid,
    pub dispatch_id: Uuid,
    pub playbook_stage_id: Uuid,
    pub params: serde_json::Value,
    pub created_at: OffsetDateTime,
    // Additional joined fields (may be absent in some SELECTs)
    pub stage_id: Option<Uuid>,
    pub playbook_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DispatchStageModel {
    pub id: Uuid,
    pub dispatch_id: Uuid,
    pub playbook_stage_id: Uuid,
    pub params: serde_json::Value,
    pub created_at: DateTime,
    pub steps: Vec<crate::model::dispatches::dispatch_steps::DispatchStepModel>,
}

impl From<DispatchStageModel> for DispatchStage {
    fn from(model: DispatchStageModel) -> Self {
        Self {
            id: model.id,
            dispatch_id: model.dispatch_id,
            params: model.params,
            created_at: model.created_at,
            steps: model.steps.into_iter().map(|s| s.into()).collect(),
        }
    }
}

// Insert args include nested steps so the stage insert can pass step args downwards
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertDispatchStagesArgs {
    pub playbook_stage_ids: Vec<Uuid>,
    pub params: Vec<serde_json::Value>,
    pub steps: Vec<crate::model::dispatches::dispatch_steps::InsertDispatchStepsArgs>,
}

impl crate::traits::InsertedModel for InsertedDispatchStageModel {}
impl Keyed for InsertedDispatchStageModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for InsertedDispatchStageModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for DispatchStageModel {
    fn id(&self) -> &sqlx::types::Uuid {
        <Self as Keyed>::id(self)
    }
}

impl Keyed for DispatchStageModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::PrincipalObject for DispatchStageModel {
    fn display_name(&self) -> Option<&str> {
        None
    }
}

impl MergeOrdered for DispatchStageModel {
    type Parent = InsertedDispatchStageModel;
    type ChildRow = crate::model::dispatches::dispatch_steps::DispatchStepModel;
    type Child = crate::model::dispatches::dispatch_steps::DispatchStepModel;
    type Final = DispatchStageModel;
    type Error = DataError;

    #[inline]
    fn parent_id(p: &Self::Parent) -> &Uuid {
        &p.id
    }

    #[inline]
    fn child_id(c: &Self::ChildRow) -> &Uuid {
        &c.dispatch_stage_id
    }

    #[inline]
    fn finalize(parent: &Self::Parent, children: Vec<Self::Child>) -> Self::Final {
        DispatchStageModel {
            id: parent.id,
            dispatch_id: parent.dispatch_id,
            playbook_stage_id: parent.playbook_stage_id,
            params: parent.params.clone(),
            created_at: parent.created_at.into(),
            steps: children,
        }
    }
}
