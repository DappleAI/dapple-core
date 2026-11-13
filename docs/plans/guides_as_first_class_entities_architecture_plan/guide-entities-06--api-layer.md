# Issue 6: Guide API Layer (Handlers, Routes, Schemas)

**Status:** Draft
**Created:** 2026-02-13
**Dependencies:** Issue 4 (Models), Issue 5 (SQL Queries)
**Blocks:** Issue 7 (Lifecycle Tests)

---

## 1. Summary

Create the complete API surface for guides in `dapple-prompt-api`: request/response schemas, Axum handlers, and route registration. Guides are a unified versioned entity with a deep ownership hierarchy (guide -> chapters -> blocks -> tags), so the API follows the Hat pattern for the main CRUD operations while introducing a richer `CreateGuideSchema` that includes the full chapter/block/tag tree.

This issue creates:
1. **Schemas** (`schema/guides.rs`) -- `CreateGuideSetSchema`, `CreateGuideSchema`, `GetGuideSetSchema`, `GetGuideSchema`
2. **Handlers** (`handler/guides.rs`) -- index, post, get-one, delete (batch + single)
3. **Routes** (`route/guides.rs`) -- `router()` function wired into `build_api_router`
4. **Module registrations** -- additions to `handler.rs`, `schema.rs`, `route.rs`

No PATCH handler is included in this initial issue. Version creation via PATCH will be added in a follow-up when the ingestion pipeline (Phase 3) needs it. The POST handler creates version 0.0.1 with full chapter/block content.

---

## 2. File Inventory

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-prompt-api/src/schema/guides.rs` | Create | Request/response schemas |
| `crates/dapple-prompt-api/src/handler/guides.rs` | Create | Index, POST, GET, DELETE handlers |
| `crates/dapple-prompt-api/src/route/guides.rs` | Create | Router function with OpenAPI docs |
| `crates/dapple-prompt-api/src/schema.rs` | Modify | Add `pub mod guides;` |
| `crates/dapple-prompt-api/src/handler.rs` | Modify | Add `pub(crate) mod guides;` |
| `crates/dapple-prompt-api/src/route.rs` | Modify | Add `mod guides;` and nest route |

---

## 3. Schemas

### File: `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/schema/guides.rs`

```rust
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
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateGuideSchema {
    /// Guide title (becomes path-level data, must be unique across all guides).
    pub title: String,
    /// Optional description of this guide version.
    pub description: Option<String>,
    /// Locale code (e.g., "en", "es"). Defaults to "en" if omitted.
    pub locale: Option<String>,
    /// Optional author name.
    pub author: Option<String>,
    /// Soft classification hint: "workflow", "reference", "tutorial", etc.
    /// Not enforced by the database.
    pub classification: Option<String>,
    /// Filesystem path of the guide source (e.g., "docs/guides/journey-builder-guide").
    pub source_path: Option<String>,
    /// Ordered list of chapters. Each chapter includes its blocks inline.
    pub chapters: Vec<CreateGuideChapterSchema>,
}

/// Schema for creating a single chapter within a guide.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateGuideChapterSchema {
    /// Chapter title (from SUMMARY.md or chapter heading).
    pub title: String,
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
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateGuideBlockSchema {
    /// Directive type key (e.g., "prose", "stage", "step", "nushell",
    /// "brief_pack", "inference_profile", "ref").
    /// Must match a key in the `rev.guide_block_directive_types` lookup table.
    pub directive_type: String,
    /// Optional name from directive metadata (e.g., `:::step { name: "Gather Info" }`).
    pub name: Option<String>,
    /// Raw content within the fence block.
    pub content: String,
    /// Parsed key-value attributes from directive metadata.
    /// Defaults to `{}` if omitted.
    pub attributes: Option<serde_json::Value>,
    /// Tag keys for audience targeting (e.g., ["agent", "product"]).
    /// Must match keys in the `rev.guide_tags` lookup table.
    /// Defaults to empty (no tags).
    pub tags: Option<Vec<String>>,
}

// ============================================================================
// GET Schemas
// ============================================================================

/// Schema for retrieving a single guide by object_id (path parameter).
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetGuideSchema {
    pub object_id: Uuid,
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
    All {
        options: Option<FilterOptions>,
    },
    /// Retrieve guides by slug (path-level unique identifier).
    BySlugs {
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    },
    /// Retrieve all versions of guides by path object_ids.
    ByPathIds {
        path_ids: Vec<Uuid>,
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
                name: "options",
                description: "Query filter options (e.g. `limit`, `page`). Optional -- when omitted a default set of filter options will be applied by the server.",
                required: false,
                schema: || param_schema_from::<FilterOptions>(),
            },
        ]
    }
}
```

### Schema Design Notes

- **`CreateGuideSchema` includes the full hierarchy inline** because chapters, blocks, and block tags are all owned dependents. There are no separate `/api/guide-chapters/` or `/api/guide-blocks/` endpoints.
- **`parent_index`** uses a zero-based index into the `chapters` array rather than an `object_id` reference. This is because at creation time the chapter object_ids do not yet exist. The handler resolves these indexes to internal IDs after the chapter INSERT.
- **`locale` defaults to `"en"`** at the SQL level (`COALESCE(args.locale, 'en')`), so the schema makes it `Option<String>`.
- **`attributes` defaults to `{}`** at the SQL level, so the schema makes it `Option<serde_json::Value>`.
- **`tags` defaults to empty** (no tags), so the schema makes it `Option<Vec<String>>`.
- **`GetGuideSetSchema` includes `ByPathIds`** because guides are versioned -- callers need to retrieve all versions of a guide by its path identifier.

---

## 4. Handlers

### File: `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/handler/guides.rs`

```rust
use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::guides::{CreateGuideSetSchema, GetGuideSetSchema};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::model::guides::Guide;
use dapple_data::query::guides::{
    DeprecateGuidesArgs, deprecate_guides, insert_guides, select_guide_by_object_id,
    select_guides, select_guides_by_object_ids, select_guides_by_path_ids,
    select_guides_by_slugs, InsertGuideWithChildrenArgs, InsertGuideChapterWithBlocksArgs,
    InsertGuideBlockWithTagsArgs,
};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// GET /api/guides/ (index)
// ============================================================================

pub async fn guides_index_handler(
    opts: Query<GetGuideSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetGuideSetSchema::ByObjectIds { ids, options } => {
            select_guides_by_object_ids(&data.db, ids, options.unwrap_or_default()).await
        }
        GetGuideSetSchema::All { options } => {
            select_guides(&data.db, options.unwrap_or_default()).await
        }
        GetGuideSetSchema::BySlugs { slugs, options } => {
            select_guides_by_slugs(&data.db, slugs, options.unwrap_or_default()).await
        }
        GetGuideSetSchema::ByPathIds { path_ids, options } => {
            select_guides_by_path_ids(&data.db, path_ids, options.unwrap_or_default()).await
        }
    };

    index_response_handler("guides", objs)
}

// ============================================================================
// POST /api/guides/
// ============================================================================

pub async fn guides_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateGuideSetSchema>,
) -> impl IntoApiResponse {
    // Build InsertGuideWithChildrenArgs from the schema.
    // This converts the schema's string keys (directive_type, tags)
    // into the format expected by the query layer. Key-to-ID resolution
    // happens inside the query function's transaction.
    let args: Vec<InsertGuideWithChildrenArgs> = body
        .objs
        .into_iter()
        .map(|s| InsertGuideWithChildrenArgs {
            title: s.title,
            description: s.description,
            locale: s.locale.unwrap_or_else(|| "en".to_string()),
            author: s.author,
            classification: s.classification,
            source_path: s.source_path,
            chapters: s
                .chapters
                .into_iter()
                .map(|ch| InsertGuideChapterWithBlocksArgs {
                    title: ch.title,
                    source_file: ch.source_file,
                    parent_index: ch.parent_index,
                    blocks: ch
                        .blocks
                        .into_iter()
                        .map(|blk| InsertGuideBlockWithTagsArgs {
                            directive_type_key: blk.directive_type,
                            name: blk.name,
                            content: blk.content,
                            attributes: blk.attributes.unwrap_or_default(),
                            tag_keys: blk.tags.unwrap_or_default(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();

    post_response_handler("guides", insert_guides(&data.db, args).await)
}

// ============================================================================
// GET /api/guides/{id}
// ============================================================================

pub async fn guides_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler("guide", select_guide_by_object_id(&data.db, id).await)
}

// ============================================================================
// DELETE /api/guides/ (batch deprecation)
// ============================================================================

pub async fn guides_delete_handler(
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<DeleteObjectSetSchema>,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let path_object_ids: Vec<Uuid> = body.objs.iter().map(|o| o.object_id).collect();
        let reasons: Vec<Option<String>> = body
            .objs
            .iter()
            .map(|o| o.reason.clone().or_else(|| params.reason.clone()))
            .collect();

        let args = DeprecateGuidesArgs {
            path_object_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_guides(&data.db, args).await
    }
    .await;

    delete_response_handler("guides", result)
}

// ============================================================================
// DELETE /api/guides/{id} (single deprecation)
// ============================================================================

pub async fn guides_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateGuidesArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_guides(&data.db, args).await
    }
    .await;

    delete_response_handler("guide", result)
}
```

### Handler Design Notes

- **`guides_index_handler`** follows the exact same pattern as `hats_index_handler`. All four `GetGuideSetSchema` variants are dispatched to the appropriate query function. The `index_response_handler` wraps the result in the standard `IndexResponse<Guide>` envelope.
- **`guides_post_handler`** is simpler than the hat handler because guides have no shared dependent references to validate (no `validate_references_not_blocked` call). Chapters, blocks, and tags are all owned dependents created atomically.
- **Schema-to-args conversion** happens entirely in the handler. The `InsertGuideWithChildrenArgs` struct is defined in Issue 5 (query module). The handler maps schema string keys (`directive_type`, `tags`) directly through -- key-to-ID resolution happens inside the SQL transaction.
- **The `locale` field** defaults to `"en"` in the handler (not left to SQL alone), so that the `InsertGuideWithChildrenArgs` always has a concrete value. The SQL also has a `COALESCE` fallback as a safety net.
- **The `attributes` field** defaults to `serde_json::Value::Object(Default::default())` (empty JSON object) when `None` is provided in the schema.
- **DELETE handlers** follow the exact hat deprecation pattern: resolve the deprecation level by key, build `DeprecateGuidesArgs`, call `deprecate_guides`, return deprecation results through `delete_response_handler`.
- **No PATCH handler yet.** PATCH (creating new versions) involves re-ingestion of the full chapter/block hierarchy, which requires the ingestion pipeline. For the initial API, only POST (create first version) and DELETE (deprecate) are needed.

---

## 5. Routes

### File: `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/route/guides.rs`

```rust
use crate::AppState;
use crate::handler::guides::{
    guides_delete_handler, guides_delete_one_handler, guides_get_handler,
    guides_index_handler, guides_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    IndexResponse, PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::guides::GetGuideSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::guides::Guide;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(guides_post_handler, |op| {
                op.description("Create one or more guides with chapters, blocks, and tags")
                    .tag("Guides")
                    .response::<201, Json<PostResponse<Guide>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(guides_index_handler, index_docs))
        .api_route(
            "/{id}",
            get_with(guides_get_handler, |op| {
                op.description("Retrieve a specific guide by version object ID")
                    .tag("Guides")
                    .response::<200, Json<GetResponse<Guide>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(guides_delete_handler, |op| {
                op.id("delete_guides")
                    .summary("Delete guides")
                    .description("Deprecate a batch of guides by path object ID")
                    .tag("Guides")
                    .response::<200, Json<DeleteResponse<Guide>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            delete_with(guides_delete_one_handler, |op| {
                op.id("delete_guide")
                    .summary("Delete a guide")
                    .description("Deprecate a single guide by path object ID")
                    .tag("Guides")
                    .response::<200, Json<DeleteResponse<Guide>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("guidesIndex")
        .description("Query guides.")
        .tag("Guides")
        .response::<200, Json<IndexResponse<Guide>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetGuideSetSchema::apply_query_param_docs(op)
}
```

---

## 6. Module Registrations

### 6.1 Handler module

**File:** `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/handler.rs`

Add the following line in alphabetical order among the existing handler module declarations (after `pub(crate) mod fluent_messages;`):

```rust
pub(crate) mod guides;
```

### 6.2 Schema module

**File:** `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/schema.rs`

Add the following line in alphabetical order among the existing schema module declarations (after `pub mod fluent_messages;`):

```rust
pub mod guides;
```

### 6.3 Route module

**File:** `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/route.rs`

**Step 1:** Add module declaration (after `mod fluent_messages;`):

```rust
mod guides;
```

**Step 2:** Add route nesting in `build_api_router` (after the `.nest("/api/drafts/", drafts::drafts_router())` line, or in alphabetical order among the simple entity routes):

```rust
.nest("/api/guides/", guides::router())
```

**Step 3:** Add the same nesting to `build_spec_router` in the same location:

```rust
.nest("/api/guides/", guides::router())
```

### Exact insertion points in `route.rs`

In `build_api_router`, add after the directives routes and before the deprecations route:

```rust
        .nest("/api/directives/", directives::directives_router())
        .nest("/api/guides/", guides::router())     // <-- ADD THIS LINE
        .nest("/api/deprecations/", deprecations::router())
```

In `build_spec_router`, add in the same relative position:

```rust
        .nest("/api/directives/", directives::directives_router())
        .nest("/api/guides/", guides::router())     // <-- ADD THIS LINE
        .nest("/api/deprecations/", deprecations::router())
```

---

## 7. API Contract Summary

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| `POST` | `/api/guides/` | `guides_post_handler` | Create guides with chapters/blocks/tags |
| `GET` | `/api/guides/` | `guides_index_handler` | Query guides (by_object_ids, all, by_slugs, by_path_ids) |
| `GET` | `/api/guides/{id}` | `guides_get_handler` | Get single guide by version object_id |
| `DELETE` | `/api/guides/` | `guides_delete_handler` | Batch deprecation by path object_ids |
| `DELETE` | `/api/guides/{id}` | `guides_delete_one_handler` | Single deprecation by path object_id |

### Request/Response Examples

**POST /api/guides/**

```json
{
  "objs": [
    {
      "title": "Journey Builder Guide",
      "description": "A comprehensive guide to building journeys",
      "locale": "en",
      "author": "Engineering Team",
      "classification": "workflow",
      "source_path": "docs/guides/journey-builder-guide",
      "chapters": [
        {
          "title": "Introduction",
          "source_file": "intro.md",
          "parent_index": null,
          "blocks": [
            {
              "directive_type": "prose",
              "name": null,
              "content": "Welcome to the journey builder guide.",
              "attributes": {},
              "tags": ["universal"]
            }
          ]
        },
        {
          "title": "Getting Started",
          "source_file": "getting-started.md",
          "parent_index": null,
          "blocks": [
            {
              "directive_type": "stage",
              "name": "Setup",
              "content": "Initial setup instructions.",
              "attributes": {"priority": "high"},
              "tags": ["agent", "product"]
            },
            {
              "directive_type": "nushell",
              "name": "install_deps",
              "content": "cargo install dapple",
              "attributes": {},
              "tags": ["coder"]
            }
          ]
        },
        {
          "title": "Setup Details",
          "source_file": "getting-started/setup-details.md",
          "parent_index": 1,
          "blocks": []
        }
      ]
    }
  ]
}
```

**Response (201 Created):**

```json
{
  "status": "success",
  "results": 1,
  "objs": [
    {
      "object_id": "01941234-5678-7abc-def0-123456789abc",
      "path_object_id": "01941234-5678-7abc-def0-000000000001",
      "title": "Journey Builder Guide",
      "slug": "journey-builder-guide",
      "version": "0.0.1",
      "deprecation_level": null,
      "description": "A comprehensive guide to building journeys",
      "locale": "en",
      "author": "Engineering Team",
      "classification": "workflow",
      "source_path": "docs/guides/journey-builder-guide",
      "chapters": [
        {
          "object_id": "01941234-5678-7abc-def0-aaaaaaaaaaaa",
          "parent_chapter_object_id": null,
          "title": "Introduction",
          "source_file": "intro.md",
          "blocks": [
            {
              "object_id": "01941234-5678-7abc-def0-bbbbbbbbbbbb",
              "directive_type": "prose",
              "name": null,
              "content": "Welcome to the journey builder guide.",
              "attributes": {},
              "tags": ["universal"],
              "created_at": "2026-02-14T00:00:00Z"
            }
          ],
          "created_at": "2026-02-14T00:00:00Z"
        }
      ],
      "created_at": "2026-02-14T00:00:00Z"
    }
  ]
}
```

**GET /api/guides/?query=by_slugs&slugs[0]=journey-builder-guide**

Returns `IndexResponse<Guide>` with the same guide structure.

**DELETE /api/guides/?level=deprecated**

```json
{
  "objs": [
    {
      "object_id": "01941234-5678-7abc-def0-000000000001",
      "reason": "Replaced by v2"
    }
  ]
}
```

---

## 8. Differences from the Hat Pattern

While guides follow the Hat pattern closely, there are notable differences:

| Aspect | Hats | Guides |
|--------|------|--------|
| **Shared Dependents** | Categories (validated via `validate_references_not_blocked`) | None |
| **CreateSchema depth** | Flat (name, description, categories[]) | Nested (chapters[blocks[tags]]) |
| **Insert mechanism** | Single SQL query + associative insert | Transaction with multiple queries (guide -> chapters -> blocks -> tags) |
| **PATCH handler** | Not present (future) | Not present (future, via ingestion pipeline) |
| **GET variants** | ByObjectIds, All, BySlugs | ByObjectIds, All, BySlugs, ByPathIds |
| **Handler complexity** | Simple args mapping | Nested args mapping with defaults |

---

## 9. Input Validation

The handler should validate the following before calling the query layer:

### Validations performed in the handler

1. **Non-empty batch:** `body.objs` must not be empty.
2. **Parent index bounds:** For each chapter with `parent_index = Some(i)`, verify `i < chapter_index` (must reference a chapter that appears earlier in the array). This ensures topological ordering.
3. **Non-empty title:** Each guide must have a non-empty `title` string.

### Validations deferred to the query layer (SQL/transaction)

1. **Unique title/slug:** Enforced by `UNIQUE` constraints on `rev.guide_path_details`. A duplicate title results in a 409 Conflict (via `data_error_to_status_code` detecting the unique constraint violation).
2. **Valid directive_type_key:** Resolved from the lookup table within the transaction. An unknown key results in a `DataError::MissingIdLookup` which maps to 422 Unprocessable Entity.
3. **Valid tag_keys:** Same resolution pattern as directive types.

### Validation helper (optional)

```rust
fn validate_create_guide_schema(schema: &CreateGuideSetSchema) -> Result<(), DataError> {
    if schema.objs.is_empty() {
        return Err(DataError::InvalidInput("Empty guide batch".into()));
    }

    for guide in &schema.objs {
        if guide.title.trim().is_empty() {
            return Err(DataError::InvalidInput("Guide title cannot be empty".into()));
        }

        for (idx, chapter) in guide.chapters.iter().enumerate() {
            if let Some(parent_idx) = chapter.parent_index {
                if parent_idx >= idx {
                    return Err(DataError::InvalidInput(format!(
                        "Chapter {} has parent_index {} which must be less than its own index",
                        idx, parent_idx
                    )));
                }
            }
        }
    }

    Ok(())
}
```

If validation is implemented, it should be called at the top of `guides_post_handler` before building args:

```rust
pub async fn guides_post_handler(...) -> impl IntoApiResponse {
    if let Err(e) = validate_create_guide_schema(&body) {
        return post_response_handler::<Guide>("guides", Err(e));
    }

    // ... proceed with args building and insert ...
}
```

---

## 10. Verification Steps

1. **Compilation:** `cargo build -p dapple-prompt-api` compiles cleanly after adding all files.
2. **OpenAPI docs:** Visit `/docs` and verify the "Guides" tag appears with all five operations.
3. **POST creates guide:** POST a guide with 2 chapters and 3 blocks. Verify 201 response with chapters and blocks.
4. **GET index retrieves:** Use `?query=all` to verify the created guide appears.
5. **GET by slug:** Use `?query=by_slugs&slugs[0]=...` to retrieve by slug.
6. **GET single:** Use `/api/guides/{object_id}` to retrieve the specific version.
7. **DELETE deprecates:** DELETE with `?level=deprecated` and verify the guide's `deprecation_level` is set.
8. **Unique title conflict:** POST two guides with the same title and verify 409 Conflict.
9. **Invalid directive_type:** POST with an unknown `directive_type` key and verify 422 Unprocessable Entity.

---

## 11. Notes for Implementation Agent

- **Follow the Hat handler as your primary reference.** The file at `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/handler/hats.rs` is the template. Match its import structure, error handling patterns, and response handler usage exactly.
- **The `ByPathIds` variant is new** compared to hats (which only have `ByObjectIds`, `All`, `BySlugs`). This is because guides are versioned and callers will need to retrieve version histories by path. The SQL query is `select_guides_by_path_ids.sql` from Issue 5.
- **Do not create separate endpoints for chapters, blocks, or tags.** These are owned dependents of the guide and are always created/returned as part of the guide.
- **The query functions from Issue 5 return `Guide` DTOs directly** (not `GuideModel`). The `insert_guides` function handles the transaction internally and converts `GuideModel` to `Guide` before returning. This means the handler does not need to call `.into()` on the results.
- **Do not add a PATCH route yet.** PATCH requires re-creating the entire chapter/block hierarchy for the new version, which is the ingestion pipeline's responsibility. It will be added in a follow-up issue.
- **The `select_guides_by_path_ids` function signature** should accept a `FilterOptions` parameter even though the architecture plan Issue 5 did not include it. Add `filter_ops: FilterOptions` and wire it through to the SQL's LIMIT/OFFSET.
- **Run `cargo sqlx prepare --workspace`** after implementing all handlers to ensure the offline cache is updated.
