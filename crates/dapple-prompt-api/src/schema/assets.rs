use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// AssetPath schemas (read-only - paths are created with assets)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetAssetPathSetSchema {
    ByObjectIds { ids: Vec<Uuid> },
}

impl QueryParamDocs for GetAssetPathSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[ParamDoc {
            name: "ids",
            description: "UUID specific to the asset path object.",
            required: false,
            schema: || param_schema_from::<Vec<Uuid>>(),
        }]
    }
}

// ============================================================================
// Asset schemas (creates path atomically)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateAssetSchema {
    /// Asset kind key (e.g. "image", "document", "audio", "video", "binary")
    pub asset_kind: String,
    /// Base64-encoded content
    pub contents: String,
    /// MIME type of the content (e.g. "image/png", "application/pdf")
    /// If not provided, defaults to "application/octet-stream"
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateAssetSetSchema {
    pub objs: Vec<CreateAssetSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetAssetSetSchema {
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

impl QueryParamDocs for GetAssetSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the asset object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "path_ids",
                description: "UUID of asset paths to query all versions.",
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
pub struct PatchAssetSchema {
    pub id: Uuid,
    /// Base64-encoded content
    pub contents: String,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchAssetSetSchema {
    pub objs: Vec<PatchAssetSchema>,
}
