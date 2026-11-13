use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ============================================================================
// DirectivePath schemas (read-only - paths are created with directives)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDirectivePathSetSchema {
    ByObjectIds { ids: Vec<Uuid> },
}

impl QueryParamDocs for GetDirectivePathSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[ParamDoc {
            name: "ids",
            description: "UUID specific to the directive path object.",
            required: false,
            schema: || param_schema_from::<Vec<Uuid>>(),
        }]
    }
}

// ============================================================================
// Directive schemas (creates path atomically)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDirectiveSchema {
    /// Name for the directive path (unique identifier)
    pub name: String,
    /// JSONLogic expression for conditional execution
    pub logic: Value,
    /// Maximum retry attempts (default: 1)
    pub max_attempts: Option<i32>,
    /// Delay in milliseconds between retry attempts
    pub retry_delay_ms: Option<i32>,
    /// Optional description
    pub description: Option<String>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDirectiveSetSchema {
    pub objs: Vec<CreateDirectiveSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDirectiveSetSchema {
    ByObjectIds {
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

impl QueryParamDocs for GetDirectiveSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the directive object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "path_ids",
                description: "UUID of directive paths to query all versions.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "options",
                description: "Query filter options (e.g. `limit`, `page`).",
                required: false,
                schema: || param_schema_from::<FilterOptions>(),
            },
        ]
    }
}

// ============================================================================
// PATCH schema
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchDirectiveSchema {
    /// The id of an existing directive to base the new version on
    pub id: Uuid,
    /// Updated JSONLogic expression
    pub logic: Value,
    /// Updated maximum retry attempts
    pub max_attempts: Option<i32>,
    /// Updated retry delay in milliseconds
    pub retry_delay_ms: Option<i32>,
    /// Updated description
    pub description: Option<String>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchDirectiveSetSchema {
    pub objs: Vec<PatchDirectiveSchema>,
}
