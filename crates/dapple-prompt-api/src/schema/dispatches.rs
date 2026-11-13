use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::{FilterOptions, JsonObject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use uuid::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDispatchSetSchema {
    pub objs: Vec<CreateDispatchSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDispatchSchema {
    pub journey_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub intent: JsonValue,
    pub steps: Vec<CreateDispatchStepSchema>,
}

#[derive(JsonSchema, Clone, Serialize, Deserialize, Debug)]
pub struct CreateDispatchStepSchema {
    pub stage_step_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub params: JsonValue,
    pub tasks: Vec<CreateDispatchTaskSchema>,
}

#[derive(JsonSchema, Clone, Serialize, Deserialize, Debug)]
pub struct CreateDispatchTaskSchema {
    pub step_task_id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDispatchSetSchema {
    ByUser {
        user_id: Uuid,
        options: Option<FilterOptions>,
    },
    ByPlaybook {
        playbook_id: Uuid,
        options: Option<FilterOptions>,
    },
    ByJourney {
        journey_id: Uuid,
        options: Option<FilterOptions>,
    },
}

impl QueryParamDocs for GetDispatchSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "user_id",
                description: "UUID specific to the user.",
                required: true,
                schema: || param_schema_from::<Uuid>(),
            },
            ParamDoc {
                name: "journey_id",
                description: "UUID of the journey to filter dispatches by.",
                required: false,
                schema: || param_schema_from::<Uuid>(),
            },
            ParamDoc {
                name: "options",
                description: "Query filter options (e.g. `limit`, `page`). Optional.",
                required: false,
                schema: || param_schema_from::<FilterOptions>(),
            },
        ]
    }
}

// ============================================================================
// Task context endpoint DTOs
// ============================================================================
//
// Response shape for `GET /api/dispatches/{id}/task_context/{task_id}`. The
// handler returns a single structured payload aggregating:
//
//   - Current journey binding state (slot name, value_kind, truncated preview)
//   - Asset extraction availability per bound asset
//   - The task's position in the dispatch (stage/step/task indices)
//   - Prior task outcomes within the same dispatch (MVP stub)
//   - Brief context attached to this task via chapter linkage (MVP stub)
//
// Used by the `task context` Nushell command so an executing agent can warm
// its prior-context cache without re-issuing multiple API calls.

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct TaskContextResponse {
    pub dispatch_id: Uuid,
    pub task_id: Uuid,
    /// Bindings visible to the conduit for this task. When the task's
    /// stage→step→task cascade carries ANY overlays, this is blinded to the
    /// overlay-exposed subset; a cascade with no overlays leaves the full
    /// journey pool visible (blinders are opt-in per node).
    pub bindings: Vec<TaskContextBinding>,
    pub assets: Vec<TaskContextAsset>,
    pub dispatch_position: TaskContextDispatchPosition,
    pub prior_tasks: Vec<TaskContextPriorTask>,
    pub brief_context: Vec<TaskContextBrief>,
    /// Raw CONDUITS.md markdown of the journey's pinned guide version — the
    /// conduit's guide frame. `None` for journeys without a guide pin or
    /// versions ingested before the frame existed.
    #[serde(default)]
    pub guide_frame: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct TaskContextBinding {
    pub slot_name: String,
    pub value_kind: String,
    /// Truncated preview of the bound value. String values are truncated to
    /// 500 characters with an ellipsis marker; other scalars pass through
    /// unchanged.
    #[schemars(with = "JsonObject")]
    pub value_preview: JsonValue,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct TaskContextAsset {
    pub asset_id: Uuid,
    pub extraction_kind: String,
    pub status: String,
    pub page_count: Option<i32>,
    pub text_length: Option<i64>,
    /// Nushell command the agent can invoke to retrieve the extracted text.
    pub retrieval_command: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct TaskContextDispatchPosition {
    pub stage_index: i32,
    pub stage_name: Option<String>,
    pub step_index: i32,
    pub step_name: Option<String>,
    pub task_index: i32,
    pub task_name: Option<String>,
    /// Authored stage description (journey-pinned version, fluent-resolved).
    #[serde(default)]
    pub stage_description: Option<String>,
    /// Authored step description (journey-pinned version, fluent-resolved).
    #[serde(default)]
    pub step_description: Option<String>,
    /// Authored stage ordinal within the pinned playbook (`playbook_stages.ord`).
    #[serde(default)]
    pub stage_ord: Option<i32>,
    /// Total stages in the pinned playbook.
    #[serde(default)]
    pub total_stages: Option<i64>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct TaskContextPriorTask {
    pub task_id: Uuid,
    pub task_name: Option<String>,
    pub status: String,
    pub summary: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct TaskContextBrief {
    pub brief_id: Uuid,
    pub brief_name: Option<String>,
    pub units: Vec<TaskContextBriefUnit>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct TaskContextBriefUnit {
    pub unit_kind: String,
    pub name: Option<String>,
    #[schemars(with = "Option<JsonObject>")]
    pub submitted_value: Option<JsonValue>,
}
