use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// ValueSlotPath schemas (read-only - paths are created with value slots)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetValueSlotPathSetSchema {
    ByObjectIds { ids: Vec<Uuid> },
}

impl QueryParamDocs for GetValueSlotPathSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[ParamDoc {
            name: "ids",
            description: "UUID specific to the value slot path object.",
            required: false,
            schema: || param_schema_from::<Vec<Uuid>>(),
        }]
    }
}

// ============================================================================
// ValueSlot schemas (creates path atomically)
// ============================================================================

fn default_cardinality() -> String {
    "single".to_string()
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateValueSlotSchema {
    /// Name for the value slot path
    pub name: String,
    /// Value kind key (e.g. "text", "int", "number", "bool", "json", "asset")
    pub value_kind: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub constraints: serde_json::Value,
    /// `"single"` (default) or `"multi"` (keep-N subset of N typed items).
    #[serde(default = "default_cardinality")]
    pub cardinality: String,
    /// For `multi`: minimum items to keep (NULL = no floor; `0` allows none).
    #[serde(default)]
    pub min_selections: Option<i32>,
    /// For `multi`: maximum items to keep (NULL = unbounded).
    #[serde(default)]
    pub max_selections: Option<i32>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateValueSlotSetSchema {
    pub objs: Vec<CreateValueSlotSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetValueSlotSetSchema {
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

impl QueryParamDocs for GetValueSlotSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the value slot object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "path_ids",
                description: "UUID of value slot paths to query all versions.",
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
pub struct PatchValueSlotSchema {
    pub id: Uuid,
    /// Value kind key (e.g. "text", "int", "number", "bool", "json", "asset").
    /// Versioned (migration 70): a kind change forks a new value-slot version.
    pub value_kind: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub constraints: serde_json::Value,
    /// `"single"` (default) or `"multi"` (keep-N subset of N typed items).
    #[serde(default = "default_cardinality")]
    pub cardinality: String,
    /// For `multi`: minimum items to keep (NULL = no floor; `0` allows none).
    #[serde(default)]
    pub min_selections: Option<i32>,
    /// For `multi`: maximum items to keep (NULL = unbounded).
    #[serde(default)]
    pub max_selections: Option<i32>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchValueSlotSetSchema {
    pub objs: Vec<PatchValueSlotSchema>,
}
