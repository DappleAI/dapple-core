use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// BriefPackPath schemas (read-only - paths are created with brief packs)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefPackPathSetSchema {
    ByObjectIds { ids: Vec<Uuid> },
}

impl QueryParamDocs for GetBriefPackPathSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[ParamDoc {
            name: "ids",
            description: "UUID specific to the brief pack path object.",
            required: false,
            schema: || param_schema_from::<Vec<Uuid>>(),
        }]
    }
}

// ============================================================================
// BriefPack schemas (creates path atomically)
// ============================================================================

/// Group reference for create/patch operations
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct BriefGroupReferenceSchema {
    /// ID of the brief group
    pub id: Uuid,
    /// Optional UI overrides for this group in this pack
    #[serde(default)]
    pub ui_overrides: serde_json::Value,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateBriefPackSchema {
    /// Name for the brief pack path (unique identifier)
    pub name: String,
    /// Title of the brief pack
    pub title: String,
    /// Optional description
    pub description: Option<String>,
    /// Pack specification (validation modes, experiment tags, etc.)
    #[serde(default)]
    pub pack_spec: serde_json::Value,
    /// Ordered list of group references to include in this pack
    pub groups: Vec<BriefGroupReferenceSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateBriefPackSetSchema {
    pub objs: Vec<CreateBriefPackSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefPackSetSchema {
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

impl QueryParamDocs for GetBriefPackSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the brief pack object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "path_ids",
                description: "UUID of brief pack paths to query all versions.",
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
pub struct PatchBriefPackSchema {
    /// The id of an existing brief pack to base the new version on
    pub id: Uuid,
    /// Updated title
    pub title: String,
    /// Updated description
    pub description: Option<String>,
    /// Updated pack specification
    #[serde(default)]
    pub pack_spec: serde_json::Value,
    /// Ordered list of group references for the new version
    pub groups: Vec<BriefGroupReferenceSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchBriefPackSetSchema {
    pub objs: Vec<PatchBriefPackSchema>,
}
