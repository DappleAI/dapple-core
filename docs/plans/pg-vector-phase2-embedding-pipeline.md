# pgvector Phase 2: Embedding Generation Pipeline Integration

**Status:** Draft
**Created:** 2026-02-24
**Last Updated:** 2026-02-24
**Target Version:** N/A (Phase 2 continuation)
**Dependencies:** Migration 16 (pgvector embeddings DDL, already applied), Phase 2 data layer (models + SQL files, already created), EmbeddingService in dapple-prompt-conduit

---

## Executive Summary

This plan specifies the exact integration for the embedding generation pipeline -- the code that runs after guide ingest to generate and store embeddings in `rev.guide_content_embeddings`. The pgvector schema and data-layer scaffolding (migration 16, SQL files, Rust model stubs, query stubs) are complete. What remains is the orchestration: extracting embeddable text from guide blocks, calling the EmbeddingService, constructing metadata, and inserting the results with tag and locale associations.

The chosen approach is **Option C from the task brief**: the API does everything except the `embed()` call itself, which it delegates to Conduit via an internal HTTP endpoint. This keeps the ONNX runtime out of the API binary, maintains the existing service boundary, and allows the API to own the full pipeline lifecycle (block extraction, metadata construction, DB writes) while Conduit handles only the compute-intensive embedding generation.

The pipeline runs asynchronously after guide ingest completes. Embedding failure never fails guide ingest -- errors are logged as warnings and the guide is stored without embeddings. This matches the existing pattern where derived objects (brief packs, inference profiles) are best-effort additions to the core guide data.

---

## Context and Background

### Current State

1. **Migration 16** is applied: `rev.guide_content_embeddings`, `rev.guide_embedding_tags`, and `rev.guide_embedding_locales` tables exist with all indexes.
2. **SQL files** exist in `crates/dapple-data/assets/sql/guide_content_embeddings/` for insert, delete, and search operations.
3. **Model stubs** are defined in the Phase 2 plan (`pg-vector-phase2-implementation.md`) but the model and query `.rs` files have NOT yet been created in the source tree (the Phase 2 plan has `todo!()` implementations).
4. **EmbeddingService** lives in `dapple-prompt-conduit` with a `ModernBertBackend` (ONNX, 768-dim) and a `StubEmbeddingBackend` (deterministic hash-based, for testing).
5. **Conduit** is a separate K8s deployment with its own Axum router (`/prompt`, `/summarize`, `/madlib`, `/synthesize` endpoints). It has `embedding_service: Option<Arc<EmbeddingService>>` in its `AppState`.
6. **API AppState** currently contains only `pub db: DbPool` -- it has no embedding-related state.
7. **Guide blocks** either have inline `content: Some(text)` (nushell, ref blocks) or `content: None` with `fluent_message_id: Some(uuid)` (prose, stage, step, brief_pack, inference_profile blocks). The embeddable text for Fluent-backed blocks lives in `rev.fluent_message_bodies`.

### Problems Addressed

- There is no code that triggers embedding generation after guide ingest.
- Fluent-backed blocks require a query to resolve their text content before embedding.
- Locale associations require determining which locales a guide covers, which is implicit in the Fluent message data rather than stored directly on the guide.
- The service boundary between API and Conduit must be respected -- ONNX runtime should not be pulled into the API binary.

---

## 1. Integration Approach: API-Orchestrated, Conduit-Computed (Option C)

### Decision

The API owns the full embedding pipeline except the `embed()` call:

```
guide_ingest.rs (after commit)
    |
    v
generate_embeddings_for_guide()   <-- new function in API
    |
    |-- 1. Load GuideModel (reuse existing select query)
    |-- 2. Extract embeddable blocks + resolve Fluent bodies
    |-- 3. Build metadata for each block
    |-- 4. POST /embed to Conduit with texts[]  <-- single HTTP call
    |-- 5. DELETE old embeddings for this guide
    |-- 6. INSERT new embeddings + tag associations + locale associations
    v
done (errors logged, never fail the ingest)
```

### Rationale

**Why not Option A (EmbeddingService in API AppState)?**
Adding `dapple-prompt-conduit` as a dependency of `dapple-prompt-api` would pull in the ONNX runtime (`ort`), `ndarray`, `tokenizers`, and all model-loading infrastructure into the API binary. These are ~50MB of additional dependencies. The API is a lightweight Axum service; Conduit is the designated compute-heavy service.

**Why not Option B (Conduit does everything)?**
Conduit would need access to the full guide data model, the Fluent message resolution logic, the metadata construction logic, and the embedding insert queries. This duplicates data-layer knowledge and creates a parallel path into `dapple-data`. The API already has all of this.

**Why Option C?**
- API already has the `DbPool`, the guide query infrastructure, and the Fluent message resolution.
- Conduit already has the `EmbeddingService` and ONNX runtime.
- A single HTTP call (`POST /embed`) is the minimal interface between them.
- The API can batch all block texts into one call (typical guide has 10-50 embeddable blocks).
- If Conduit is unavailable, the API gracefully degrades (no embeddings, guide still stored).

---

## 2. New Conduit Endpoint: POST /embed

### Endpoint Contract

```
POST /embed
Content-Type: application/json

Request:
{
    "texts": ["block text 1", "block text 2", ...]
}

Response (200):
{
    "embeddings": [[0.123, -0.456, ...], [0.789, ...], ...],
    "dimension": 768
}

Response (503 - no embedding service):
{
    "error": "Embedding service not available"
}
```

### Implementation Location

**File:** `crates/dapple-prompt-conduit/src/handler/embed.rs` (new)

```rust
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct EmbedRequest {
    pub texts: Vec<String>,
}

#[derive(Serialize)]
pub struct EmbedResponse {
    pub embeddings: Vec<Vec<f32>>,
    pub dimension: usize,
}

#[derive(Serialize)]
pub struct EmbedErrorResponse {
    pub error: String,
}

pub async fn handle_embed(
    State(state): State<AppState>,
    Json(req): Json<EmbedRequest>,
) -> Result<Json<EmbedResponse>, (StatusCode, Json<EmbedErrorResponse>)> {
    let service = state.embedding_service.as_ref().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(EmbedErrorResponse {
            error: "Embedding service not available".to_string(),
        }))
    })?;

    let embeddings = service.embed(&req.texts).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(EmbedErrorResponse {
            error: format!("Embedding generation failed: {}", e),
        }))
    })?;

    Ok(Json(EmbedResponse {
        dimension: service.dimension(),
        embeddings,
    }))
}
```

### Route Registration

**File:** `crates/dapple-prompt-conduit/src/main.rs`

Add to the existing router:

```rust
// Existing:
.route("/prompt", post(handler::handle_prompt))
.route("/summarize", post(handler::handle_summarize))
.route("/madlib", post(handler::handle_madlib))
.route("/synthesize", post(handler::handle_synthesize))
// NEW:
.route("/embed", post(handler::handle_embed))
```

### Conduit EmbeddingService Initialization

The `EmbeddingService` is already wired into `AppState` but not initialized in `main.rs`. Add initialization after the TTS service block:

```rust
// After TTS initialization (around line 179 of main.rs):
if let Some(embedding_config) = EmbeddingConfig::from_env() {
    log::info!(
        "Embedding service configured with model: {}",
        embedding_config.model_id
    );
    match EmbeddingService::with_modernbert(&embedding_config).await {
        Ok(service) => {
            log::info!(
                "Embedding service initialized with {} backend (dim={})",
                service.backend_name(),
                service.dimension()
            );
            state.embedding_service = Some(Arc::new(service));
        }
        Err(e) => {
            if embedding_config.required {
                panic!("Failed to initialize required embedding service: {}", e);
            }
            log::warn!(
                "Failed to initialize embedding service, continuing without: {}",
                e
            );
        }
    }
}
```

**Environment variables** (already supported by `EmbeddingConfig::from_env()`):
- `EMBEDDING_ENABLED=true` -- enables the service
- `EMBEDDING_MODEL_ID=lightonai/modernbert-embed-large` -- defaults to this
- `EMBEDDING_DIMENSION=768` -- Matryoshka dimension (default 768)
- `EMBEDDING_REQUIRED=false` -- whether startup fails if loading fails

---

## 3. API-Side Embedding Pipeline

### 3a. New API State: Conduit URL

**File:** `crates/dapple-prompt-api/src/lib.rs`

```rust
pub struct AppState {
    pub db: DbPool,
    pub conduit_url: Option<String>,  // NEW: e.g., "http://dapple-prompt-conduit:8082"
}
```

The `conduit_url` is loaded from the `DAPPLE_CONDUIT_URL` environment variable. When `None`, embedding generation is skipped silently.

**File:** `crates/dapple-prompt-api/src/main.rs` (or wherever `build_app` constructs the state)

```rust
let conduit_url = std::env::var("DAPPLE_CONDUIT_URL").ok();
let state = Arc::new(AppState {
    db: pool,
    conduit_url,
});
```

### 3b. Embedding Client Module

**File:** `crates/dapple-prompt-api/src/service/embedding_client.rs` (new)

A thin HTTP client that calls Conduit's `/embed` endpoint:

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct EmbeddingClient {
    client: Client,
    base_url: String,
}

#[derive(Serialize)]
struct EmbedRequest {
    texts: Vec<String>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
    dimension: usize,
}

impl EmbeddingClient {
    pub fn new(conduit_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: conduit_url.trim_end_matches('/').to_string(),
        }
    }

    /// Generate embeddings for a batch of texts.
    /// Returns None if the service is unavailable (graceful degradation).
    pub async fn embed(&self, texts: &[String]) -> Result<Option<Vec<Vec<f32>>>, String> {
        if texts.is_empty() {
            return Ok(Some(Vec::new()));
        }

        let url = format!("{}/embed", self.base_url);
        let response = self.client
            .post(&url)
            .json(&EmbedRequest { texts: texts.to_vec() })
            .timeout(std::time::Duration::from_secs(120))  // ONNX may be slow
            .send()
            .await
            .map_err(|e| format!("Conduit embed request failed: {}", e))?;

        if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            // Conduit has no embedding service -- degrade gracefully
            return Ok(None);
        }

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Conduit embed returned {}: {}", response.status(), body));
        }

        let result: EmbedResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse embed response: {}", e))?;

        Ok(Some(result.embeddings))
    }
}
```

### 3c. Block Content Resolution

For Fluent-backed blocks, we need to resolve the body text. This is the critical piece: how to get embeddable text from `rev.fluent_message_bodies`.

#### New SQL File

**File:** `crates/dapple-data/assets/sql/fluent_messages/fluent_message_bodies/select_fluent_bodies_by_message_ids.sql` (new)

```sql
-- Resolve Fluent message bodies for a batch of message IDs.
-- Returns all locale bodies for the given messages.
-- The caller picks the preferred locale (e.g., "en" first, then any available).
--
-- Parameters:
--   $1: fluent_message_ids (UUID[])
SELECT
    fmb.fluent_message_id,
    fml.tag AS locale_tag,
    fmb.body
FROM rev.fluent_message_bodies fmb
JOIN rev.fluent_message_locales fml ON fml.id = fmb.locale_id
WHERE fmb.fluent_message_id = ANY($1::UUID[])
ORDER BY fmb.fluent_message_id, fml.tag;
```

#### New Rust Query Function

**File:** `crates/dapple-data/src/query/fluent_messages.rs` (add to existing file)

```rust
/// Row returned from select_fluent_bodies_by_message_ids.sql
#[derive(Clone, Debug)]
pub struct FluentBodyRow {
    pub fluent_message_id: Uuid,
    pub locale_tag: String,
    pub body: String,
}

/// Batch-resolve Fluent message bodies for multiple message IDs.
/// Returns all locale variants for each message.
pub async fn select_fluent_bodies_by_message_ids(
    pool: &PgPool,
    message_ids: &[Uuid],
) -> Result<Vec<FluentBodyRow>, DataError> {
    if message_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_file_as!(
        FluentBodyRow,
        "assets/sql/fluent_messages/fluent_message_bodies/select_fluent_bodies_by_message_ids.sql",
        message_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}
```

#### Resolution Strategy

The resolution follows this priority:

1. If block has `content: Some(text)` -- use inline content directly (nushell, ref blocks).
2. If block has `fluent_message_id: Some(id)` -- batch-fetch all locale bodies for all fluent IDs in the guide, then pick "en" locale first, falling back to the first available locale.
3. If neither -- skip the block (empty or structural-only).

The batch-fetch is critical: instead of N+1 queries (one per Fluent-backed block), we fetch ALL bodies for ALL fluent_message_ids in a single query, then index by `(fluent_message_id, locale_tag)` in Rust.

### 3d. Locale Determination

**Question from the task brief:** How do we determine which locale(s) to associate with each embedding?

**Answer:** From the Fluent messages themselves.

1. During block content resolution (section 3c), we fetch all locale bodies for all Fluent messages in the guide.
2. The set of locale tags across all messages IS the guide's locale set (e.g., `["en", "es", "pt-br"]`).
3. For Fluent-backed blocks (locale-specific text), the embedding gets the locale(s) from which the resolved body was drawn (typically "en").
4. For inline content blocks (nushell scripts, code) that are locale-neutral, the embedding gets ALL locales present in the guide.

**Locale ID resolution:** We need to look up `rev.fluent_message_locales.id` for each locale tag. This is a small table (typically <10 rows). A single query at the start of the pipeline provides the mapping.

#### New SQL File for Locale ID Resolution

**File:** `crates/dapple-data/assets/sql/fluent_messages/fluent_message_bodies/select_fluent_message_locale_ids.sql` (new)

```sql
-- Resolve locale tags to locale IDs.
-- Parameters: $1 locale tags (TEXT[])
SELECT id, tag
FROM rev.fluent_message_locales
WHERE tag = ANY($1::TEXT[]);
```

### 3e. The Main Embedding Function

**File:** `crates/dapple-prompt-api/src/handler/guide_embeddings.rs` (new)

This module contains the embedding generation pipeline, called from `guide_ingest.rs` after the transaction commits.

```rust
/// Generate embeddings for a guide after successful ingest.
///
/// This function is intentionally fire-and-forget: any error is logged as a
/// warning but never propagates to the caller. Guide ingest should never fail
/// because of embedding generation.
pub async fn generate_embeddings_for_guide(
    data: &Arc<AppState>,
    guide_id: Uuid,
) {
    if let Err(e) = generate_embeddings_inner(data, guide_id).await {
        log::warn!(
            "Embedding generation failed for guide {}: {}. Guide was stored without embeddings.",
            guide_id, e
        );
    }
}

async fn generate_embeddings_inner(
    data: &Arc<AppState>,
    guide_id: Uuid,
) -> Result<(), String> {
    // 0. Check if conduit URL is configured
    let conduit_url = data.conduit_url.as_ref()
        .ok_or_else(|| "DAPPLE_CONDUIT_URL not set; skipping embedding generation".to_string())?;
    let client = EmbeddingClient::new(conduit_url);

    let pool = &data.db;

    // 1. Load the guide with full chapter/block hierarchy
    let guide = select_guide_model_by_object_id(pool, guide_id).await
        .map_err(|e| format!("Failed to load guide: {}", e))?;

    // 2. Collect all fluent_message_ids from blocks
    let mut fluent_ids: Vec<Uuid> = Vec::new();
    for chapter in &guide.chapters {
        for block in &chapter.blocks {
            if let Some(fm_id) = block.fluent_message_id {
                fluent_ids.push(fm_id);
            }
        }
    }
    fluent_ids.sort();
    fluent_ids.dedup();

    // 3. Batch-resolve all Fluent bodies
    let fluent_bodies = select_fluent_bodies_by_message_ids(pool, &fluent_ids).await
        .map_err(|e| format!("Failed to resolve Fluent bodies: {}", e))?;

    // Build lookup: (message_id, locale) -> body
    let mut body_map: HashMap<(Uuid, String), String> = HashMap::new();
    let mut all_locale_tags: HashSet<String> = HashSet::new();
    for row in &fluent_bodies {
        body_map.insert((row.fluent_message_id, row.locale_tag.clone()), row.body.clone());
        all_locale_tags.insert(row.locale_tag.clone());
    }

    // 4. Resolve locale tag -> locale ID mapping
    let locale_tags_vec: Vec<String> = all_locale_tags.iter().cloned().collect();
    let locale_id_rows = select_fluent_message_locale_ids(pool, &locale_tags_vec).await
        .map_err(|e| format!("Failed to resolve locale IDs: {}", e))?;
    let locale_tag_to_id: HashMap<String, i16> = locale_id_rows
        .into_iter()
        .map(|r| (r.tag, r.id))
        .collect();

    // Default locale if nothing found
    let all_locale_ids: Vec<i16> = locale_tag_to_id.values().cloned().collect();

    // 5. Load directive type and tag lookups
    let directive_types = load_directive_type_lookup_pool(pool).await
        .map_err(|e| format!("Failed to load directive types: {}", e))?;
    let tag_lookup = load_tag_lookup_pool(pool).await
        .map_err(|e| format!("Failed to load tag lookup: {}", e))?;

    // 6. Extract embeddable blocks
    let embeddable_types = ["prose", "stage", "step", "block", "ref"];
    let mut blocks_to_embed: Vec<EmbeddableBlock> = Vec::new();
    let mut block_position_in_chapter: usize = 0;

    for chapter in &guide.chapters {
        block_position_in_chapter = 0;
        for block in &chapter.blocks {
            if !embeddable_types.contains(&block.directive_type_key.as_str()) {
                block_position_in_chapter += 1;
                continue;
            }

            // Resolve text: inline content takes precedence, then Fluent body
            let text = if let Some(ref content) = block.content {
                content.clone()
            } else if let Some(fm_id) = block.fluent_message_id {
                // Try "en" first, then first available
                body_map.get(&(fm_id, "en".to_string()))
                    .cloned()
                    .or_else(|| {
                        fluent_bodies.iter()
                            .find(|r| r.fluent_message_id == fm_id)
                            .map(|r| r.body.clone())
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };

            if text.trim().is_empty() {
                block_position_in_chapter += 1;
                continue;
            }

            let directive_type_id = *directive_types.get(&block.directive_type_key)
                .ok_or_else(|| format!("Unknown directive type: {}", block.directive_type_key))?;

            // Determine if this block is locale-specific or locale-neutral
            let is_locale_neutral = block.content.is_some(); // inline content = locale-neutral

            blocks_to_embed.push(EmbeddableBlock {
                guide_id: guide.id,
                chapter_id: chapter.id,
                block_id: block.id,
                directive_type_id,
                directive_type_key: block.directive_type_key.clone(),
                text,
                tags: block.tags.clone(),
                chapter_title: chapter.title.clone(),
                block_name: block.name.clone(),
                position: block_position_in_chapter,
                is_locale_neutral,
            });

            block_position_in_chapter += 1;
        }
    }

    if blocks_to_embed.is_empty() {
        log::info!("Guide {} has no embeddable blocks; skipping embedding generation", guide_id);
        return Ok(());
    }

    // 7. Call Conduit for embedding generation (single batch HTTP call)
    let texts: Vec<String> = blocks_to_embed.iter().map(|b| b.text.clone()).collect();
    let embeddings = client.embed(&texts).await?;
    let embeddings = match embeddings {
        Some(e) => e,
        None => {
            log::info!("Conduit embedding service not available; skipping for guide {}", guide_id);
            return Ok(());
        }
    };

    if embeddings.len() != texts.len() {
        return Err(format!(
            "Embedding count mismatch: expected {}, got {}",
            texts.len(), embeddings.len()
        ));
    }

    // 8. Delete existing embeddings for this guide (atomic swap)
    delete_guide_content_embeddings_by_guide_id(pool, guide_id).await
        .map_err(|e| format!("Failed to delete old embeddings: {}", e))?;

    // 9. Build metadata and insert embeddings
    let metadata: Vec<serde_json::Value> = blocks_to_embed.iter()
        .map(|b| build_embedding_metadata(b, &guide))
        .collect();

    let insert_args = InsertGuideContentEmbeddingArgs {
        guide_ids: blocks_to_embed.iter().map(|b| b.guide_id).collect(),
        chapter_ids: blocks_to_embed.iter().map(|b| b.chapter_id).collect(),
        block_ids: blocks_to_embed.iter().map(|b| b.block_id).collect(),
        directive_type_ids: blocks_to_embed.iter().map(|b| b.directive_type_id).collect(),
        chunk_texts: texts,
        embeddings,
        metadata,
    };

    let inserted = insert_guide_content_embeddings(pool, insert_args).await
        .map_err(|e| format!("Failed to insert embeddings: {}", e))?;

    // 10. Insert tag associations
    let mut tag_emb_ids: Vec<Uuid> = Vec::new();
    let mut tag_ids: Vec<i16> = Vec::new();

    for (i, block) in blocks_to_embed.iter().enumerate() {
        let emb_id = inserted[i].id;
        if block.tags.is_empty() {
            // Untagged blocks: no tag associations (they are visible to all audiences)
            // The search query uses a LEFT JOIN pattern that includes untagged embeddings.
            // If a "universal" tag exists, associate it here instead.
            if let Some(&universal_id) = tag_lookup.get("universal") {
                tag_emb_ids.push(emb_id);
                tag_ids.push(universal_id);
            }
        } else {
            for tag_key in &block.tags {
                if let Some(&tag_id) = tag_lookup.get(tag_key) {
                    tag_emb_ids.push(emb_id);
                    tag_ids.push(tag_id);
                } else {
                    log::warn!("Unknown tag '{}' on block {}; skipping tag association", tag_key, block.block_id);
                }
            }
        }
    }

    if !tag_emb_ids.is_empty() {
        insert_guide_embedding_tags(pool, InsertGuideEmbeddingTagArgs {
            embedding_ids: tag_emb_ids,
            tag_ids,
        }).await.map_err(|e| format!("Failed to insert embedding tags: {}", e))?;
    }

    // 11. Insert locale associations
    let mut locale_emb_ids: Vec<Uuid> = Vec::new();
    let mut locale_ids: Vec<i16> = Vec::new();

    for (i, block) in blocks_to_embed.iter().enumerate() {
        let emb_id = inserted[i].id;
        if block.is_locale_neutral {
            // Locale-neutral blocks get ALL locales in the guide
            for &loc_id in &all_locale_ids {
                locale_emb_ids.push(emb_id);
                locale_ids.push(loc_id);
            }
        } else {
            // Locale-specific: associate with "en" (or the locale used for embedding)
            if let Some(&en_id) = locale_tag_to_id.get("en") {
                locale_emb_ids.push(emb_id);
                locale_ids.push(en_id);
            } else if let Some((&ref first_tag, &first_id)) = locale_tag_to_id.iter().next() {
                locale_emb_ids.push(emb_id);
                locale_ids.push(first_id);
            }
        }
    }

    if !locale_emb_ids.is_empty() {
        insert_guide_embedding_locales(pool, InsertGuideEmbeddingLocaleArgs {
            embedding_ids: locale_emb_ids,
            locale_ids,
        }).await.map_err(|e| format!("Failed to insert embedding locales: {}", e))?;
    }

    log::info!(
        "Generated {} embeddings for guide {} ({} tags, {} locale associations)",
        inserted.len(), guide_id,
        tag_emb_ids.len(), locale_emb_ids.len()
    );

    Ok(())
}
```

### 3f. Metadata Construction

The `build_embedding_metadata` function constructs the JSONB metadata for each embedding, following the schema from the Phase 2 implementation plan.

```rust
fn build_embedding_metadata(
    block: &EmbeddableBlock,
    guide: &GuideModel,
) -> serde_json::Value {
    serde_json::json!({
        "guide_slug": guide.slug,
        "guide_title": guide.title,
        "chapter_title": block.chapter_title,
        "chapter_id": block.chapter_id.to_string(),
        "block_id": block.block_id.to_string(),
        "block_name": block.block_name,
        "directive_type": block.directive_type_key,
        "position": block.position,
        "has_emphasis": block.text.contains("**") || block.text.contains("__"),
        "has_annotations": !block.tags.is_empty(),
    })
}
```

**Note:** The `depth`, `parent_stage_name`, `parent_stage_id`, and `brief_unit_keys` fields from the original metadata schema are deferred. Computing these requires traversing the guide's stage/step hierarchy, which is not available at this point because the guide model does not carry the playbook structure. These fields can be added in a follow-up when the embedding pipeline is enhanced to cross-reference the derived playbook.

### 3g. Internal Types

```rust
/// A block that has been resolved and is ready for embedding.
struct EmbeddableBlock {
    guide_id: Uuid,
    chapter_id: Uuid,
    block_id: Uuid,
    directive_type_id: i16,
    directive_type_key: String,
    text: String,
    tags: Vec<String>,
    chapter_title: String,
    block_name: Option<String>,
    position: usize,
    is_locale_neutral: bool,
}
```

---

## 4. Hook Points in guide_ingest.rs

### POST Path (new guide)

**File:** `crates/dapple-prompt-api/src/handler/guide_ingest.rs`
**Function:** `materialize_guide_with_hash`
**Location:** After line ~542, after `resolve_unresolved_guide_pins`, before the final `Ok(...)` return.

```rust
// Existing line ~542:
// resolve_unresolved_guide_pins(&data.db).await.map_err(|e| ...)?;

// NEW: Generate embeddings asynchronously (fire-and-forget)
let embed_data = data.clone();
let embed_guide_id = guide_model.id;
tokio::spawn(async move {
    generate_embeddings_for_guide(&embed_data, embed_guide_id).await;
});

// Existing return:
Ok(IngestGuideResponse { ... })
```

### PATCH Path (new version)

**File:** `crates/dapple-prompt-api/src/handler/guide_ingest.rs`
**Function:** `materialize_guide_version_with_hash`
**Location:** After line ~657, same position as POST path.

```rust
// Same pattern:
let embed_data = data.clone();
let embed_guide_id = guide_model.id;
tokio::spawn(async move {
    generate_embeddings_for_guide(&embed_data, embed_guide_id).await;
});
```

### Why `tokio::spawn` (fire-and-forget)?

1. **Embedding is slow:** ONNX inference for 20-50 blocks takes 1-5 seconds. The API should return the ingest response immediately.
2. **Embedding is non-critical:** The guide is fully functional without embeddings. Search and RAG are additive features.
3. **Embedding is idempotent:** The delete-then-insert pattern means re-running always produces correct results. If the spawn panics, the next ingest will regenerate.

---

## 5. Lookup Table Helpers (Pool-Based)

The existing `load_directive_type_lookup` and `load_tag_lookup` functions in `guide_ingest.rs` operate within a transaction (`&mut Transaction<...>`). The embedding pipeline runs OUTSIDE the transaction, so it needs pool-based variants.

**File:** `crates/dapple-data/src/query/guides.rs` (add to existing file)

```rust
/// Load directive type lookup from pool (for use outside transactions).
pub async fn load_directive_type_lookup_pool(
    pool: &DbPool,
) -> Result<HashMap<String, i16>, DataError> {
    let rows = sqlx::query_as::<_, (i16, String)>(
        "SELECT id, key FROM rev.guide_block_directive_types"
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows.into_iter().map(|(id, key)| (key, id)).collect())
}

/// Load tag lookup from pool (for use outside transactions).
pub async fn load_tag_lookup_pool(
    pool: &DbPool,
) -> Result<HashMap<String, i16>, DataError> {
    let rows = sqlx::query_as::<_, (i16, String)>(
        "SELECT id, key FROM rev.guide_tags"
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows.into_iter().map(|(id, key)| (key, id)).collect())
}
```

---

## 6. Data Layer Implementation (dapple-data)

The Phase 2 plan defined models and SQL files but left Rust query functions as `todo!()`. This section specifies the complete implementations.

### 6a. Model File

**File:** `crates/dapple-data/src/model/guide_content_embeddings.rs` (create new)

Implement exactly as specified in Phase 2 plan section 3. Register in `crates/dapple-data/src/model/mod.rs` with:

```rust
pub mod guide_content_embeddings;
```

### 6b. Query File

**File:** `crates/dapple-data/src/query/guide_content_embeddings.rs` (create new)

Register in `crates/dapple-data/src/query/mod.rs` with:

```rust
pub mod guide_content_embeddings;
```

Implementation notes for each function:

**`insert_guide_content_embeddings`:** The SQL uses `$6::vector(768)[]`. With the `pgvector` crate's SQLx integration, bind `Vec<pgvector::Vector>`. If `query_file_as!` cannot handle the vector type at compile time, fall back to `sqlx::query_as()` with inline SQL (this is the documented exception for pgvector).

**`delete_guide_content_embeddings_by_guide_id`:** Simple `DELETE ... WHERE guide_id = $1 RETURNING id`. The `ON DELETE CASCADE` on junction tables handles cleanup.

**`insert_guide_embedding_tags` and `insert_guide_embedding_locales`:** Standard UNNEST pattern, both already have SQL files.

### 6c. Dependencies

Add to `crates/dapple-data/Cargo.toml`:

```toml
pgvector = { version = "0.4", features = ["sqlx"] }
```

Add to `crates/dapple-prompt-api/Cargo.toml`:

```toml
reqwest = { version = "0.12", features = ["json"] }
```

(`reqwest` may already be a dependency -- verify before adding.)

---

## 7. Error Handling Strategy

### Principle: Embedding Failure Never Fails Guide Ingest

The entire embedding pipeline is wrapped in a catch-all:

```rust
pub async fn generate_embeddings_for_guide(data: &Arc<AppState>, guide_id: Uuid) {
    if let Err(e) = generate_embeddings_inner(data, guide_id).await {
        log::warn!("Embedding generation failed for guide {}: {}", guide_id, e);
    }
}
```

### Specific Error Handling

| Error | Behavior |
|-------|----------|
| `DAPPLE_CONDUIT_URL` not set | Skip silently (log at debug level) |
| Conduit `/embed` returns 503 | Log info, skip -- embedding service not available |
| Conduit network error | Log warning, skip -- transient failure |
| Conduit returns wrong embedding count | Log error, skip -- indicates a bug |
| Database error during delete/insert | Log error, skip -- will be retried on next ingest |
| Unknown directive type key | Return error, skip entire guide -- indicates data inconsistency |
| Empty embeddable blocks | Log info, return -- guide has no embeddable content |

### Re-embedding on Retry

Because the pipeline is idempotent (delete-all-then-insert-all), if a guide is re-ingested (PATCH), the new version's embeddings will replace any partial or failed embeddings from a previous attempt.

---

## 8. Testing Strategy

### Integration Tests

Add a trial to `tests/user_lifecycle.rs` that exercises the full embedding pipeline:

```rust
fn trial_guide_embeddings() -> Trial {
    // Wait for: guide ingest trials
    // 1. Ingest a test guide with mixed block types
    //    (prose with Fluent content, nushell with inline content)
    // 2. Verify embeddings exist: SELECT count(*) FROM rev.guide_content_embeddings WHERE guide_id = ...
    // 3. Verify tag associations exist
    // 4. Verify locale associations exist
    // 5. Re-ingest (PATCH) and verify old embeddings were replaced
}
```

### Test Consumer for Conduit Embed Endpoint

**File:** `crates/dapple-prompt-api/tests/consumer/guide_embeddings.rs` (new)

Tests that call the Conduit `/embed` endpoint directly (requires Conduit running in test environment).

### Unit Tests for Metadata Construction

**File:** `crates/dapple-prompt-api/src/handler/guide_embeddings.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_metadata_prose_block() {
        // Test metadata construction for a prose block
    }

    #[test]
    fn test_build_metadata_emphasis_detection() {
        // Test that **bold** and __italic__ are detected
    }
}
```

### Stub Testing Without Conduit

For tests where Conduit is not available, the embedding generation silently degrades. Tests should verify:
- Guide ingest succeeds even without `DAPPLE_CONDUIT_URL` set.
- Guide ingest succeeds when Conduit returns 503.
- No embeddings are created in these cases.

---

## 9. File Creation Checklist

### New Files

| File | Purpose |
|------|---------|
| `crates/dapple-prompt-conduit/src/handler/embed.rs` | Conduit `/embed` endpoint handler |
| `crates/dapple-prompt-api/src/service/embedding_client.rs` | HTTP client for Conduit embed |
| `crates/dapple-prompt-api/src/handler/guide_embeddings.rs` | Embedding generation pipeline |
| `crates/dapple-data/src/model/guide_content_embeddings.rs` | Rust models for embeddings |
| `crates/dapple-data/src/query/guide_content_embeddings.rs` | Rust query functions for embeddings |
| `crates/dapple-data/assets/sql/fluent_messages/fluent_message_bodies/select_fluent_bodies_by_message_ids.sql` | Batch Fluent body resolution |
| `crates/dapple-data/assets/sql/fluent_messages/fluent_message_bodies/select_fluent_message_locale_ids.sql` | Locale tag to ID mapping |

### Modified Files

| File | Change |
|------|--------|
| `crates/dapple-prompt-conduit/src/handler/mod.rs` | Add `pub mod embed; pub use embed::handle_embed;` |
| `crates/dapple-prompt-conduit/src/main.rs` | Add `/embed` route, initialize EmbeddingService |
| `crates/dapple-prompt-api/src/lib.rs` | Add `conduit_url: Option<String>` to `AppState` |
| `crates/dapple-prompt-api/src/handler/guide_ingest.rs` | Add `tokio::spawn(generate_embeddings_for_guide(...))` at two hook points |
| `crates/dapple-prompt-api/src/handler/mod.rs` | Add `pub mod guide_embeddings;` |
| `crates/dapple-data/src/model/mod.rs` | Add `pub mod guide_content_embeddings;` |
| `crates/dapple-data/src/query/mod.rs` | Add `pub mod guide_content_embeddings;` |
| `crates/dapple-data/src/query/guides.rs` | Add `load_directive_type_lookup_pool()`, `load_tag_lookup_pool()` |
| `crates/dapple-data/src/query/fluent_messages.rs` | Add `select_fluent_bodies_by_message_ids()`, `select_fluent_message_locale_ids()` |
| `crates/dapple-data/Cargo.toml` | Add `pgvector = { version = "0.4", features = ["sqlx"] }` |
| `crates/dapple-prompt-api/Cargo.toml` | Add `reqwest` if not already present |

---

## 10. Environment Configuration

### Required for Conduit

```env
# Enable embedding service in Conduit
EMBEDDING_ENABLED=true
# Optional: override model (default: lightonai/modernbert-embed-large)
EMBEDDING_MODEL_ID=lightonai/modernbert-embed-large
# Optional: Matryoshka dimension (default: 768, must match migration DDL)
EMBEDDING_DIMENSION=768
```

### Required for API

```env
# Conduit URL for embedding requests
DAPPLE_CONDUIT_URL=http://dapple-prompt-conduit:8082
```

### K8s Manifest Updates

Add `DAPPLE_CONDUIT_URL` to the API's environment in `assets/k8s/registry/dapple-prompt-api.yaml`:

```yaml
env:
  - name: DAPPLE_CONDUIT_URL
    value: "http://dapple-prompt-conduit:8082"
```

Add `EMBEDDING_ENABLED` to Conduit's environment in `assets/k8s/registry/dapple-prompt-conduit.yaml`:

```yaml
env:
  - name: EMBEDDING_ENABLED
    value: "true"
```

---

## 11. Open Questions and Risks

### Resolved Questions

1. **Service boundary:** Option C -- API orchestrates, Conduit computes via `/embed`.
2. **Block content resolution:** Batch-fetch Fluent bodies by message_id array, prefer "en" locale.
3. **Guide data access:** `select_guide_model_by_object_id()` returns the full chapter/block hierarchy including `fluent_message_id`, `content`, `directive_type_key`, and `tags` per block. This is sufficient.
4. **Locale determination:** Derived from the Fluent message bodies present for the guide. Inline content blocks are locale-neutral (all locales).

### Remaining Risks

1. **ONNX model availability on first deploy.** The ModernBERT model must be pre-downloaded to `ONNX_MODEL_PATH`. If missing, Conduit returns 503 and embeddings are skipped. The prefetch command (`dapple-prompt-conduit prefetch`) does not yet cover the ONNX model -- only the Candle/mistral.rs models.

2. **Large guides.** A guide with 200+ blocks would produce a single `/embed` request with 200 texts. The ModernBERT model processes these sequentially (no GPU batching in ONNX CPU mode). This could take 30+ seconds. The 120-second timeout is generous, but monitoring should alert on slow requests. Future improvement: chunk the batch into groups of 32.

3. **pgvector + SQLx type compatibility.** The `pgvector` crate must integrate correctly with the SQLx version used in the project. If `query_file_as!` cannot handle `vector(768)` parameters, the fallback to `sqlx::query_as()` with inline SQL is acceptable and documented in the Phase 2 plan.

4. **Metadata completeness.** The `depth`, `parent_stage_name`, `parent_stage_id`, and `brief_unit_keys` fields are omitted from the initial metadata because they require cross-referencing the derived playbook structure. A follow-up should add these when playbook-to-guide block mapping is available.

5. **Race condition on rapid re-ingest.** If a guide is PATCH-ingested twice in quick succession, both spawned embedding tasks will try to delete-then-insert. The second task's DELETE will remove the first task's freshly-inserted embeddings. The second task's INSERT will then correctly populate. This is acceptable because the second version's embeddings are correct. However, there is a brief window where the first version's embeddings exist and the second version has not yet generated its own. A future enhancement could use a lock (advisory lock keyed on guide_id) to serialize concurrent embedding generation for the same guide.
