use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// POST Schemas
// ============================================================================

/// Batch creation request for guides.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateGuideSetSchema {
    pub objs: Vec<CreateGuideSchema>,
}

/// Schema for creating a single guide with its full chapter/block hierarchy.
///
/// Chapters and blocks are included inline because they are owned dependents.
/// The handler creates the entire hierarchy atomically within a transaction.
/// Fluent-native: no locale field -- guide structure is locale-independent.
/// Localized content lives in FluentMessage entities, not inline.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateGuideSchema {
    /// Guide title (becomes path-level data, must be unique across all guides).
    pub title: String,
    /// Optional description of this guide version.
    pub description: Option<String>,
    /// Optional author name.
    pub author: Option<String>,
    /// Soft classification hint: "workflow", "reference", "tutorial", etc.
    /// Not enforced by the database.
    pub classification: Option<String>,
    /// Filesystem path of the guide source (e.g., "docs/books/journey-builder-guide").
    pub source_path: Option<String>,
    /// Ordered list of chapters. Each chapter includes its blocks inline.
    pub chapters: Vec<CreateGuideChapterSchema>,
}

/// Schema for creating a single chapter within a guide.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateGuideChapterSchema {
    /// Chapter title (from SUMMARY.md or chapter heading).
    pub title: String,
    /// Optional subtitle for the chapter (e.g., placeholder text for input fields).
    pub subtitle: Option<String>,
    /// Relative path to the source markdown file within the guide
    /// (e.g., "getting-started/intro.md").
    pub source_file: String,
    /// Optional zero-based index into the parent `chapters` array.
    /// If present, this chapter is a child of `chapters[parent_index]`.
    /// If absent, this is a top-level chapter.
    ///
    /// The index must refer to a chapter that appears *before* this one
    /// in the `chapters` array (topological ordering requirement).
    pub parent_index: Option<usize>,
    /// Ordered list of blocks within this chapter.
    pub blocks: Vec<CreateGuideBlockSchema>,
}

/// Schema for creating a single block within a chapter.
/// Fluent-native: content is optional (None for Fluent-backed blocks),
/// fluent_message_object_id links to localized content.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateGuideBlockSchema {
    /// Directive type key (e.g., "prose", "stage", "step", "nushell",
    /// "brief_pack", "inference_profile", "ref").
    /// Must match a key in the `rev.guide_block_directive_types` lookup table.
    pub directive_type: String,
    /// Optional name from directive metadata (e.g., `:::step { name: "Gather Info" }`).
    pub name: Option<String>,
    /// Raw content within the fence block. Optional under the Fluent-native model:
    /// blocks with localized content have this set to null, with text living in
    /// FluentMessage entities instead.
    pub content: Option<String>,
    /// Parsed key-value attributes from directive metadata.
    /// Defaults to `{}` if omitted.
    pub attributes: Option<serde_json::Value>,
    /// Optional reference to a FluentMessage id for localized content.
    /// When set, the block's text content lives in the referenced FluentMessage.
    pub fluent_message_id: Option<Uuid>,
    /// Tag keys for audience targeting (e.g., ["agent", "product"]).
    /// Must match keys in the `rev.guide_tags` lookup table.
    /// Defaults to empty (no tags).
    pub tags: Option<Vec<String>>,
}

// ============================================================================
// GET Schemas
// ============================================================================

/// Schema for retrieving a single guide by id (path parameter).
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetGuideSchema {
    pub id: Uuid,
}

/// Query parameter schema for the guide index endpoint.
///
/// Guides are a versioned system config entity with slugs, so
/// the required variants are: `ByObjectIds`, `All`, `BySlugs`, `ByPathIds`.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetGuideSetSchema {
    /// Retrieve guides by their version-level object_ids.
    ByObjectIds {
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
    /// Retrieve all guides (system config entity, enumeration allowed).
    /// When `scope` and `environment` are provided, returns the pinned version
    /// per canonical_release_pins; otherwise returns the latest non-deprecated version.
    All {
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
    /// Retrieve guides by slug (path-level unique identifier).
    /// When `scope` and `environment` are provided, returns the pinned version;
    /// otherwise returns the latest non-deprecated version.
    BySlugs {
        slugs: Vec<String>,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
    /// Retrieve all versions of guides by path object_ids.
    ByPathIds {
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
    /// Retrieve guides filtered by classification (e.g., "workflow", "reference").
    /// Returns the latest non-deprecated version per path, like `All`.
    ByClassification {
        classification: String,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
}

impl QueryParamDocs for GetGuideSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "Guide version object IDs (UUIDs).",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "slugs",
                description: "Slug strings to look up guides by their unique slug.",
                required: false,
                schema: || param_schema_from::<Vec<String>>(),
            },
            ParamDoc {
                name: "path_ids",
                description: "Path object IDs to retrieve all versions of specific guides.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "scope",
                description: "Canonical pin scope (e.g. 'common', 'internal'). Used with `environment` to select a pinned guide version.",
                required: false,
                schema: || param_schema_from::<String>(),
            },
            ParamDoc {
                name: "environment",
                description: "Deployment environment (e.g. 'sandbox', 'production'). Used with `scope` to select a pinned guide version.",
                required: false,
                schema: || param_schema_from::<String>(),
            },
            ParamDoc {
                name: "classification",
                description: "Filter guides by classification (e.g., 'workflow', 'reference', 'tutorial').",
                required: false,
                schema: || param_schema_from::<String>(),
            },
            ParamDoc {
                name: "options",
                description: "Query filter options (e.g. `limit`, `page`). Optional -- when omitted a default set of filter options will be applied by the server.",
                required: false,
                schema: || param_schema_from::<FilterOptions>(),
            },
        ]
    }
}

// ============================================================================
// Guide Version Schemas
// ============================================================================

/// Query parameter schema for the guide versions endpoint.
/// Returns lightweight version metadata (no chapters/blocks).
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetGuideVersionsSchema {
    /// Filter by guide slug.
    pub slug: Option<String>,
    /// Filter by guide source_path (e.g., "public/dapple/essay-writing").
    pub source_path: Option<String>,
    #[serde(flatten)]
    pub options: Option<FilterOptions>,
}

// ============================================================================
// Guide Card Schemas
// ============================================================================

/// Query parameter schema for the guide card carousel endpoint.
///
/// Cards are read-only -- they are created as part of guide ingest.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetGuideCardSetSchema {
    /// Retrieve all guide cards (latest version per path, non-deprecated).
    /// When `scope` and `environment` are provided, returns the pinned version's card.
    All {
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
    /// Retrieve guide cards filtered by category.
    /// When `scope` and `environment` are provided, returns the pinned version's card.
    ByCategory {
        category: String,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
    /// Retrieve guide cards filtered by guide classification.
    /// Useful for excluding internal/reference guides from the carousel.
    ByClassification {
        classification: String,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
}

impl QueryParamDocs for GetGuideCardSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "category",
                description: "Filter cards by category (e.g., 'researcher', 'tutorial').",
                required: false,
                schema: || param_schema_from::<String>(),
            },
            ParamDoc {
                name: "classification",
                description: "Filter guide cards by guide classification (e.g., 'workflow', 'reference', 'tutorial').",
                required: false,
                schema: || param_schema_from::<String>(),
            },
            ParamDoc {
                name: "scope",
                description: "Canonical pin scope (e.g. 'common', 'internal'). Used with `environment` to select a pinned guide version.",
                required: false,
                schema: || param_schema_from::<String>(),
            },
            ParamDoc {
                name: "environment",
                description: "Deployment environment (e.g. 'sandbox', 'production'). Used with `scope` to select a pinned guide version.",
                required: false,
                schema: || param_schema_from::<String>(),
            },
            ParamDoc {
                name: "options",
                description: "Query filter options (e.g. `limit`, `page`). Optional -- when omitted a default set of filter options will be applied by the server.",
                required: false,
                schema: || param_schema_from::<FilterOptions>(),
            },
        ]
    }
}
