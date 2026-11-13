use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// FluentMessagePath schemas
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateFluentMessagePathSchema {
    pub name: String,
    pub description: Option<String>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateFluentMessagePathSetSchema {
    pub objs: Vec<CreateFluentMessagePathSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetFluentMessagePathSetSchema {
    ByObjectIds { ids: Vec<Uuid> },
}

impl QueryParamDocs for GetFluentMessagePathSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[ParamDoc {
            name: "ids",
            description: "UUID specific to the fluent message path object.",
            required: false,
            schema: || param_schema_from::<Vec<Uuid>>(),
        }]
    }
}

// ============================================================================
// FluentMessage schemas
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateFluentMessageLocalizedBodySchema {
    pub locale: String,
    pub body: String,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateFluentMessageEngineBodySchema {
    pub lineage_key: String,
    pub provider_key: String,
    pub tier: String,
    pub release: Option<String>,
    pub body: String,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateFluentMessageSchema {
    pub name: String,
    pub description: Option<String>,
    pub localized_bodies: Vec<CreateFluentMessageLocalizedBodySchema>,
    pub engine_bodies: Vec<CreateFluentMessageEngineBodySchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateFluentMessageSetSchema {
    pub objs: Vec<CreateFluentMessageSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetFluentMessageSetSchema {
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

impl QueryParamDocs for GetFluentMessageSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the fluent message object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "path_ids",
                description: "UUID of fluent message paths to query all versions.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "options",
                description: "Query filter options (e.g. `limit`, `page`). Optional — when omitted a default set of filter options will be applied by the server.",
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
pub struct PatchFluentMessageSchema {
    pub id: Uuid,
    pub localized_bodies: Vec<CreateFluentMessageLocalizedBodySchema>,
    pub engine_bodies: Vec<CreateFluentMessageEngineBodySchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchFluentMessageSetSchema {
    pub objs: Vec<PatchFluentMessageSchema>,
}
