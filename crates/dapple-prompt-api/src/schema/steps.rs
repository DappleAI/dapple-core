use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateStepSetSchema {
    pub objs: Vec<CreateStepSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateStepSchema {
    pub name: String,
    pub description: String,
    pub tasks: Vec<CreateStepTaskSchema>,
}

#[derive(Clone, JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateStepTaskSchema {
    pub task_id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetStepSchema {
    id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetStepSetSchema {
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

impl QueryParamDocs for GetStepSetSchema {
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
