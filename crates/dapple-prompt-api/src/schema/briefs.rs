use crate::schema::brief_units::CreateBriefUnitSchema;
use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// BriefPath schemas (read-only - paths are created with briefs)
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefPathSetSchema {
    ByObjectIds { ids: Vec<Uuid> },
}

impl QueryParamDocs for GetBriefPathSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[ParamDoc {
            name: "ids",
            description: "UUID specific to the brief path object.",
            required: false,
            schema: || param_schema_from::<Vec<Uuid>>(),
        }]
    }
}

// ============================================================================
// Brief schemas (creates path atomically)
// ============================================================================

/// Output mapping for a brief (maps value slots to unit keys for data collection)
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBriefOutputSchema {
    /// Key of the unit this output maps to
    pub unit_key: String,
    /// The value_slot to map this output to
    pub value_slot_id: Uuid,
    /// Where the value comes from: "input", "files", "computed"
    pub source_kind: String,
    /// JSONPath within the consumer payload (e.g., "/value", "/0")
    pub source_path: String,
    /// Optional transform to apply to the value
    #[serde(default)]
    pub transform: serde_json::Value,
    /// Typed display hint (image/text/markdown/list/checklist/table/fields/hidden).
    /// Defaults to `text` when omitted.
    #[serde(default)]
    pub display: Option<String>,
    /// Collapse the rendered block behind a "Reveal" spoiler toggle.
    #[serde(default)]
    pub reveal: bool,
    /// Heading label for the block.
    #[serde(default)]
    pub title: Option<String>,
    /// Sibling-format grouping: outputs sharing a `group` are the same
    /// deliverable in different formats, so a client can render one block with a
    /// format switcher. Opaque token — any stable string; absent ⇒ ungrouped.
    #[serde(default)]
    pub group: Option<String>,
    /// Short label naming this output's format within its `group` (`"html"`,
    /// `"pdf"`, …). Open vocabulary, stored verbatim.
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateBriefSchema {
    /// Name for the brief path (unique identifier)
    pub name: String,
    /// Title of the brief
    pub title: String,
    /// Optional description
    pub description: Option<String>,
    /// Units to create with this brief (optional, can be added later)
    #[serde(default)]
    pub units: Vec<CreateBriefUnitSchema>,
    /// Output mappings for this brief (optional, can be added later)
    #[serde(default)]
    pub outputs: Vec<CreateBriefOutputSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateBriefSetSchema {
    pub objs: Vec<CreateBriefSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefSetSchema {
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

impl QueryParamDocs for GetBriefSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the brief object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "path_ids",
                description: "UUID of brief paths to query all versions.",
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
pub struct PatchBriefSchema {
    /// The id of an existing brief to base the new version on
    pub id: Uuid,
    /// Updated title
    pub title: String,
    /// Updated description
    pub description: Option<String>,
    /// If Some, replace all units. If None (or absent), carry forward from previous version.
    #[serde(default)]
    pub units: Option<Vec<CreateBriefUnitSchema>>,
    /// If Some, replace all outputs. If None (or absent), carry forward from previous version.
    #[serde(default)]
    pub outputs: Option<Vec<CreateBriefOutputSchema>>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchBriefSetSchema {
    pub objs: Vec<PatchBriefSchema>,
}
