use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::{FilterOptions, JsonObject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use uuid::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateStageSetSchema {
    pub objs: Vec<CreateStageSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateStageSchema {
    pub playbook_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub params: JsonValue,
    pub steps: Vec<CreateStageStepSchema>,
}

#[derive(JsonSchema, Clone, Serialize, Deserialize, Debug)]
pub struct CreateStageStepSchema {
    pub stage_step_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub params: JsonValue,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetStageSchema {
    id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetStageSetSchema {
    ByObjectIds {
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
    BySlugs {
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    },
}

impl QueryParamDocs for GetStageSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "slugs",
                description: "Slug strings to look up entities by their unique slug.",
                required: false,
                schema: || param_schema_from::<Vec<String>>(),
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
