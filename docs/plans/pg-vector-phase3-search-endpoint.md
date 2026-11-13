# pgvector Phase 3: Search Endpoint & Classification Filtering

**Status:** Draft
**Created:** 2026-02-24
**Last Updated:** 2026-02-24
**Parent Plan:** `docs/plans/pg-vector-searching.md` (sections 4a, 6)
**Dependencies:** Phase 2 (migration 16, embedding pipeline, search SQL)

---

## Executive Summary

Phase 2 delivered the write path: embedding generation triggered on guide ingest, with
embeddings, tags, and locale associations stored in pgvector. Phase 3 delivers the
read path: a search endpoint that accepts a natural-language query, embeds it via
Conduit, runs pgvector similarity search, and returns ranked results.

This plan chooses **Option B** from the task brief: the API owns search. The API calls
Conduit's existing `/embed` endpoint to generate a query embedding, then runs the
similarity SQL directly against its own `DbPool`. This avoids duplicating database
access in Conduit, keeps all query endpoints in one service, and reuses the existing
`EmbeddingClient` infrastructure from Phase 2.

Additionally, this plan adds `ByClassification` query variants to both the guides
index and guide cards endpoints, allowing the UI to filter out internal/reference
guides from the explore carousel.

---

## 1. Architecture Decision: API Owns Search (Option B)

### Why Not Conduit (Option A)?

Conduit has a `DbPool` (confirmed in `dapple-prompt-conduit/src/service/mod.rs`), so
Option A is technically feasible. However:

1. **All other query endpoints live in the API.** Search is a query endpoint. Putting
   it in Conduit would create a second query surface with its own schemas, error
   handling, and response formatting -- none of which Conduit has today.

2. **Conduit's `/embed` endpoint already exists.** The API already has an
   `EmbeddingClient` that calls it. Search is: embed the query text, then run SQL.
   The API has both pieces.

3. **No new Conduit endpoints needed.** Conduit stays focused on compute-intensive
   ML inference. The API stays responsible for all data access and response shaping.

4. **Simpler testing.** Integration tests already exercise the API. Adding a search
   endpoint follows the same `consumer/` + `user_lifecycle.rs` pattern.

### Data Flow

```
dapple-ui / Nushell command
    |
    |  GET /api/guide-search/?q=...&tags=...&locale=...&limit=...
    v
dapple-prompt-api (search handler)
    |
    |  1. POST /embed { texts: [query] }  --> dapple-prompt-conduit
    |     <-- { embeddings: [[0.1, ...]], dimension: 768 }
    |
    |  2. search_guide_content_embeddings(pool, embedding, tags, locale, limit)
    |     --> pgvector similarity search
    |     <-- Vec<GuideEmbeddingSearchResult>
    |
    |  3. Convert to Vec<GuideSearchResult> DTO
    |
    v
{ status: "success", results: N, objs: [...] }
```

---

## 2. Search Endpoint Design

### 2a. Endpoint Shape

```
GET /api/guide-search/?q=authentication&tags=product,universal&locale=en&limit=20
```

This is a **new top-level route** (`/api/guide-search/`), not a variant on the
existing `/api/guides/` index. Rationale:

- Search is semantically different from CRUD queries. It takes free-text input and
  returns ranked results with similarity scores -- not full `Guide` DTOs.
- The response type (`GuideSearchResult`) is different from `Guide`.
- Mixing it into `GetGuideSetSchema` would complicate the enum and confuse the
  OpenAPI docs.
- The Nushell search command and dapple-ui search bar both want a simple, dedicated
  endpoint.

### 2b. Query Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `q` | `String` | Yes | -- | Natural language search query |
| `tags` | `Vec<String>` | No | `["universal"]` | Audience tag filter (comma-separated in URL) |
| `locale` | `String` | No | `"en"` | Locale filter |
| `limit` | `i32` | No | `20` | Maximum results (clamped to 1..100) |
| `guide_ids` | `Vec<Uuid>` | No | `None` | Scope search to specific guides (for agent RAG) |
| `threshold` | `f64` | No | `0.0` | Minimum similarity score (0.0-1.0) |

### 2c. Schema Definition

**File:** `crates/dapple-prompt-api/src/schema/guide_search.rs`

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Query parameters for the guide search endpoint.
///
/// Search uses Conduit's embedding service to encode the query text,
/// then runs pgvector similarity search with audience/locale filtering.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GuideSearchParams {
    /// Natural language search query.
    pub q: String,
    /// Audience tags to filter by (e.g., ["product", "universal"]).
    /// Defaults to ["universal"] if omitted.
    pub tags: Option<Vec<String>>,
    /// Locale tag for filtering (e.g., "en", "es", "pt-br").
    /// Defaults to "en" if omitted.
    pub locale: Option<String>,
    /// Maximum number of results. Clamped to 1..100.
    /// Defaults to 20 if omitted.
    pub limit: Option<i32>,
    /// Scope search to specific guide IDs (for agent RAG use cases).
    /// When omitted, searches all guides.
    pub guide_ids: Option<Vec<Uuid>>,
    /// Minimum similarity score threshold (0.0 to 1.0).
    /// Results below this score are excluded. Defaults to 0.0.
    pub threshold: Option<f64>,
}

impl GuideSearchParams {
    /// Resolve defaults and clamp values.
    pub fn resolve(&self) -> ResolvedSearchParams {
        ResolvedSearchParams {
            q: self.q.clone(),
            tags: self.tags.clone().unwrap_or_else(|| vec!["universal".to_string()]),
            locale: self.locale.clone().unwrap_or_else(|| "en".to_string()),
            limit: self.limit.unwrap_or(20).clamp(1, 100),
            guide_ids: self.guide_ids.clone(),
            threshold: self.threshold.unwrap_or(0.0),
        }
    }
}

/// Resolved search parameters with defaults applied.
pub struct ResolvedSearchParams {
    pub q: String,
    pub tags: Vec<String>,
    pub locale: String,
    pub limit: i32,
    pub guide_ids: Option<Vec<Uuid>>,
    pub threshold: f64,
}
```

### 2d. Handler

**File:** `crates/dapple-prompt-api/src/handler/guide_search.rs`

```rust
use crate::extract_qs::QsQuery as Query;
use crate::handler::index_response_handler;
use crate::schema::guide_search::GuideSearchParams;
use crate::service::embedding_client::EmbeddingClient;
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use dapple_data::model::guide_content_embeddings::GuideSearchResult;
use dapple_data::query::guide_content_embeddings::{
    search_guide_content_embeddings, search_guide_content_embeddings_by_guide_ids,
};
use serde_json::json;
use std::sync::Arc;

pub async fn guide_search_handler(
    opts: Query<GuideSearchParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    // 1. Validate query
    if params.q.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "fail",
                "message": "Search query 'q' cannot be empty"
            })),
        ));
    }

    let resolved = params.resolve();

    // 2. Get conduit URL
    let conduit_url = match &data.conduit_url {
        Some(url) => url.clone(),
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "fail",
                    "message": "Search is not available: embedding service not configured"
                })),
            ));
        }
    };

    // 3. Embed the query text
    let client = EmbeddingClient::new(&conduit_url);
    let query_embedding = match client.embed(&[resolved.q.clone()]).await {
        Ok(Some(embeddings)) if !embeddings.is_empty() => embeddings.into_iter().next().unwrap(),
        Ok(Some(_)) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": "Embedding service returned empty result"
                })),
            ));
        }
        Ok(None) => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "fail",
                    "message": "Embedding service not available"
                })),
            ));
        }
        Err(e) => {
            log::error!("Search embedding failed: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": format!("Failed to embed search query: {}", e)
                })),
            ));
        }
    };

    // 4. Run pgvector similarity search
    let search_results = if let Some(ref guide_ids) = resolved.guide_ids {
        search_guide_content_embeddings_by_guide_ids(
            &data.db,
            &query_embedding,
            guide_ids,
            &resolved.tags.iter().map(|s| s.as_str().to_string()).collect::<Vec<_>>(),
            &resolved.locale,
            resolved.limit,
        )
        .await
    } else {
        search_guide_content_embeddings(
            &data.db,
            &query_embedding,
            &resolved.tags.iter().map(|s| s.as_str().to_string()).collect::<Vec<_>>(),
            &resolved.locale,
            resolved.limit,
        )
        .await
    };

    // 5. Convert to DTOs, apply threshold filter
    let objs: Result<Vec<GuideSearchResult>, _> = search_results.map(|results| {
        results
            .into_iter()
            .filter(|r| r.similarity >= resolved.threshold)
            .map(GuideSearchResult::from)
            .collect()
    });

    Ok(index_response_handler("guide_search", objs))
}
```

### 2e. Route

**File:** `crates/dapple-prompt-api/src/route/guide_search.rs`

```rust
use crate::AppState;
use crate::handler::guide_search::guide_search_handler;
use crate::handler::{IndexErrorResponse, IndexResponse};
use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use dapple_data::model::guide_content_embeddings::GuideSearchResult;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new().api_route(
        "/",
        get_with(guide_search_handler, |op| {
            op.id("guideSearch")
                .description(
                    "Search guide content using natural language. Embeds the query \
                     via Conduit's embedding service, then runs pgvector similarity \
                     search with audience tag and locale filtering.",
                )
                .tag("Search")
                .tag("Guides")
                .response::<200, Json<IndexResponse<GuideSearchResult>>>()
                .response::<400, Json<IndexErrorResponse>>()
                .response::<503, Json<IndexErrorResponse>>()
                .response::<500, Json<IndexErrorResponse>>()
        }),
    )
}
```

### 2f. Route Registration

In `crates/dapple-prompt-api/src/route.rs`, add:

```rust
mod guide_search;
```

And in `build_api_router()` and `build_spec_router()`:

```rust
.nest("/api/guide-search/", guide_search::router())
```

### 2g. Module Registration

In `crates/dapple-prompt-api/src/handler.rs`, add:

```rust
pub(crate) mod guide_search;
```

In `crates/dapple-prompt-api/src/schema.rs`, add:

```rust
pub mod guide_search;
```

---

## 3. `ByClassification` Query Variants

### 3a. Guides Index: `ByClassification`

Add to `GetGuideSetSchema` in `crates/dapple-prompt-api/src/schema/guides.rs`:

```rust
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetGuideSetSchema {
    ByObjectIds { ... },
    All { ... },
    BySlugs { ... },
    ByPathIds { ... },
    /// Retrieve guides filtered by classification (e.g., "workflow", "reference").
    /// Returns the latest non-deprecated version per path, like `All`.
    ByClassification {
        classification: String,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
}
```

Add the `classification` parameter to `QueryParamDocs`:

```rust
ParamDoc {
    name: "classification",
    description: "Filter guides by classification (e.g., 'workflow', 'reference', 'tutorial').",
    required: false,
    schema: || param_schema_from::<String>(),
},
```

### 3b. Guide Cards: `ByClassification`

Add to `GetGuideCardSetSchema` in `crates/dapple-prompt-api/src/schema/guides.rs`:

```rust
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetGuideCardSetSchema {
    All { ... },
    ByCategory { ... },
    /// Retrieve guide cards filtered by guide classification.
    /// Useful for excluding internal/reference guides from the carousel.
    ByClassification {
        classification: String,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
}
```

Add the `classification` parameter to `GetGuideCardSetSchema::extra_params()`.

### 3c. SQL for `ByClassification` Guides

**File:** `crates/dapple-data/assets/sql/guides/select_guides_by_classification.sql`

This query follows the same pattern as `select_guides.sql` but adds a
`WHERE g.classification = $5` filter on the `guide_latest` CTE.

```sql
-- Select guides filtered by classification, one version per path.
-- Follows the same pinned/latest pattern as select_guides.sql.
-- $1: LIMIT
-- $2: OFFSET
-- $3: environment (nullable)
-- $4: scope (nullable)
-- $5: classification (TEXT, exact match)
WITH current_pins AS (
    SELECT crp.ref_key, crp.versioned_object_id, crp.id AS pin_id
    FROM rev.canonical_release_pins crp
    LEFT JOIN rev.canonical_release_pins newer ON newer.supersedes_id = crp.id
    WHERE newer.id IS NULL
      AND crp.scope = $4
      AND crp.environment = $3
      AND crp.object_type = 'guides'
      AND crp.resolved_at IS NOT NULL
),
guide_latest AS (
    SELECT DISTINCT ON (op.id)
        g.id
    FROM rev.guides g
    JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = vo.deprecation_level_id
    LEFT JOIN current_pins cp
        ON cp.ref_key = g.source_path
       AND cp.versioned_object_id = vo.id
    WHERE dl.id IS NULL
      AND g.classification = $5
    ORDER BY op.id,
        cp.pin_id NULLS LAST,
        vo.created_at DESC
),
guide_page AS (
    SELECT id FROM guide_latest
    ORDER BY id
    LIMIT $1 OFFSET $2
)
SELECT
    g.id,
    g.versioned_object_id,
    vo.path_id,
    gpd.title,
    gpd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    g.description,
    g.author,
    g.classification,
    g.source_path,
    g.created_at,
    gc.id AS chapter_id,
    gc.parent_chapter_id AS chapter_parent_chapter_id,
    gc.title AS chapter_title,
    gc.source_file AS chapter_source_file,
    gc.created_at AS chapter_created_at,
    gb.id AS block_id,
    bdt.key AS block_directive_type_key,
    gb.name AS block_name,
    gb.content AS block_content,
    gb.attributes AS block_attributes,
    gb.fluent_message_id AS block_fluent_message_id,
    gb.created_at AS block_created_at,
    ARRAY_REMOVE(ARRAY_AGG(DISTINCT gt.key), NULL) AS block_tag_keys
FROM guide_page gp
JOIN rev.guides g ON g.id = gp.id
JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = vo.deprecation_level_id
LEFT JOIN rev.guide_chapters gc ON gc.guide_id = g.id
LEFT JOIN rev.guide_blocks gb ON gb.chapter_id = gc.id
LEFT JOIN rev.guide_block_directive_types bdt ON bdt.id = gb.directive_type_id
LEFT JOIN rev.guide_block_tags gbt ON gbt.block_id = gb.id
LEFT JOIN rev.guide_tags gt ON gt.id = gbt.tag_id
GROUP BY
    g.id, g.versioned_object_id, vo.path_id,
    gpd.title, gpd.slug, vo.content_hash,
    dl.key, g.description, g.author, g.classification, g.source_path,
    g.created_at,
    gc.id, gc.parent_chapter_id, gc.title,
    gc.source_file, gc.created_at,
    gb.id, bdt.key, gb.name, gb.content, gb.attributes,
    gb.fluent_message_id,
    gb.created_at
ORDER BY g.id, gc.id, gb.id;
```

### 3d. SQL for `ByClassification` Guide Cards

**File:** `crates/dapple-data/assets/sql/guides/select_guide_cards_by_classification.sql`

```sql
-- Select latest version's card for each guide path, filtered by guide classification.
-- $1: LIMIT
-- $2: OFFSET
-- $3: environment (nullable)
-- $4: scope (nullable)
-- $5: classification (TEXT, exact match)
WITH current_pins AS (
    SELECT crp.ref_key, crp.versioned_object_id, crp.id AS pin_id
    FROM rev.canonical_release_pins crp
    LEFT JOIN rev.canonical_release_pins newer ON newer.supersedes_id = crp.id
    WHERE newer.id IS NULL
      AND crp.scope = $4
      AND crp.environment = $3
      AND crp.object_type = 'guides'
      AND crp.resolved_at IS NOT NULL
),
latest_guides AS (
    SELECT DISTINCT ON (op.id)
        g.id AS guide_id,
        vo.path_id,
        gpd.title,
        gpd.slug,
        vo.content_hash
    FROM rev.guides g
    JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = vo.deprecation_level_id
    LEFT JOIN current_pins cp
        ON cp.ref_key = g.source_path
       AND cp.versioned_object_id = vo.id
    WHERE dl.id IS NULL
      AND g.classification = $5
    ORDER BY op.id,
        cp.pin_id NULLS LAST,
        vo.created_at DESC
)
SELECT
    gc.id,
    gc.guide_id,
    lg.path_id,
    lg.title,
    lg.slug,
    gc.subtitle,
    gc.card_image_asset_id,
    gc.category,
    gc.color,
    gc.accent,
    gc.created_at
FROM latest_guides lg
JOIN rev.guide_cards gc ON gc.guide_id = lg.guide_id
ORDER BY gc.category, lg.title
LIMIT $1 OFFSET $2;
```

### 3e. Query Functions

Add to `crates/dapple-data/src/query/guides.rs`:

```rust
pub async fn select_guides_by_classification(
    pool: &DbPool,
    classification: &str,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<Guide>, DataError> {
    let models = select_guide_models_by_classification(
        pool, classification, scope, environment, filter_ops,
    ).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_guide_models_by_classification(
    pool: &DbPool,
    classification: &str,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideModel>, DataError> {
    let flat_rows = sqlx::query_file_as!(
        FlatGuideModel,
        "assets/sql/guides/select_guides_by_classification.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        environment as Option<&str>,
        scope as Option<&str>,
        classification,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let mut models = FlatGuideModel::fold_models(flat_rows)?;
    enrich_guide_models(pool, &mut models).await?;
    Ok(models)
}

pub async fn select_guide_cards_by_classification(
    pool: &DbPool,
    classification: &str,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideCardCarouselItem>, DataError> {
    let rows = sqlx::query_file_as!(
        GuideCardCarouselModel,
        "assets/sql/guides/select_guide_cards_by_classification.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        environment as Option<&str>,
        scope as Option<&str>,
        classification,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}
```

### 3f. Handler Wiring

Add match arms to the existing handlers in `crates/dapple-prompt-api/src/handler/guides.rs`:

In `guides_index_handler`:

```rust
GetGuideSetSchema::ByClassification {
    classification,
    scope,
    environment,
    options,
} => {
    select_guides_by_classification(
        &data.db,
        &classification,
        scope.as_deref(),
        environment.as_deref(),
        options.unwrap_or_default(),
    )
    .await
}
```

In `guide_cards_index_handler`:

```rust
GetGuideCardSetSchema::ByClassification {
    classification,
    scope,
    environment,
    options,
} => {
    select_guide_cards_by_classification(
        &data.db,
        &classification,
        scope.as_deref(),
        environment.as_deref(),
        options.unwrap_or_default(),
    )
    .await
}
```

---

## 4. Complete File Manifest

### New Files

| File | Purpose |
|------|---------|
| `crates/dapple-prompt-api/src/schema/guide_search.rs` | `GuideSearchParams`, `ResolvedSearchParams` |
| `crates/dapple-prompt-api/src/handler/guide_search.rs` | Search handler (embed query + pgvector search) |
| `crates/dapple-prompt-api/src/route/guide_search.rs` | Route registration for `/api/guide-search/` |
| `crates/dapple-data/assets/sql/guides/select_guides_by_classification.sql` | Guides filtered by classification |
| `crates/dapple-data/assets/sql/guides/select_guide_cards_by_classification.sql` | Guide cards filtered by classification |

### Modified Files

| File | Change |
|------|--------|
| `crates/dapple-prompt-api/src/handler.rs` | Add `pub(crate) mod guide_search;` |
| `crates/dapple-prompt-api/src/schema.rs` | Add `pub mod guide_search;` |
| `crates/dapple-prompt-api/src/route.rs` | Add `mod guide_search;` and nest route in both `build_api_router()` and `build_spec_router()` |
| `crates/dapple-prompt-api/src/schema/guides.rs` | Add `ByClassification` to both `GetGuideSetSchema` and `GetGuideCardSetSchema`, add `classification` to both `QueryParamDocs` implementations |
| `crates/dapple-prompt-api/src/handler/guides.rs` | Add match arms for `ByClassification` in both index handlers, add imports for new query functions |
| `crates/dapple-data/src/query/guides.rs` | Add `select_guides_by_classification`, `select_guide_models_by_classification`, `select_guide_cards_by_classification` |

### No DDL Changes

Phase 3 requires **no new migrations**. All tables, indexes, and junction tables were
created in Phase 2 (migration 16). The search SQL files from Phase 2 are used
directly.

---

## 5. Implementation Guidance

### Module Organization

The search endpoint is a new top-level route, not nested under guides. This keeps it
clean and follows the pattern of other cross-cutting endpoints. The file structure
mirrors the standard `schema/` + `handler/` + `route/` trio.

### Error Handling Strategy

The search handler has three failure modes, each with a distinct HTTP status:

1. **400 Bad Request** -- Empty query string. Client error, no retry.
2. **503 Service Unavailable** -- Conduit URL not configured, or embedding service
   not running. Conduit is down or unconfigured. Client should show a "search
   unavailable" message and retry later.
3. **500 Internal Server Error** -- Embedding generation failed, or pgvector query
   failed. Unexpected error, logged for debugging.

The handler returns the standard `IndexResponse<GuideSearchResult>` on success, which
is consistent with all other list endpoints.

### Performance Considerations

- **Single embedding call per search.** The query text is one string, so the
  `/embed` call sends exactly one text. The HTTP overhead is minimal (~10ms local,
  ~50ms cross-pod).
- **pgvector index.** Migration 16 created an IVFFlat index with 100 lists. For the
  current corpus size (hundreds of blocks, not millions), this is sufficient. If
  recall degrades at scale, switch to HNSW.
- **Threshold filtering in Rust.** The SQL `LIMIT` is applied before the threshold
  filter. This means if 20 results are returned but 5 are below threshold, the
  client gets 15. This is acceptable: over-requesting from pgvector and filtering
  in Rust is simpler than adding a `WHERE similarity > $N` clause that would
  interact poorly with the index scan.
- **No caching.** Search queries are inherently diverse. Caching query embeddings
  would have low hit rates. If hot queries emerge (e.g., from a search suggest
  dropdown), caching can be added later.

### EmbeddingClient Reuse

The existing `EmbeddingClient` in `crates/dapple-prompt-api/src/service/embedding_client.rs`
is instantiated per-call in the search handler (same pattern as
`guide_embeddings.rs`). It uses `reqwest::Client::new()` which internally uses a
connection pool, so there is no TCP connection overhead on repeated calls.

If search volume increases, the `EmbeddingClient` could be moved to `AppState` as a
persistent instance. This is a future optimization, not needed for Phase 3.

### Query Parameter Parsing

The handler uses the existing `QsQuery` extractor (from `extract_qs.rs`), which
supports both JSON-object-style query params (`?query={...}`) and flat query params.
The `GuideSearchParams` struct uses flat fields (not a tagged enum), so the URL is
clean:

```
/api/guide-search/?q=authentication&tags=product&tags=universal&locale=en&limit=20
```

The `tags` field is `Option<Vec<String>>`. The `qs` crate handles repeated keys
(`tags=product&tags=universal`) as a Vec.

---

## 6. Testing Strategy

### 6a. Unit Tests

In `crates/dapple-prompt-api/src/schema/guide_search.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_defaults() {
        let params = GuideSearchParams {
            q: "test query".into(),
            tags: None,
            locale: None,
            limit: None,
            guide_ids: None,
            threshold: None,
        };
        let resolved = params.resolve();
        assert_eq!(resolved.tags, vec!["universal".to_string()]);
        assert_eq!(resolved.locale, "en");
        assert_eq!(resolved.limit, 20);
        assert_eq!(resolved.threshold, 0.0);
    }

    #[test]
    fn test_resolve_limit_clamping() {
        let params = GuideSearchParams {
            q: "test".into(),
            tags: None,
            locale: None,
            limit: Some(500),
            guide_ids: None,
            threshold: None,
        };
        assert_eq!(params.resolve().limit, 100);

        let params2 = GuideSearchParams {
            q: "test".into(),
            tags: None,
            locale: None,
            limit: Some(-5),
            guide_ids: None,
            threshold: None,
        };
        assert_eq!(params2.resolve().limit, 1);
    }
}
```

### 6b. Integration Tests

Search integration testing requires embeddings in the database, which in turn
requires a running Conduit with the embedding service. The integration test should:

1. **Skip gracefully** if `DAPPLE_CONDUIT_URL` is not set or Conduit is unavailable.
2. When available, test the full round-trip: ingest a guide (which generates
   embeddings), then search for content from that guide and verify results.

**File:** `crates/dapple-prompt-api/tests/consumer/guide_search.rs`

```rust
use dapple_data::model::guide_content_embeddings::GuideSearchResult;
use reqwest::Client;

pub struct GuideSearchEndpoint {
    pub base_url: String,
    pub client: Client,
}

impl GuideSearchEndpoint {
    pub async fn search(
        &self,
        q: &str,
        tags: Option<&[&str]>,
        locale: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<GuideSearchResult>, String> {
        let mut url = format!("{}/api/guide-search/?q={}", self.base_url, q);
        if let Some(tags) = tags {
            for tag in tags {
                url.push_str(&format!("&tags={}", tag));
            }
        }
        if let Some(locale) = locale {
            url.push_str(&format!("&locale={}", locale));
        }
        if let Some(limit) = limit {
            url.push_str(&format!("&limit={}", limit));
        }

        let resp = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
        // Parse IndexResponse<GuideSearchResult>
        // ...
        todo!()
    }
}
```

**Trial in `tests/user_lifecycle.rs`:**

```rust
fn trial_guide_search() -> Trial {
    // Wait for: guide ingest completed (embeddings generated)
    // Search for a known term from the ingested guide
    // Verify: results contain blocks from the expected guide
    // Verify: similarity scores are > 0
    // Verify: results respect the limit parameter
}
```

### 6c. Classification Variant Testing

The `ByClassification` variants can be tested with the existing guide lifecycle
tests. After creating guides with different classifications, query with each
classification and verify only matching guides are returned.

```rust
fn trial_guides_by_classification() -> Trial {
    // Wait for: guides-init batch
    // Query: ByClassification { classification: "workflow" }
    // Verify: only workflow guides returned
    // Query: ByClassification { classification: "reference" }
    // Verify: only reference guides returned (or empty if none exist)
}
```

---

## 7. Future Considerations

### 7a. Nushell Search Command

The original plan (section 6) describes a Nushell search command for the librarian
guide. With the API search endpoint in place, this command is straightforward:

```nushell
# In the guide's task block
def search [query: string, --tags: list<string> = ["universal"], --locale: string = "en"] {
    let url = $"($env.DAPPLE_METADATA.api_url)/api/guide-search/?q=($query)&locale=($locale)"
    let result = (http get $url --headers [Authorization $"Bearer ($env.DAPPLE_METADATA.token)"])
    $result.objs
}
```

This is authoring work, not infrastructure work, and is out of scope for this plan.

### 7b. ExcludeClassification vs ByClassification

The UI carousel wants to show all guides **except** internal/reference ones. Two
approaches:

1. **ByClassification** (this plan) -- UI queries `?classification=workflow` to get
   only workflow guides. Simple, but requires the UI to know which classifications
   to include.
2. **ExcludeClassification** -- UI queries `?exclude_classification=reference` to
   exclude specific types. More flexible if new classifications are added.

This plan implements `ByClassification` because it is simpler and matches the
existing `ByCategory` pattern on guide cards. If `ExcludeClassification` is needed
later, it can be added as another variant.

### 7c. Search Result Enrichment

The current `GuideSearchResult` includes `metadata` JSONB which contains
`guide_slug`, `guide_title`, `chapter_title`, etc. The UI can use these to render
rich search results without additional API calls. If the UI needs full guide data
(e.g., to show a card preview), it can follow up with
`GET /api/guides/?query=by_object_ids&ids=[guide_id]`.

### 7d. Agent RAG Path

The `guide_ids` parameter on the search endpoint supports the agent RAG use case
described in the parent plan (section 4a). When Flow dispatches a task, it can call
the search endpoint with `guide_ids` scoped to the playbook's source guide. The
full `RagQueryContext` and Lance integration (Phase 4) will build on this foundation.

---

## 8. Open Questions

### Q1: Should search be unauthenticated?

The current plan requires authentication (via `RegisteredUser` extractor), consistent
with all other API endpoints. If the `/references` page should be publicly accessible
(no login required), the search endpoint would need to be added to the auth middleware
exclusion list. **Recommendation:** Keep it authenticated for now. The librarian guide
runs inside a journey, which is already authenticated.

### Q2: Should the threshold default be higher?

A threshold of `0.0` means all results are returned regardless of relevance. This is
useful for development and debugging. For production, a default of `0.3` or `0.4`
might provide better UX by filtering out irrelevant noise. **Recommendation:** Start
with `0.0` and tune based on real usage data.

### Q3: Rate limiting on search?

Each search call triggers an embedding generation call to Conduit. If search is
heavily used, this could overload the ONNX model. **Recommendation:** Defer rate
limiting to Phase 4. The current single-user / low-volume usage does not warrant it.
