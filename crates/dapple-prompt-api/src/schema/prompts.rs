use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// PromptPath schemas (read-only - paths are created with prompts)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetPromptPathSetSchema {
    ByObjectIds { ids: Vec<Uuid> },
}

impl QueryParamDocs for GetPromptPathSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[ParamDoc {
            name: "ids",
            description: "UUID specific to the prompt path object.",
            required: false,
            schema: || param_schema_from::<Vec<Uuid>>(),
        }]
    }
}

// ============================================================================
// Prompt schemas (creates path atomically)
// ============================================================================

/// Input for a fluent message association when creating a prompt
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreatePromptFluentMessageSchema {
    /// ID of the fluent message to associate
    pub fluent_message_id: Uuid,
    /// Role for this fluent message (e.g., "system", "user", "assistant")
    pub role: Option<String>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreatePromptSchema {
    /// Name for the prompt path (unique identifier)
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Ordered list of fluent messages to include in this prompt
    #[serde(default)]
    pub fluent_messages: Vec<CreatePromptFluentMessageSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreatePromptSetSchema {
    pub objs: Vec<CreatePromptSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetPromptSetSchema {
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

impl QueryParamDocs for GetPromptSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the prompt object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "path_ids",
                description: "UUID of prompt paths to query all versions.",
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
