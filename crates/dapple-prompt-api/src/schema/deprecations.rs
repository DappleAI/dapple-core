use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Schema for creating a deprecation event
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDeprecationSchema {
    /// The object_id of the versioned object to deprecate
    pub versioned_object_id: Uuid,
    /// The new deprecation level (deprecated, retired, quarantined)
    pub to_level: String,
    /// Optional reason for the deprecation
    pub reason: Option<String>,
}

/// Schema for creating multiple deprecation events
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDeprecationSetSchema {
    pub objs: Vec<CreateDeprecationSchema>,
}

/// Schema for getting deprecation events
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDeprecationSetSchema {
    /// Get deprecation events by versioned object IDs
    ByVersionedObjectIds {
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
    /// Get current deprecation status for versioned objects
    StatusByObjectIds { ids: Vec<Uuid> },
}

impl QueryParamDocs for GetDeprecationSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUIDs of versioned objects to query.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "options",
                description: "Query filter options (e.g. `limit`, `page`). Optional.",
                required: false,
                schema: || param_schema_from::<FilterOptions>(),
            },
        ]
    }
}

/// Schema for DELETE endpoint - deprecate a single object
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct DeleteObjectSchema {
    /// The object_id of the entity to deprecate
    pub id: Uuid,
    /// Optional reason for the deprecation
    pub reason: Option<String>,
}

/// Schema for batch DELETE endpoint
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct DeleteObjectSetSchema {
    pub objs: Vec<DeleteObjectSchema>,
}

/// Query parameters for DELETE endpoint
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct DeleteQueryParams {
    /// Deprecation level: "deprecated" (default), "retired", or "quarantined"
    #[serde(default = "default_deprecation_level")]
    pub level: String,
    /// Optional reason for the deprecation
    pub reason: Option<String>,
}

fn default_deprecation_level() -> String {
    "deprecated".to_string()
}

impl QueryParamDocs for DeleteQueryParams {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "level",
                description: "Deprecation level: 'deprecated' (default), 'retired', or 'quarantined'.",
                required: false,
                schema: || param_schema_from::<String>(),
            },
            ParamDoc {
                name: "reason",
                description: "Optional reason for the deprecation.",
                required: false,
                schema: || param_schema_from::<Option<String>>(),
            },
        ]
    }
}

/// Response schema showing current deprecation status
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct DeprecationStatusResponse {
    /// The versioned object's id
    pub versioned_object_id: Uuid,
    /// The path id
    pub path_id: Uuid,
    /// The entity type (task, step, playbook, etc.)
    pub entity_type: String,
    /// Current version
    pub version: String,
    /// Current deprecation level (None means active)
    pub deprecation_level: Option<String>,
    /// Timestamp of last deprecation event
    pub last_deprecation_at: Option<String>,
}
