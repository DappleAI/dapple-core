pub mod dispatch_stages;
pub mod dispatch_steps;
pub mod dispatch_tasks;

use crate::error::DataError;
use crate::error::ModelColumnMismatchError::ChildWithoutParent;
use crate::model::dispatches::dispatch_stages::{
    DispatchStageModel, InsertDispatchStagesArgs, InsertedDispatchStageModel,
};
use crate::model::dispatches::dispatch_steps::{DispatchStepModel, InsertedDispatchStepModel};
use crate::model::dispatches::dispatch_tasks::DispatchTaskModel;
use crate::{DateTime, InsertColumnArgs, Keyed, MergeOrdered};
use serde::{Deserialize, Serialize};
use sqlx::types::{JsonValue, Uuid};
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::dispatches::*;

/// Represents a completed task output from a prior dispatch in the same journey.
/// Used for journey context seeding in `execute_dispatch()`.
#[derive(Debug, Deserialize, Serialize)]
pub struct CompletedTaskContext {
    pub dispatch_id: Uuid,
    pub task_id: Uuid,
    pub task_name: String,
    pub function: String,
    pub function_input: JsonValue,
    pub context_data: JsonValue,
    pub created_at: OffsetDateTime,
}

/// Result of resolving stage_step_ids to their parent playbook_stage_ids.
/// Used by the handler to derive dispatch stages from caller-supplied steps.
#[derive(Debug, Deserialize, Serialize)]
pub struct ResolvedPlaybookStepStage {
    pub ord: Option<i64>,
    pub stage_step_id: Uuid,
    pub playbook_stage_id: Uuid,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedDispatchModel {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub intent: JsonValue,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatDispatchModel {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub intent: JsonValue,
    pub created_at: OffsetDateTime,
    pub dispatch_step_id: Option<Uuid>,
    pub dispatch_step_stage_step_id: Option<Uuid>,
    pub dispatch_step_playbook_stage_id: Option<Uuid>,
    pub dispatch_step_step_id: Option<Uuid>,
    pub dispatch_step_playbook_id: Option<Uuid>,
    pub dispatch_step_params: Option<JsonValue>,
    pub dispatch_step_created_at: Option<OffsetDateTime>,
    pub dispatch_stage_id: Option<Uuid>,
    pub dispatch_stage_playbook_stage_id: Option<Uuid>,
    pub dispatch_stage_stage_id: Option<Uuid>,
    pub dispatch_stage_playbook_id: Option<Uuid>,
    pub dispatch_stage_params: Option<JsonValue>,
    pub dispatch_stage_created_at: Option<OffsetDateTime>,
    pub dispatch_task_id: Option<Uuid>,
    pub dispatch_task_step_task_id: Option<Uuid>,
    pub dispatch_task_task_id: Option<Uuid>,
    pub dispatch_task_created_at: Option<OffsetDateTime>,
    pub dispatch_task_status: Option<String>,
}

impl FlatDispatchModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<DispatchModel>, DataError> {
        // Preconditions: models ordered by (dispatch_id, stage_id NULLS FIRST, step_id NULLS FIRST, task_id NULLS FIRST)

        // Build dispatch parents (dedup by id, keep first occurrence)
        let mut parents: Vec<InsertedDispatchModel> = Vec::new();
        {
            let mut last: Option<Uuid> = None;
            for m in models.iter() {
                if last != Some(m.id) {
                    parents.push(InsertedDispatchModel {
                        id: m.id,
                        journey_id: m.journey_id,
                        intent: m.intent.clone(),
                        created_at: m.created_at,
                    });
                    last = Some(m.id);
                }
            }
        }

        // (b) Build stage heads (ordered, dedup by dispatch_stage_id)
        let mut stage_heads: Vec<InsertedDispatchStageModel> = Vec::new();
        {
            let mut last: Option<Uuid> = None;
            for m in models.iter() {
                let Some(stage_id) = m.dispatch_stage_id else {
                    continue;
                };
                if last == Some(stage_id) {
                    continue;
                }
                last = Some(stage_id);
                stage_heads.push(InsertedDispatchStageModel {
                    id: stage_id,
                    dispatch_id: m.id,
                    playbook_stage_id: m.dispatch_stage_playbook_stage_id.ok_or_else(|| {
                        ChildWithoutParent {
                            child_key: format!("stage({stage_id})"),
                        }
                    })?,
                    params: m
                        .dispatch_stage_params
                        .clone()
                        .ok_or_else(|| ChildWithoutParent {
                            child_key: format!("stage({stage_id})"),
                        })?,
                    created_at: m
                        .dispatch_stage_created_at
                        .ok_or_else(|| ChildWithoutParent {
                            child_key: format!("stage({stage_id})"),
                        })?,
                    stage_id: None,
                    playbook_id: None,
                });
            }
        }

        // (c) Build step heads (ordered, dedup by dispatch_step_id)
        let mut step_heads: Vec<InsertedDispatchStepModel> = Vec::new();
        {
            let mut last: Option<Uuid> = None;
            for m in models.iter() {
                let Some(step_id) = m.dispatch_step_id else {
                    continue;
                };
                if last == Some(step_id) {
                    continue;
                }
                last = Some(step_id);
                step_heads.push(InsertedDispatchStepModel {
                    id: step_id,
                    dispatch_id: m.id,
                    dispatch_stage_id: m.dispatch_stage_id.ok_or_else(|| ChildWithoutParent {
                        child_key: format!("step({step_id})"),
                    })?,
                    stage_step_id: m.dispatch_step_stage_step_id.ok_or_else(|| {
                        ChildWithoutParent {
                            child_key: format!("step({step_id})"),
                        }
                    })?,
                    playbook_stage_id: m.dispatch_step_playbook_stage_id.ok_or_else(|| {
                        ChildWithoutParent {
                            child_key: format!("step({step_id})"),
                        }
                    })?,
                    step_id: m.dispatch_step_step_id.ok_or_else(|| ChildWithoutParent {
                        child_key: format!("step({step_id})"),
                    })?,
                    playbook_id: m
                        .dispatch_step_playbook_id
                        .ok_or_else(|| ChildWithoutParent {
                            child_key: format!("step({step_id})"),
                        })?,
                    params: m
                        .dispatch_step_params
                        .clone()
                        .ok_or_else(|| ChildWithoutParent {
                            child_key: format!("step({step_id})"),
                        })?,
                    created_at: m
                        .dispatch_step_created_at
                        .ok_or_else(|| ChildWithoutParent {
                            child_key: format!("step({step_id})"),
                        })?,
                });
            }
        }

        // (d) Build task rows (already final DispatchTaskModel), ordered by step_id
        let task_rows: Vec<DispatchTaskModel> = models
            .iter()
            .filter_map(|m| {
                let id = m.dispatch_task_id?;
                Some(DispatchTaskModel {
                    id,
                    dispatch_step_id: m.dispatch_step_id?,
                    step_task_id: m.dispatch_task_step_task_id?,
                    task_id: m.dispatch_task_task_id?,
                    created_at: m.dispatch_task_created_at?,
                    status: m.dispatch_task_status.clone(),
                })
            })
            .collect();

        // (d) Merge tasks into steps
        let steps: Vec<DispatchStepModel> =
            DispatchStepModel::merge_ordered(&step_heads, task_rows)?;

        // (e) Merge steps into stages
        let stages: Vec<DispatchStageModel> =
            DispatchStageModel::merge_ordered(&stage_heads, steps)?;

        // (f) Merge stages into dispatches
        let dispatches: Vec<DispatchModel> = DispatchModel::merge_ordered(&parents, stages)?;

        Ok(dispatches)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DispatchModel {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub intent: JsonValue,
    pub created_at: DateTime,
    pub stages: Vec<DispatchStageModel>,
}

impl MergeOrdered for DispatchModel {
    type Parent = InsertedDispatchModel;
    // Children are already "structured" (each stage already has its steps)
    type ChildRow = DispatchStageModel;
    type Child = DispatchStageModel; // From<ChildRow> is the identity here
    type Final = DispatchModel;
    type Error = DataError;

    #[inline]
    fn parent_id(p: &Self::Parent) -> &Uuid {
        &p.id
    }

    #[inline]
    fn child_id(c: &Self::ChildRow) -> &Uuid {
        &c.dispatch_id
    }

    #[inline]
    fn finalize(parent: &Self::Parent, children: Vec<Self::Child>) -> Self::Final {
        DispatchModel {
            id: parent.id,
            journey_id: parent.journey_id,
            intent: parent.intent.clone(),
            created_at: parent.created_at.into(),
            stages: children,
        }
    }
}

impl Keyed for DispatchModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl From<DispatchModel> for Dispatch {
    fn from(model: DispatchModel) -> Self {
        Self {
            id: model.id,
            journey_id: model.journey_id,
            intent: model.intent,
            created_at: model.created_at,
            stages: model.stages.into_iter().map(|s| s.into()).collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertDispatchArgs {
    pub intents: Vec<JsonValue>,
    pub stages: Vec<InsertDispatchStagesArgs>,
}

impl InsertColumnArgs for InsertDispatchArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![("intents", self.intents.len())])
    }
}

/// A pure-compute "no-input" step the runtime should auto-dispatch (see
/// `query::dispatches::select_pending_autodispatch_steps_for_journey`). The
/// `step_task_ids` are the step's tasks in authored order, ready to drop into one
/// `InsertDispatchArgs` (one stage / one step / its tasks).
#[derive(Clone, Debug)]
pub struct PendingAutodispatchStepModel {
    pub playbook_stage_id: Uuid,
    pub stage_step_id: Uuid,
    pub step_task_ids: Vec<Uuid>,
}

impl crate::traits::InsertedModel for InsertedDispatchModel {}
impl crate::Keyed for InsertedDispatchModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for InsertedDispatchModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for DispatchModel {
    fn id(&self) -> &Uuid {
        <Self as crate::Keyed>::id(self)
    }
}

impl crate::traits::PrincipalObject for DispatchModel {
    fn display_name(&self) -> Option<&str> {
        None
    }
}

impl crate::traits::FlatRelationalObject for FlatDispatchModel {
    type ParentInserted = InsertedDispatchModel;
    fn to_parent_inserted(&self) -> Self::ParentInserted {
        InsertedDispatchModel {
            id: self.id,
            journey_id: self.journey_id,
            intent: self.intent.clone(),
            created_at: self.created_at,
        }
    }
}
