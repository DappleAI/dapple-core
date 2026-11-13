# Data Model

> **Audience:** Backend Developers -- Platform Engineers

This chapter documents the database schema, Rust models, SQL query patterns, and
fold logic that back the guide system. All guide tables live in the `rev` schema.

## Table Hierarchy

```
rev.object_paths
  └── rev.guide_path_details        (one-to-one)
        └── rev.versioned_objects    (one-to-many versions per path, content_hash for dedup)
              └── rev.guides         (one-to-one per versioned object)
                    ├── rev.guide_chapters      (owned dependents)
                    │     └── rev.guide_blocks   (owned dependents)
                    │           └── rev.guide_block_tags (associative)
                    ├── rev.guide_assets         (associative)
                    ├── rev.guide_cards          (owned dependent)
                    ├── rev.guide_shared_refs    (associative)
                    └── rev.guide_content_embeddings  (owned dependents, pgvector)
                          ├── rev.guide_content_embedding_tags    (associative)
                          └── rev.guide_content_embedding_locales (associative)
```

### Core Tables

| Table | Role | Key Columns |
|-------|------|-------------|
| `rev.object_paths` | Universal path tracking | `id` (UUID) |
| `rev.guide_path_details` | Path-level metadata | `path_id`, `slug`, `title`, `classification` |
| `rev.versioned_objects` | Version envelope | `path_id`, `version_major`, `version_minor`, `version_patch`, `content_hash` |
| `rev.guides` | Guide content root | `versioned_object_id`, `title`, `description`, `author`, `source_path` |
| `rev.guide_chapters` | Chapter within a guide | `guide_id`, `title`, `source_file`, `parent_chapter_id` |
| `rev.guide_blocks` | Block within a chapter | `guide_chapter_id`, `directive_type_id`, `name`, `content`, `attributes`, `fluent_message_id` |
| `rev.guide_block_tags` | Block-to-tag association | `guide_block_id`, `guide_tag_id` |
| `rev.guide_assets` | Guide-to-asset association | `guide_id`, `asset_id`, `role`, `relative_path` |
| `rev.guide_cards` | Promotional card | `guide_id`, `card_image_asset_id`, `subtitle`, `color`, `accent` |
| `rev.guide_shared_refs` | Cross-reference to shared content | `guide_id`, `ref_key`, `directive_type`, `shared_content_id` |
| `rev.guide_content_embeddings` | Block-level vector embeddings | `guide_id`, `chapter_id`, `block_id`, `chunk_text`, `embedding vector(768)`, `metadata` (JSONB) |
| `rev.guide_content_embedding_tags` | Embedding→tag association | `embedding_id`, `tag_id` (FK to `rev.guide_tags`) |
| `rev.guide_content_embedding_locales` | Embedding→locale association | `embedding_id`, `locale_id` (FK to `rev.fluent_message_locales`) |

### Lookup Tables

| Table | Purpose | Seed data |
|-------|---------|-----------|
| `rev.guide_block_directive_types` | Valid directive type keys | `prose`, `stage`, `step`, `nushell`, `brief_pack`, `inference_profile`, `ref`, `block` |
| `rev.guide_tags` | Valid tag keys for block filtering | `agent`, `product`, `coder`, `auditor`, `planner` |
| `rev.deprecation_levels` | Valid deprecation states | `active`, `deprecated`, `retired`, `quarantined` |
| `rev.shared_scopes` | Valid shared content scopes | `common`, `internal` |

## Entity Classification

Using the terminology from the dapple-data relational object model:

| Entity | Classification | Notes |
|--------|---------------|-------|
| Guide | **Versioned Principal** | Has path + versioned object |
| GuideChapter | **Owned Dependent** of Guide | Same lifecycle as parent guide |
| GuideBlock | **Owned Dependent** of GuideChapter | Same lifecycle as parent chapter |
| GuideBlockTag | **Associative Dependent** | Many-to-many: block <-> tag |
| GuideAsset | **Associative Dependent** | Many-to-many: guide <-> asset |
| GuideCard | **Owned Dependent** of Guide | One-to-one with guide |
| GuideSharedRef | **Associative Dependent** | Guide <-> shared content |

## Versioning and Deprecation

Guides are **versioned principal objects**. Each ingest creates:

1. A path (on first ingest) or reuses the existing path (on version ingest).
2. A new `versioned_objects` row with incremented version numbers.
3. A new `guides` row linked to the versioned object.

**Deprecation** is tracked via `rev.deprecation_events`, not directly on
`rev.versioned_objects`. When a guide version is deprecated, an event is inserted
into `rev.deprecation_events` referencing the versioned object. Queries derive
the current deprecation state at read time by joining against this table.
Deprecation propagates during syncing operations.

## SQL Query Patterns

### INSERT (insert_guides.sql)

The guide insert is an atomic CTE that creates the entire entity graph in one
query:

```sql
WITH
-- 1. Create object path
new_path AS (
    INSERT INTO rev.object_paths (id)
    VALUES (uuidv7())
    RETURNING id
),
-- 2. Create path details
new_path_details AS (
    INSERT INTO rev.guide_path_details (path_id, slug, title, classification)
    SELECT new_path.id, slugify($1), $2, $3
    FROM new_path
    RETURNING id
),
-- 3. Create versioned object
new_vo AS (
    INSERT INTO rev.versioned_objects (id, path_id, version_major, version_minor, version_patch, content_hash)
    SELECT uuidv7(), new_path.id, $4, $5, $6, $7
    FROM new_path
    RETURNING id
),
-- 4. Create guide
new_guide AS (
    INSERT INTO rev.guides (id, versioned_object_id, title, description, author, source_path)
    SELECT uuidv7(), new_vo.id, $2, $8, $9, $10
    FROM new_vo
    RETURNING id
),
-- 5. Create chapters (UNNEST for batch)
new_chapters AS (
    INSERT INTO rev.guide_chapters (id, guide_id, title, source_file, parent_chapter_id)
    SELECT uuidv7(), ... FROM UNNEST(...) WITH ORDINALITY
    RETURNING id
),
-- 6. Create blocks (UNNEST for batch)
new_blocks AS (
    INSERT INTO rev.guide_blocks (id, guide_chapter_id, directive_type_id, name, content, attributes, fluent_message_id)
    SELECT uuidv7(), ... FROM UNNEST(...) WITH ORDINALITY
    JOIN rev.guide_block_directive_types dt ON dt.key = ...
    RETURNING id
),
-- 7. Create block tags
new_tags AS (
    INSERT INTO rev.guide_block_tags (guide_block_id, guide_tag_id)
    SELECT ... FROM UNNEST(...)
    JOIN rev.guide_tags gt ON gt.key = ...
    RETURNING id
)
SELECT ... FROM new_guide
JOIN new_vo ON ...
JOIN new_path ON ...;
```

### SELECT (select_guides.sql)

Guide SELECT queries use LEFT JOINs across the hierarchy and return flat rows:

```sql
SELECT
    g.id,
    g.title,
    g.description,
    vo.version_major, vo.version_minor, vo.version_patch,
    vo.content_hash,
    op.id AS path_id,
    de.deprecation_level_id IS NOT NULL AS is_deprecated,
    -- Chapter fields
    gc.id AS chapter_id,
    gc.title AS chapter_title,
    gc.source_file AS chapter_source_file,
    -- Block fields
    gb.id AS block_id,
    dt.key AS block_directive_type,
    gb.name AS block_name,
    gb.content AS block_content,
    gb.attributes AS block_attributes,
    -- Tags (aggregated or joined)
    ...
FROM rev.guides g
JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
LEFT JOIN rev.deprecation_events de ON de.versioned_object_id = vo.id
LEFT JOIN rev.guide_chapters gc ON gc.guide_id = g.id
LEFT JOIN rev.guide_blocks gb ON gb.guide_chapter_id = gc.id
LEFT JOIN rev.guide_block_directive_types dt ON dt.id = gb.directive_type_id
WHERE ...
ORDER BY g.id, gc.id, gb.id;
```

### PATCH (patch_guides.sql)

The PATCH query creates a new versioned object under an existing path:

```sql
WITH
existing_path AS (
    SELECT op.id AS path_id, ...
    FROM rev.object_paths op
    WHERE op.id = $1  -- path_id from URL
),
new_vo AS (
    INSERT INTO rev.versioned_objects (id, path_id, version_major, version_minor, version_patch)
    SELECT uuidv7(), existing_path.path_id, $2, $3, $4
    FROM existing_path
    RETURNING id
),
-- ... same chapter/block CTEs as INSERT
```

## Rust Models

### Public DTO

```rust
pub struct Guide {
    pub id: Uuid,
    pub path_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub version: String,                // "major.minor.patch"
    pub deprecation_level: Option<String>,
    pub deprecation_reason: Option<String>,
    pub chapters: Vec<GuideChapter>,
    pub created_at: DateTime,
}

pub struct GuideChapter {
    pub id: Uuid,
    pub parent_chapter_id: Option<Uuid>,
    pub title: String,
    pub source_file: String,
    pub blocks: Vec<GuideBlock>,
    pub created_at: DateTime,
}

pub struct GuideBlock {
    pub id: Uuid,
    pub directive_type: String,
    pub name: Option<String>,
    pub content: String,
    pub attributes: serde_json::Value,
    pub tags: Vec<String>,
    pub created_at: DateTime,
}
```

### Internal FlatModel

The flat model is the direct output of the JOIN query. Each row represents one
block (or one guide with no blocks for empty guides).

```rust
pub(crate) struct FlatGuideModel {
    // Guide fields
    pub id: Uuid,
    pub title: String,
    // ...
    // Chapter fields (nullable for LEFT JOIN)
    pub chapter_id: Option<Uuid>,
    pub chapter_title: Option<String>,
    // Block fields (nullable for LEFT JOIN)
    pub block_id: Option<Uuid>,
    pub block_directive_type: Option<String>,
    pub block_name: Option<String>,
    pub block_content: Option<String>,
    // ...
}
```

### InsertGuideArgs

The insert arguments use the nested structure that maps to the CTE hierarchy:

```rust
pub struct InsertGuideWithChildrenArgs {
    pub title: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub version_major: Option<i32>,
    pub version_minor: Option<i32>,
    pub version_patch: Option<i32>,
    pub chapters: Vec<InsertGuideChapterWithBlocksArgs>,
    pub deprecation_level_key: Option<String>,
    pub deprecation_reason: Option<String>,
}

pub struct InsertGuideChapterWithBlocksArgs {
    pub title: String,
    pub source_file: String,
    pub parent_index: Option<usize>,
    pub blocks: Vec<InsertGuideBlockWithTagsArgs>,
}

pub struct InsertGuideBlockWithTagsArgs {
    pub directive_type_key: String,
    pub name: Option<String>,
    pub content: Option<String>,
    pub attributes: serde_json::Value,
    pub fluent_message_id: Option<Uuid>,
    pub tag_keys: Vec<String>,
}
```

## Fold Function

`fold_guides()` converts the flat JOIN rows into the nested `Guide` -> `Chapter`
-> `Block` hierarchy:

1. Group rows by guide `id`.
2. Within each guide group, group by chapter `id`.
3. Within each chapter group, collect blocks.
4. Deduplicate tags per block.
5. Assemble the nested structure.

```rust
fn fold_guides(flat: Vec<FlatGuideModel>) -> Result<Vec<Guide>, DataError> {
    // Use itertools::group_by or manual HashMap-based folding
    // to reconstruct the nested hierarchy from flat rows.
}
```

The fold preserves UUIDv7 ordering: guides, chapters, and blocks are ordered by
their `id` (which encodes creation time via UUIDv7).

## GuideCard

```rust
pub struct GuideCard {
    pub id: Uuid,
    pub subtitle: Option<String>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
    pub card_image_asset_id: Option<Uuid>,
    pub created_at: DateTime,
}
```

Under the Fluent-native model, `subtitle` and `category` are `None` on the card
itself. These values are now Fluent attributes on the brief-pack message.

## Source Files

| File | Purpose |
|------|---------|
| `crates/dapple-data/src/model/guides.rs` | Rust model types (DTOs, FlatModel, InsertArgs) |
| `crates/dapple-data/src/query/guides.rs` | Query functions (`insert_guides`, `patch_guides`, `select_guides_*`, `fold_guides`) |
| `crates/dapple-data/assets/sqlc/guides/` | SQL source files (`.sqlc` — edit these) |
| `crates/dapple-data/assets/.sql/guides/` | Generated SQL output (never edit directly) |
| `crates/dapple-data/migrations/` | DDL migrations for guide tables |

## Content-Addressable Versioning

The `rev.versioned_objects` table includes a `content_hash` column (text, 40-char SHA-1 hex) used for deduplication during ingest.

- The Guide DTO exposes `content_hash: Option<String>` on the public response
- `hash_guide_directory()` in `dapple-data::hash` computes the hash from the tarball's file contents (as a `BTreeMap<String, Vec<u8>>`)
- On ingest, if the computed hash matches the latest version's hash, the operation short-circuits and returns the existing version without creating a duplicate
- Files matching `is_excluded_path()` (`.git/`, `__MACOSX/`, `.DS_Store`, `.swp`) are filtered before hashing

## Embedding Lifecycle

Embeddings are generated **asynchronously** after successful guide ingest via `tokio::spawn`. Failure is logged as a warning and never propagates to the ingest response.

### Pipeline

1. Load guide with full chapter/block hierarchy
2. Resolve Fluent message bodies (prefer `"en"` locale)
3. Filter blocks by embeddable directive types: `prose`, `stage`, `step`, `block`, `ref`
4. Call Conduit `POST /embed` to generate 768-dim vectors
5. Delete existing embeddings for the guide (cascades to junction tables)
6. Insert new embeddings with metadata (guide slug, chapter title, block name, etc.)
7. Insert tag associations (untagged blocks receive the `"universal"` tag)
8. Insert locale associations

### Regeneration

Embeddings follow the **delete-then-reinsert** pattern — not UPDATE. Each ingest version replaces all embeddings for that guide atomically.

## Search & RAG

### Guide Search (`GET /api/guides/search`)

Natural language search over guide content. The API embeds the query text via Conduit, then queries pgvector for cosine similarity matches filtered by audience tags and locale.

- Results are returned as `GuideSearchResult` DTOs with similarity scores
- Supports scoping to specific guides via `guide_ids` parameter
- Minimum similarity threshold via `threshold` parameter

### RAG (Retrieval-Augmented Generation)

The Conduit's `RagService` injects relevant guide content into prompt context at dispatch time:

1. Embed the prompt text (or `query_hint` override)
2. Search pgvector within the dispatch's guide scope
3. Exclude blocks already retrieved in prior dispatch steps (`exclude_block_ids`)
4. Format results as `# Reference Context` with numbered chunks and similarity percentages
5. Prepend as the first system message

Both search and RAG use the same `rev.guide_content_embeddings` table and pgvector `<=>` (L2 distance) operator with `1 - distance` for similarity.

## Next Steps

- [Materialization](./materialization.md) -- How the `IngestPlan` writes to these tables
- [Render Pipeline](./render-pipeline.md) -- How guide data is rendered to PDF
- [Extending the Guide System](./extending.md) -- Adding new lookup table values and derived objects
