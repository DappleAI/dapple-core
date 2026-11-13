// filepath: /home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/schema/hats.rs
use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateHatSetSchema {
    pub objs: Vec<CreateHatSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateHatSchema {
    pub name: String,
    pub description: String,
    pub categories: Vec<uuid::Uuid>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetHatSchema {
    id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetHatSetSchema {
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

impl QueryParamDocs for GetHatSetSchema {
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
