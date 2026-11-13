//! Schema types for the canonical manifest sync and query endpoints.

use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// POST /api/canonical/sync
// ============================================================================

/// Request body for canonical manifest sync.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CanonicalSyncRequestSchema {
    /// The scope this manifest governs (must match metadata.scope in the TOML).
    pub scope: String,
    /// Raw TOML content of the canonical manifest file.
    pub toml_content: String,
}

/// Query parameters for the canonical sync endpoint.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct CanonicalSyncQueryParams {
    /// When true, parse and validate only; return summary without persisting.
    #[serde(default)]
    pub dry_run: bool,
}

/// Successful canonical sync response.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CanonicalSyncResponse {
    pub status: String,
    pub scope: String,
    /// Total number of pins processed.
    pub total_pins: usize,
    /// Number of pins resolved to an existing versioned object.
    pub resolved: usize,
    /// Number of pins that could not be resolved (version not ingested).
    pub unresolved: usize,
    /// Warnings from parsing or resolution.
    pub warnings: Vec<String>,
    /// Details about unresolved pins.
    pub unresolved_pins: Vec<UnresolvedPinDetail>,
}

/// Detail about a pin that could not be resolved.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct UnresolvedPinDetail {
    pub environment: String,
    pub object_type: String,
    pub ref_key: String,
    pub version: String,
    pub reason: String,
}

/// Error response from the canonical sync endpoint.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CanonicalSyncErrorResponse {
    pub error: String,
    pub message: String,
}

// ============================================================================
// GET /api/canonical/
// ============================================================================

/// Query parameter schema for the canonical pins index endpoint.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetCanonicalPinSetSchema {
    /// Retrieve all canonical release pins.
    All { options: Option<FilterOptions> },
    /// Retrieve canonical release pins for a specific scope and environment.
    ByEnvironment {
        scope: String,
        environment: String,
        options: Option<FilterOptions>,
    },
    /// Retrieve specific canonical release pins by object_id.
    ByObjectIds {
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
}

impl QueryParamDocs for GetCanonicalPinSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "Canonical pin object IDs (UUIDs).",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "scope",
                description: "Manifest scope (e.g., 'common', 'internal').",
                required: false,
                schema: || param_schema_from::<String>(),
            },
            ParamDoc {
                name: "environment",
                description: "Deployment environment (e.g., 'sandbox', 'production').",
                required: false,
                schema: || param_schema_from::<String>(),
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
