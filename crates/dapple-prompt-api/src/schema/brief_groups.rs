use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// BriefGroupPath schemas (read-only - paths are created with brief groups)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefGroupPathSetSchema {
    ByObjectIds { ids: Vec<Uuid> },
}

impl QueryParamDocs for GetBriefGroupPathSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[ParamDoc {
            name: "ids",
            description: "UUID specific to the brief group path object.",
            required: false,
            schema: || param_schema_from::<Vec<Uuid>>(),
        }]
    }
}

// ============================================================================
// BriefGroup schemas (creates path atomically)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateBriefGroupSchema {
    /// Name for the brief group path (unique identifier)
    pub name: String,
    /// Title of the brief group
    pub title: String,
    /// Optional description
    pub description: Option<String>,
    /// Ordered list of brief IDs to include in this group
    pub brief_ids: Vec<Uuid>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateBriefGroupSetSchema {
    pub objs: Vec<CreateBriefGroupSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefGroupSetSchema {
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

impl QueryParamDocs for GetBriefGroupSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the brief group object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "path_ids",
                description: "UUID of brief group paths to query all versions.",
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
pub struct PatchBriefGroupSchema {
    /// The id of an existing brief group to base the new version on
    pub id: Uuid,
    /// Updated title
    pub title: String,
    /// Updated description
    pub description: Option<String>,
    /// Ordered list of brief IDs for the new version
    pub brief_ids: Vec<Uuid>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchBriefGroupSetSchema {
    pub objs: Vec<PatchBriefGroupSchema>,
}
