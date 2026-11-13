# pgvector Phase 2: Implementation Specification

**Status:** Draft
**Created:** 2026-02-24
**Last Updated:** 2026-02-24
**Parent Plan:** `docs/plans/pg-vector-searching.md`
**Dependencies:** Migration 15 (content-addressable versioning), EmbeddingService (dapple-prompt-conduit)

---

## Executive Summary

This document is the corrected, implementation-ready specification for Phase 2 of the pgvector search plan. It reconciles the original plan (`pg-vector-searching.md`) with the actual codebase state as of migration 15. The original plan contained several incorrect type assumptions (BIGINT PKs, 1536-dimension vectors, `object_id` columns) that do not match the project conventions. This spec provides exact DDL, Rust models, SQL queries, and hook points that an implementation agent can execute without ambiguity.

Phase 2 delivers: the pgvector extension, the `rev.guide_content_embeddings` table with correct FK types, junction tables for tags and locales (reusing the existing `rev.fluent_message_locales` table rather than creating a new locales table), and the Rust data layer (models + queries) needed to insert and query embeddings.

---

## 1. Corrections from Original Plan

| Original Plan | Actual Codebase | Corrected Value |
|---|---|---|
| `guide_content_embeddings.id BIGINT GENERATED ALWAYS AS IDENTITY` | Principal/dependent tables use `UUID DEFAULT uuidv7()` PK | `id UUID DEFAULT uuidv7() PRIMARY KEY` |
| `object_id UUID DEFAULT gen_random_uuid() UNIQUE` | The `id` field IS the UUID PK; no separate `object_id` | **Removed.** `id` is the UUID PK. |
| `guide_id BIGINT NOT NULL REFERENCES rev.guides(id)` | `rev.guides.id` is `UUID DEFAULT uuidv7()` | `guide_id UUID NOT NULL REFERENCES rev.guides(id)` |
| `chapter_id BIGINT NOT NULL REFERENCES rev.guide_chapters(id)` | `rev.guide_chapters.id` is `UUID DEFAULT uuidv7()` | `chapter_id UUID NOT NULL REFERENCES rev.guide_chapters(id)` |
| `block_id BIGINT NOT NULL REFERENCES rev.guide_blocks(id)` | `rev.guide_blocks.id` is `UUID DEFAULT uuidv7()` | `block_id UUID NOT NULL REFERENCES rev.guide_blocks(id)` |
| `embedding vector(1536)` | ModernBERT backend uses 768 dimensions (Matryoshka default) | `embedding vector(768)` |
| `embedding_id BIGINT` in junction tables | FK to a UUID PK table | `embedding_id UUID` |
| Junction table PKs: `BIGINT GENERATED ALWAYS AS IDENTITY` | Associative tables in this codebase use `UUID DEFAULT uuidv7()` | `id UUID DEFAULT uuidv7() PRIMARY KEY` |
| `version_major/minor/patch` references | Migration 15 replaced semver with `content_hash TEXT` | References updated to `content_hash` |
| Plan proposes `rev.locales` | `rev.fluent_message_locales` already exists with `(id SMALLINT, tag TEXT)` | **Reuse `rev.fluent_message_locales`** instead of creating a new table |

### Critical Design Decision: Reuse `rev.fluent_message_locales`

The original plan proposes a new `rev.locales` table. However, `rev.fluent_message_locales` already exists in migration 2 with the exact structure needed:

```sql
-- Already exists in migration 2
CREATE TABLE rev.fluent_message_locales (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tag TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);
```

This table is auto-populated by the Fluent message upsert pipeline (`insert_fluent_message_bodies.sql` uses `ON CONFLICT (tag) DO NOTHING`). Locale tags like `en`, `es`, `pt-br` are inserted when guides with those locales are ingested. The embedding junction table should reference this existing table. No new `rev.locales` table is needed.

### Embeddings Table Classification: Owned Dependent

The embeddings table is an **owned dependent** of guides. It has no independent lifecycle -- embeddings are created when a guide version is designated canonical, and deleted when a new canonical version replaces it. It does not need to be a versioned principal. However, per codebase convention, it still uses a UUID PK (not BIGINT) because it references and is referenced by other UUID-PK tables, and the associative tables follow the same pattern as `rev.guide_block_tags`.

---

## 2. Migration DDL

**File:** `crates/dapple-data/migrations/00000000000016_pgvector_embeddings.sql`

```sql
-- Migration 16: pgvector extension and guide content embeddings
-- Enables vector similarity search for guide content.
-- Uses the existing rev.fluent_message_locales table for locale references.

-- ============================================================================
-- Enable pgvector extension
-- ============================================================================
CREATE EXTENSION IF NOT EXISTS vector;

-- ============================================================================
-- rev.guide_content_embeddings — One row per embedded block
-- ============================================================================
CREATE TABLE rev.guide_content_embeddings (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL REFERENCES rev.guides (id),
    chapter_id UUID NOT NULL REFERENCES rev.guide_chapters (id),
    block_id UUID NOT NULL REFERENCES rev.guide_blocks (id),
    directive_type_id SMALLINT NOT NULL REFERENCES rev.guide_block_directive_types (id),
    chunk_text TEXT NOT NULL,
    embedding vector(768) NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

-- FK indexes
CREATE INDEX guide_content_embeddings_guide_idx
    ON rev.guide_content_embeddings (guide_id);
CREATE INDEX guide_content_embeddings_chapter_idx
    ON rev.guide_content_embeddings (chapter_id);
CREATE INDEX guide_content_embeddings_block_idx
    ON rev.guide_content_embeddings (block_id);
CREATE INDEX guide_content_embeddings_directive_type_idx
    ON rev.guide_content_embeddings (directive_type_id);

-- Vector similarity index (IVFFlat with cosine distance)
-- NOTE: IVFFlat requires data to exist before building the index.
-- For initial deployment with < 1000 rows, this index will be under-trained.
-- Consider rebuilding with `REINDEX INDEX CONCURRENTLY` after initial data load,
-- or switching to HNSW (which does not require pre-existing data) if recall
-- needs improvement.
CREATE INDEX guide_content_embeddings_vector_idx
    ON rev.guide_content_embeddings
    USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);

-- ============================================================================
-- rev.guide_embedding_tags — Junction: embedding <-> guide_tags
-- ============================================================================
CREATE TABLE rev.guide_embedding_tags (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    embedding_id UUID NOT NULL REFERENCES rev.guide_content_embeddings (id) ON DELETE CASCADE,
    tag_id SMALLINT NOT NULL REFERENCES rev.guide_tags (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (embedding_id, tag_id)
);

CREATE INDEX guide_embedding_tags_embedding_idx
    ON rev.guide_embedding_tags (embedding_id);
CREATE INDEX guide_embedding_tags_tag_idx
    ON rev.guide_embedding_tags (tag_id);

-- ============================================================================
-- rev.guide_embedding_locales — Junction: embedding <-> fluent_message_locales
-- ============================================================================
CREATE TABLE rev.guide_embedding_locales (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    embedding_id UUID NOT NULL REFERENCES rev.guide_content_embeddings (id) ON DELETE CASCADE,
    locale_id SMALLINT NOT NULL REFERENCES rev.fluent_message_locales (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (embedding_id, locale_id)
);

CREATE INDEX guide_embedding_locales_embedding_idx
    ON rev.guide_embedding_locales (embedding_id);
CREATE INDEX guide_embedding_locales_locale_idx
    ON rev.guide_embedding_locales (locale_id);
```

### DDL Notes

- All PKs are `UUID DEFAULT uuidv7()` per project convention.
- All FKs to `rev.guides`, `rev.guide_chapters`, `rev.guide_blocks` are `UUID` matching those tables' PK types.
- `ON DELETE CASCADE` on junction table FKs enables atomic re-embedding: `DELETE FROM rev.guide_content_embeddings WHERE guide_id = $1` cascades to both junction tables.
- The `embedding vector(768)` dimension matches `EmbeddingConfig::default().dimension` (768) and the Matryoshka truncation in `ModernBertBackend::truncate_dimension`.
- `rev.fluent_message_locales` is reused -- no new locale table is created.
- The IVFFlat index is created eagerly. For a cold-start database with zero rows, the index will train on empty data. This is acceptable for development; production should `REINDEX CONCURRENTLY` after initial bulk embedding.

---

## 3. Rust Models

**File:** `crates/dapple-data/src/model/guide_content_embeddings.rs`

```rust
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for guide content embeddings.
/// All vectors MUST be the same length.
#[derive(Clone, Debug)]
pub struct InsertGuideContentEmbeddingArgs {
    pub guide_ids: Vec<Uuid>,
    pub chapter_ids: Vec<Uuid>,
    pub block_ids: Vec<Uuid>,
    pub directive_type_ids: Vec<i16>,
    pub chunk_texts: Vec<String>,
    pub embeddings: Vec<Vec<f32>>,
    pub metadata: Vec<serde_json::Value>,
}

impl_insert_column_args!(
    InsertGuideContentEmbeddingArgs,
    [guide_ids, chapter_ids, block_ids, directive_type_ids, chunk_texts, embeddings, metadata]
);

/// Column-wise insert arguments for guide embedding tags.
#[derive(Clone, Debug)]
pub struct InsertGuideEmbeddingTagArgs {
    pub embedding_ids: Vec<Uuid>,
    pub tag_ids: Vec<i16>,
}

impl_insert_column_args!(InsertGuideEmbeddingTagArgs, [embedding_ids, tag_ids]);

/// Column-wise insert arguments for guide embedding locales.
#[derive(Clone, Debug)]
pub struct InsertGuideEmbeddingLocaleArgs {
    pub embedding_ids: Vec<Uuid>,
    pub locale_ids: Vec<i16>,
}

impl_insert_column_args!(InsertGuideEmbeddingLocaleArgs, [embedding_ids, locale_ids]);

// ============================================================================
// Inserted Models (INSERT...RETURNING)
// ============================================================================

/// Inserted embedding model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideContentEmbeddingModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub block_id: Uuid,
    pub directive_type_id: i16,
    pub created_at: OffsetDateTime,
}

/// Inserted embedding tag model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideEmbeddingTagModel {
    pub id: Uuid,
    pub embedding_id: Uuid,
    pub tag_id: i16,
    pub created_at: OffsetDateTime,
}

/// Inserted embedding locale model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideEmbeddingLocaleModel {
    pub id: Uuid,
    pub embedding_id: Uuid,
    pub locale_id: i16,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Query Result Models
// ============================================================================

/// Search result row from a similarity query.
/// Returned as a flat model from pgvector similarity queries.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideEmbeddingSearchResult {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub block_id: Uuid,
    pub directive_type_key: String,
    pub chunk_text: String,
    pub metadata: serde_json::Value,
    pub similarity: f64,
}

/// Public DTO for search results (API response).
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct GuideSearchResult {
    pub block_id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub directive_type: String,
    pub chunk_text: String,
    pub metadata: serde_json::Value,
    pub similarity: f64,
}

impl From<GuideEmbeddingSearchResult> for GuideSearchResult {
    fn from(m: GuideEmbeddingSearchResult) -> Self {
        Self {
            block_id: m.block_id,
            guide_id: m.guide_id,
            chapter_id: m.chapter_id,
            directive_type: m.directive_type_key,
            chunk_text: m.chunk_text,
            metadata: m.metadata,
            similarity: m.similarity,
        }
    }
}
```

### Model Registration

Add `pub mod guide_content_embeddings;` to `crates/dapple-data/src/model/mod.rs`.

---

## 4. SQL Query Files

All files in `crates/dapple-data/assets/sql/guide_content_embeddings/`.

### 4a. `insert_guide_content_embeddings.sql`

```sql
-- Insert guide content embeddings.
-- Vectors are passed as float4[] arrays (one per embedding row).
-- pgvector accepts float4[] as implicit cast to vector(768).
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],       -- guide_id[]
        $2::UUID[],       -- chapter_id[]
        $3::UUID[],       -- block_id[]
        $4::SMALLINT[],   -- directive_type_id[]
        $5::TEXT[],        -- chunk_text[]
        $6::vector(768)[], -- embedding[]
        $7::JSONB[]        -- metadata[]
    ) WITH ORDINALITY AS o(
        guide_id, chapter_id, block_id, directive_type_id,
        chunk_text, embedding, metadata, ord
    )
),
new_embeddings AS (
    INSERT INTO rev.guide_content_embeddings (
        id, guide_id, chapter_id, block_id,
        directive_type_id, chunk_text, embedding, metadata
    )
    SELECT
        uuidv7(),
        args.guide_id,
        args.chapter_id,
        args.block_id,
        args.directive_type_id,
        args.chunk_text,
        args.embedding,
        args.metadata
    FROM args
    ORDER BY args.ord
    RETURNING id, guide_id, chapter_id, block_id, directive_type_id, created_at
)
SELECT
    ne.id,
    ne.guide_id,
    ne.chapter_id,
    ne.block_id,
    ne.directive_type_id,
    ne.created_at
FROM new_embeddings ne
ORDER BY ne.id;
```

### 4b. `insert_guide_embedding_tags.sql`

```sql
-- Insert guide embedding tag associations.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],       -- embedding_id[]
        $2::SMALLINT[]    -- tag_id[]
    ) WITH ORDINALITY AS o(embedding_id, tag_id, ord)
),
new_tags AS (
    INSERT INTO rev.guide_embedding_tags (id, embedding_id, tag_id)
    SELECT uuidv7(), args.embedding_id, args.tag_id
    FROM args
    ORDER BY args.ord
    ON CONFLICT (embedding_id, tag_id) DO NOTHING
    RETURNING id, embedding_id, tag_id, created_at
)
SELECT
    nt.id,
    nt.embedding_id,
    nt.tag_id,
    nt.created_at
FROM new_tags nt
ORDER BY nt.id;
```

### 4c. `insert_guide_embedding_locales.sql`

```sql
-- Insert guide embedding locale associations.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],       -- embedding_id[]
        $2::SMALLINT[]    -- locale_id[]
    ) WITH ORDINALITY AS o(embedding_id, locale_id, ord)
),
new_locales AS (
    INSERT INTO rev.guide_embedding_locales (id, embedding_id, locale_id)
    SELECT uuidv7(), args.embedding_id, args.locale_id
    FROM args
    ORDER BY args.ord
    ON CONFLICT (embedding_id, locale_id) DO NOTHING
    RETURNING id, embedding_id, locale_id, created_at
)
SELECT
    nl.id,
    nl.embedding_id,
    nl.locale_id,
    nl.created_at
FROM new_locales nl
ORDER BY nl.id;
```

### 4d. `delete_guide_content_embeddings_by_guide_id.sql`

```sql
-- Delete all embeddings for a guide (cascades to junction tables).
-- Used when re-embedding a guide after canonical version change.
DELETE FROM rev.guide_content_embeddings
WHERE guide_id = $1
RETURNING id;
```

### 4e. `search_guide_content_embeddings.sql`

```sql
-- Similarity search with audience tag and locale filtering.
-- $1: query embedding (vector(768))
-- $2: tag keys (TEXT[]) -- e.g., ['product', 'universal']
-- $3: locale tag (TEXT) -- e.g., 'en'
-- $4: limit (INT)
SELECT
    e.id,
    e.guide_id,
    e.chapter_id,
    e.block_id,
    dt.key AS directive_type_key,
    e.chunk_text,
    e.metadata,
    1 - (e.embedding <=> $1::vector(768)) AS similarity
FROM rev.guide_content_embeddings e
JOIN rev.guide_block_directive_types dt ON dt.id = e.directive_type_id
JOIN rev.guide_embedding_tags et ON et.embedding_id = e.id
JOIN rev.guide_embedding_locales el ON el.embedding_id = e.id
JOIN rev.guide_tags gt ON gt.id = et.tag_id
JOIN rev.fluent_message_locales fml ON fml.id = el.locale_id
WHERE gt.key = ANY($2::TEXT[])
AND fml.tag = $3
ORDER BY e.embedding <=> $1::vector(768)
LIMIT $4;
```

### 4f. `search_guide_content_embeddings_by_guide_ids.sql`

```sql
-- Similarity search scoped to specific guides (for agent RAG).
-- $1: query embedding (vector(768))
-- $2: guide_ids (UUID[])
-- $3: tag keys (TEXT[])
-- $4: locale tag (TEXT)
-- $5: limit (INT)
SELECT
    e.id,
    e.guide_id,
    e.chapter_id,
    e.block_id,
    dt.key AS directive_type_key,
    e.chunk_text,
    e.metadata,
    1 - (e.embedding <=> $1::vector(768)) AS similarity
FROM rev.guide_content_embeddings e
JOIN rev.guide_block_directive_types dt ON dt.id = e.directive_type_id
JOIN rev.guide_embedding_tags et ON et.embedding_id = e.id
JOIN rev.guide_embedding_locales el ON el.embedding_id = e.id
JOIN rev.guide_tags gt ON gt.id = et.tag_id
JOIN rev.fluent_message_locales fml ON fml.id = el.locale_id
WHERE e.guide_id = ANY($2::UUID[])
AND gt.key = ANY($3::TEXT[])
AND fml.tag = $4
ORDER BY e.embedding <=> $1::vector(768)
LIMIT $5;
```

---

## 5. Rust Query Functions

**File:** `crates/dapple-data/src/query/guide_content_embeddings.rs`

```rust
use crate::error::DataError;
use crate::model::guide_content_embeddings::*;
use sqlx::PgPool;
use uuid::Uuid;

/// Insert guide content embeddings.
pub async fn insert_guide_content_embeddings(
    pool: &PgPool,
    args: InsertGuideContentEmbeddingArgs,
) -> Result<Vec<InsertedGuideContentEmbeddingModel>, DataError> {
    // NOTE: pgvector's vector type requires special handling with sqlx.
    // The embeddings Vec<Vec<f32>> must be passed as pgvector::Vector values.
    // The implementation agent should use the `pgvector` crate's `Vector` type
    // and bind via sqlx's `.bind()` rather than `query_file_as!`, because
    // `query_file_as!` cannot handle vector(768) parameters at compile time
    // without pgvector being enabled in the offline sqlx cache.
    //
    // Alternative: use `sqlx::query_as()` with inline SQL that casts
    // `$6::float4[][]` to `vector(768)[]`. See implementation notes below.
    todo!()
}

/// Insert guide embedding tag associations.
pub async fn insert_guide_embedding_tags(
    pool: &PgPool,
    args: InsertGuideEmbeddingTagArgs,
) -> Result<Vec<InsertedGuideEmbeddingTagModel>, DataError> {
    todo!()
}

/// Insert guide embedding locale associations.
pub async fn insert_guide_embedding_locales(
    pool: &PgPool,
    args: InsertGuideEmbeddingLocaleArgs,
) -> Result<Vec<InsertedGuideEmbeddingLocaleModel>, DataError> {
    todo!()
}

/// Delete all embeddings for a guide (used before re-embedding).
pub async fn delete_guide_content_embeddings_by_guide_id(
    pool: &PgPool,
    guide_id: Uuid,
) -> Result<Vec<Uuid>, DataError> {
    todo!()
}

/// Search guide content embeddings with audience and locale filtering.
pub async fn search_guide_content_embeddings(
    pool: &PgPool,
    query_embedding: &[f32],
    tag_keys: &[String],
    locale_tag: &str,
    limit: i32,
) -> Result<Vec<GuideEmbeddingSearchResult>, DataError> {
    todo!()
}

/// Search guide content embeddings scoped to specific guides.
pub async fn search_guide_content_embeddings_by_guide_ids(
    pool: &PgPool,
    query_embedding: &[f32],
    guide_ids: &[Uuid],
    tag_keys: &[String],
    locale_tag: &str,
    limit: i32,
) -> Result<Vec<GuideEmbeddingSearchResult>, DataError> {
    todo!()
}
```

### Query Registration

Add `pub mod guide_content_embeddings;` to `crates/dapple-data/src/query/mod.rs`.

---

## 6. pgvector + SQLx Integration Notes

### The `pgvector` Crate

The `pgvector` Rust crate provides a `pgvector::Vector` type that implements `sqlx::Type` and `sqlx::Encode/Decode` for the PostgreSQL `vector` type. This is required because:

1. `query_file_as!` needs type information at compile time
2. SQLx does not natively understand the `vector` type
3. The `pgvector` crate bridges this gap

**Add to `Cargo.toml` for `dapple-data`:**

```toml
[dependencies]
pgvector = { version = "0.4", features = ["sqlx"] }
```

### Binding Vectors

```rust
use pgvector::Vector;

// Convert Vec<f32> to pgvector::Vector for binding
let vectors: Vec<Vector> = args.embeddings
    .iter()
    .map(|v| Vector::from(v.clone()))
    .collect();

// Bind as $6::vector[] in the SQL
sqlx::query_as(...)
    .bind(&vectors)
    // ...
```

### Offline Mode Consideration

With `SQLX_OFFLINE=true`, the compile-time check needs the pgvector extension in `.sqlx/` metadata. After creating the migration and running it:

```bash
nu -c 'cargo sqlx migrate run'
nu -c 'cargo sqlx prepare --workspace'
```

If `query_file_as!` proves problematic with vector types, fall back to `sqlx::query_as()` with inline SQL (this is the only acceptable exception to the "no inline SQL" rule, documented as a known limitation of SQLx with pgvector).

---

## 7. Embedding Pipeline Hook Points

### Where Embedding Generation Should Be Triggered

The embedding pipeline triggers when a guide's canonical version is updated. There are two integration points to consider:

#### Option A: Post-Ingest Hook (Recommended for Phase 2)

After `materialize_guide_with_hash` and `materialize_guide_version_with_hash` in `crates/dapple-prompt-api/src/handler/guide_ingest.rs`, AFTER the transaction commits (step 9) and AFTER canonical pin resolution (step 10), add an async embedding generation step:

```
// Existing flow (guide_ingest.rs):
// 9. tx.commit()
// 10. resolve_unresolved_guide_pins()
// NEW:
// 11. generate_embeddings_for_guide(guide_id)
```

This runs outside the transaction because:
- Embedding generation is slow (ONNX inference for N blocks)
- A failure should not roll back the guide ingest
- The operation is idempotent (delete-then-reinsert)

**Exact location in `guide_ingest.rs`:**
- `materialize_guide_with_hash()`: after line ~542 (after `resolve_unresolved_guide_pins`)
- `materialize_guide_version_with_hash()`: after line ~657 (same position in the patch path)

#### Option B: Canonical Sync Hook (Deferred to Phase 3)

When a canonical manifest is synced via `POST /api/canonical/sync`, the handler resolves guide pins. After resolution, any newly-pinned guide versions should have their embeddings generated. This requires knowing which guides were newly resolved, which the current `canonical_sync_handler` does not track.

**Recommendation:** Implement Option A for Phase 2. Option B can be added in Phase 3 when the canonical sync handler is enhanced to return which guides were newly resolved.

### AppState Changes

The `EmbeddingService` must be available to the API handler. Two approaches:

**Approach 1: Add to AppState** (preferred if conduit and API share a process)

```rust
pub struct AppState {
    pub db: DbPool,
    pub embedding_service: Option<Arc<EmbeddingService>>,
}
```

**Approach 2: HTTP call to Conduit** (if embedding lives in a separate service)

The API handler calls a Conduit endpoint to trigger embedding generation. This is the cleaner separation if Conduit is a separate deployment.

**Recommendation for Phase 2:** Start with Approach 1. The `EmbeddingService` is lightweight (lazy model loading). If the API binary should not include ONNX runtime dependencies, the implementation agent should use Approach 2 and defer to Conduit via HTTP.

### Embedding Generation Logic

The embedding generation function (regardless of where it lives) follows this sequence:

```rust
async fn generate_embeddings_for_guide(
    pool: &PgPool,
    embedding_service: &EmbeddingService,
    guide_id: Uuid,
) -> Result<(), EmbeddingError> {
    // 1. Delete existing embeddings for this guide
    delete_guide_content_embeddings_by_guide_id(pool, guide_id).await?;

    // 2. Fetch the guide with all blocks (reuse existing select query)
    let guides = select_guides_by_object_ids(pool, &[guide_id]).await?;
    let guide = guides.into_iter().next().ok_or(EmbeddingError::NotFound)?;

    // 3. Filter to embeddable blocks (skip nushell, brief_pack, inference_profile)
    let embeddable_types = ["prose", "stage", "step", "block", "ref"];
    let mut blocks_to_embed = Vec::new();

    for chapter in &guide.chapters {
        for block in &chapter.blocks {
            if embeddable_types.contains(&block.directive_type_key.as_str()) {
                // Use block content if present, otherwise resolve Fluent body
                let text = block.content.clone()
                    .or_else(|| resolve_fluent_body(pool, block.fluent_message_id).await)
                    .unwrap_or_default();

                if !text.trim().is_empty() {
                    blocks_to_embed.push(EmbeddableBlock {
                        guide_id: guide.id,
                        chapter_id: chapter.id,
                        block_id: block.id,
                        directive_type_id: /* resolve from key */,
                        text,
                        tags: block.tags.clone(),
                        chapter_title: chapter.title.clone(),
                        block_name: block.name.clone(),
                    });
                }
            }
        }
    }

    // 4. Batch embed all texts
    let texts: Vec<String> = blocks_to_embed.iter().map(|b| b.text.clone()).collect();
    let embeddings = embedding_service.embed(&texts).await?;

    // 5. Build insert args
    let args = InsertGuideContentEmbeddingArgs {
        guide_ids: blocks_to_embed.iter().map(|b| b.guide_id).collect(),
        chapter_ids: blocks_to_embed.iter().map(|b| b.chapter_id).collect(),
        block_ids: blocks_to_embed.iter().map(|b| b.block_id).collect(),
        directive_type_ids: blocks_to_embed.iter().map(|b| b.directive_type_id).collect(),
        chunk_texts: texts,
        embeddings,
        metadata: blocks_to_embed.iter().map(|b| build_metadata(b, &guide)).collect(),
    };

    // 6. Insert embeddings
    let inserted = insert_guide_content_embeddings(pool, args).await?;

    // 7. Insert tag associations
    // For each embedding, copy the source block's tags.
    // Untagged blocks get the 'universal' tag.
    let (tag_emb_ids, tag_ids) = build_tag_associations(&blocks_to_embed, &inserted);
    insert_guide_embedding_tags(pool, InsertGuideEmbeddingTagArgs {
        embedding_ids: tag_emb_ids,
        tag_ids,
    }).await?;

    // 8. Insert locale associations
    // Use guide's authoring_locale from path_details.
    // Locale-neutral blocks (code, diagrams) get all active locales.
    let (locale_emb_ids, locale_ids) = build_locale_associations(&blocks_to_embed, &inserted, &guide);
    insert_guide_embedding_locales(pool, InsertGuideEmbeddingLocaleArgs {
        embedding_ids: locale_emb_ids,
        locale_ids,
    }).await?;

    Ok(())
}
```

---

## 8. Metadata Schema

The `metadata` JSONB column stores display and ranking metadata per embedding. This matches the original plan with one correction: `chapter_object_id` and `block_object_id` are renamed to `chapter_id` and `block_id` (there is no `object_id` concept in this codebase -- `id` IS the UUID PK).

```json
{
    "guide_slug": "api-consumer-guide",
    "guide_title": "API Consumer Guide",
    "chapter_title": "Authentication",
    "chapter_id": "01944a2c-...",
    "block_id": "01944a2d-...",
    "block_name": "gather-credentials",
    "directive_type": "step",
    "depth": 2,
    "position": 0,
    "parent_stage_name": "Setup",
    "parent_stage_id": "01944a2c-...",
    "has_emphasis": true,
    "has_annotations": false,
    "brief_unit_keys": ["agent_context", "product_context"]
}
```

The `brief_unit_keys` are derived from the block's tags by looking up `rev.guide_tags.brief_unit_key` for each tag.

---

## 9. File Creation Checklist

### Migration
- [ ] `crates/dapple-data/migrations/00000000000016_pgvector_embeddings.sql`

### SQL Files
- [ ] `crates/dapple-data/assets/sql/guide_content_embeddings/insert_guide_content_embeddings.sql`
- [ ] `crates/dapple-data/assets/sql/guide_content_embeddings/insert_guide_embedding_tags.sql`
- [ ] `crates/dapple-data/assets/sql/guide_content_embeddings/insert_guide_embedding_locales.sql`
- [ ] `crates/dapple-data/assets/sql/guide_content_embeddings/delete_guide_content_embeddings_by_guide_id.sql`
- [ ] `crates/dapple-data/assets/sql/guide_content_embeddings/search_guide_content_embeddings.sql`
- [ ] `crates/dapple-data/assets/sql/guide_content_embeddings/search_guide_content_embeddings_by_guide_ids.sql`

### Rust Model
- [ ] `crates/dapple-data/src/model/guide_content_embeddings.rs`
- [ ] Register in `crates/dapple-data/src/model/mod.rs`

### Rust Query
- [ ] `crates/dapple-data/src/query/guide_content_embeddings.rs`
- [ ] Register in `crates/dapple-data/src/query/mod.rs`

### Dependencies
- [ ] Add `pgvector` crate to `crates/dapple-data/Cargo.toml`

### Hook Point
- [ ] Add embedding generation call to `guide_ingest.rs` (both POST and PATCH paths)
- [ ] Wire `EmbeddingService` into `AppState` or add Conduit HTTP endpoint

### Validation
- [ ] Run migration: `nu -c 'cargo sqlx migrate run'`
- [ ] Prepare offline cache: `nu -c 'cargo sqlx prepare --workspace'`
- [ ] Build: `nu -c 'cargo build --workspace'`

---

## 10. Open Items Deferred to Phase 3

1. **Canonical sync trigger** -- When canonical pins are resolved, newly-pinned guides should have embeddings regenerated. Requires tracking which guides were newly resolved in `canonical_sync_handler`.
2. **Fluent body resolution** -- For Fluent-backed blocks without inline `content`, the embedding pipeline must resolve the body from `rev.fluent_message_bodies` for the guide's authoring locale. This requires a new query: `select_fluent_body_by_message_id_and_locale.sql`.
3. **Conduit search endpoint** -- The `search_guide_content_embeddings` query is ready, but the HTTP endpoint in Conduit (or API) and the Nushell search command are Phase 3 work.
4. **Metadata depth/position computation** -- Computing `depth` and `position` from the guide hierarchy requires knowing the block's position within its chapter and the chapter's position within the stage structure. This is derivable from the ordered data but requires logic that traverses the guide structure.
5. **HNSW index evaluation** -- IVFFlat requires pre-existing data for training. Once the corpus exceeds ~10,000 embeddings, evaluate switching to HNSW for better recall without the data-dependency constraint.

---

## 11. Risk: SQLx + pgvector Compatibility

The pgvector extension adds a custom PostgreSQL type (`vector`). SQLx's compile-time verification (`query_file_as!`) may not handle this type without the `pgvector` crate's SQLx integration. If compile-time verification fails:

1. Use `sqlx::query_as()` with runtime SQL strings (acceptable exception for vector operations)
2. Ensure `SQLX_OFFLINE=false` during local development
3. Run `cargo sqlx prepare --workspace` with pgvector extension enabled

This is a known rough edge documented in the pgvector crate's README and should not block implementation.
