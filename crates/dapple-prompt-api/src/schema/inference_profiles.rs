use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateInferenceProfileSetSchema {
    pub objs: Vec<CreateInferenceProfileSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateInferenceProfileSchema {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub configs: Vec<CreateInferenceProfileConfigSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateInferenceProfileConfigSchema {
    pub prompt_path_id: Uuid,
    pub model_release_id: Option<Uuid>,
    pub model_variant_id: Option<Uuid>,
    pub priority: i32,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetInferenceProfileSetSchema {
    ByObjectIds {
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

impl QueryParamDocs for GetInferenceProfileSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the object.",
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
