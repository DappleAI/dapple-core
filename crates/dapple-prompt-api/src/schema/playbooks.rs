use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::{FilterOptions, JsonObject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use uuid::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreatePlaybookSetSchema {
    pub objs: Vec<CreatePlaybookSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreatePlaybookSchema {
    pub name: String,
    pub description: String,
    pub stages: Vec<CreatePlaybookStageSchema>,
    pub categories: Vec<uuid::Uuid>,
}

#[derive(JsonSchema, Clone, Serialize, Deserialize, Debug)]
pub struct CreatePlaybookStageSchema {
    pub steps: Vec<CreatePlaybookStageStepSchema>,
}

#[derive(JsonSchema, Clone, Serialize, Deserialize, Debug)]
pub struct CreatePlaybookStageStepSchema {
    pub step_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub params: JsonValue,
    pub tasks: Vec<CreatePlaybookTaskSchema>,
}

#[derive(JsonSchema, Clone, Serialize, Deserialize, Debug)]
pub struct CreatePlaybookTaskSchema {
    pub task_id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetPlaybookSchema {
    id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetPlaybookSetSchema {
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

impl QueryParamDocs for GetPlaybookSetSchema {
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
