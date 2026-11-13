use crate::error::DataError;
use crate::model::dispatches::dispatch_tasks::{DispatchTaskModel, InsertDispatchTaskArgs};
use crate::{Keyed, MergeOrdered};
use serde::{Deserialize, Serialize};
use sqlx::types::{JsonValue, Uuid};
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::dispatches::DispatchStep;

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedDispatchStepModel {
    pub id: Uuid,
    pub dispatch_id: Uuid,
    pub dispatch_stage_id: Uuid,
    pub stage_step_id: Uuid,
    pub playbook_stage_id: Uuid,
    pub step_id: Uuid,
    pub playbook_id: Uuid,
    pub params: JsonValue,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatDispatchStepModel {
    pub id: Uuid,
    pub dispatch_id: Uuid,
    pub dispatch_stage_id: Uuid,
    pub stage_step_id: Uuid,
    pub playbook_stage_id: Uuid,
    pub step_id: Uuid,
    pub playbook_id: Uuid,
    pub params: JsonValue,
    pub created_at: OffsetDateTime,
    pub dispatch_task_id: Option<Uuid>,
    pub dispatch_task_step_task_id: Option<Uuid>,
    pub dispatch_task_task_id: Option<Uuid>,
    pub dispatch_task_created_at: Option<OffsetDateTime>,
}

impl FlatDispatchStepModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<DispatchStepModel>, DataError> {
        // Preconditions: models ordered by (dispatch_id, step_id NULLS FIRST, task_id NULLS FIRST)

        // Build dispatch parents (dedup by m.id, keep first occurrence)
        let mut parents: Vec<InsertedDispatchStepModel> = Vec::new();
        {
            let mut last = None;
            for m in models.iter() {
                if last != Some(m.id) {
                    parents.push(InsertedDispatchStepModel {
                        id: m.id,
                        dispatch_id: m.dispatch_id,
                        dispatch_stage_id: m.dispatch_stage_id,
                        stage_step_id: m.stage_step_id,
                        playbook_stage_id: m.playbook_stage_id,
                        playbook_id: m.playbook_id,
                        step_id: m.step_id,
                        params: m.params.clone(),
                        created_at: m.created_at,
                    });
                    last = Some(m.id);
                }
            }
        }

        // (c) Build task rows (already final DispatchTaskModel), ordered by step_id
        let task_rows: Vec<DispatchTaskModel> = models
            .iter()
            .filter_map(|m| {
                let id = m.dispatch_task_id?;
                Some(DispatchTaskModel {
                    id,
                    dispatch_step_id: m.id,
                    step_task_id: m.dispatch_task_step_task_id?,
                    task_id: m.dispatch_task_task_id?,
                    created_at: m.dispatch_task_created_at?,
                    status: None,
                })
            })
            .collect();

        // (d) Merge tasks into steps, then steps into dispatches
        let acc: Vec<DispatchStepModel> = DispatchStepModel::merge_ordered(&parents, task_rows)?;

        Ok(acc)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DispatchStepModel {
    pub id: Uuid,
    pub dispatch_id: Uuid,
    pub dispatch_stage_id: Uuid,
    pub stage_step_id: Uuid,
    pub step_id: Uuid,
    pub playbook_id: Uuid,
    pub params: JsonValue,
    pub created_at: OffsetDateTime,
    pub tasks: Vec<DispatchTaskModel>,
}

impl From<InsertedDispatchStepModel> for DispatchStepModel {
    fn from(model: InsertedDispatchStepModel) -> Self {
        Self {
            id: model.id,
            dispatch_id: model.dispatch_id,
            dispatch_stage_id: model.dispatch_stage_id,
            stage_step_id: model.stage_step_id,
            playbook_id: model.playbook_id,
            step_id: model.step_id,
            params: model.params,
            created_at: model.created_at,
            tasks: vec![],
        }
    }
}

impl MergeOrdered for DispatchStepModel {
    type Parent = InsertedDispatchStepModel;
    type ChildRow = DispatchTaskModel;
    type Child = DispatchTaskModel;
    type Final = DispatchStepModel;
    type Error = DataError;

    #[inline]
    fn parent_id(p: &Self::Parent) -> &Uuid {
        &p.id
    }

    #[inline]
    fn child_id(c: &Self::ChildRow) -> &Uuid {
        &c.dispatch_step_id
    }

    #[inline]
    fn finalize(parent: &Self::Parent, children: Vec<Self::Child>) -> Self::Final {
        DispatchStepModel {
            id: parent.id,
            dispatch_id: parent.dispatch_id,
            dispatch_stage_id: parent.dispatch_stage_id,
            stage_step_id: parent.stage_step_id,
            playbook_id: parent.playbook_id,
            step_id: parent.step_id,
            params: parent.params.clone(),
            created_at: parent.created_at,
            tasks: children,
        }
    }
}

impl Keyed for DispatchStepModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl From<DispatchStepModel> for DispatchStep {
    fn from(model: DispatchStepModel) -> Self {
        Self {
            id: model.id,
            dispatch_id: model.dispatch_id,
            step: crate::reference::StepReference::from(model.step_id),
            playbook_id: model.playbook_id,
            params: model.params,
            created_at: model.created_at.into(),
            tasks: model.tasks.into_iter().map(|t| t.into()).collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertDispatchStepsArgs {
    pub stage_step_ids: Vec<Uuid>,
    pub params: Vec<JsonValue>,
    pub tasks: Vec<InsertDispatchTaskArgs>,
}

impl crate::traits::InsertedModel for InsertedDispatchStepModel {}
impl crate::Keyed for InsertedDispatchStepModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for InsertedDispatchStepModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl crate::traits::FlatRelationalObject for FlatDispatchStepModel {
    type ParentInserted = InsertedDispatchStepModel;
    fn to_parent_inserted(&self) -> Self::ParentInserted {
        InsertedDispatchStepModel {
            id: self.id,
            dispatch_id: self.dispatch_id,
            dispatch_stage_id: self.dispatch_stage_id,
            stage_step_id: self.stage_step_id,
            playbook_stage_id: self.playbook_stage_id,
            step_id: self.step_id,
            playbook_id: self.playbook_id,
            params: self.params.clone(),
            created_at: self.created_at,
        }
    }
}
