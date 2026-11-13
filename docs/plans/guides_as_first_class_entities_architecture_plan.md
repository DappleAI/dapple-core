# Guides as First-Class System Entities Architecture Plan

**Status:** Draft
**Created:** 2026-02-13
**Last Updated:** 2026-02-13
**Target Version:** 0.1.0
**Dependencies:** Unified Versioning Infrastructure (completed), Brief/BriefUnit Restructure (completed), Directive Entity Type (completed)

---

## 1. Executive Summary

This plan defines the architecture for elevating Guides from static mdbook files (`docs/guides/`) into first-class, versioned system entities. Guides become the primary authoring artifact from which playbooks, briefs, tasks, and contextual prompt content are derived. A single document produces multiple projections for different consumers.

Today, guides exist outside the runtime system while playbooks, briefs, and tasks exist inside it. Authors maintain both independently, leading to drift and dead documentation. By making guides ingestible into the database as structured, versioned data, and materializing their annotated content into existing runtime objects, we close the authoring gap and eliminate documentation decay.

This is a large, multi-phase effort. The guide data model, parser, and ingestion pipeline form the foundation (Phases 1-3). Content and object materialization (Phases 4-7) build on that foundation. Provenance, re-ingestion diffing, and cross-guide composability (Phases 8-11) complete the feedback loop. CI/CD and mdBook integration (Phases 12-13) are the final polish. Each phase is self-contained and independently deliverable.

### Implementation Issue Plans

The Phase 1 foundation has been decomposed into 7 detailed implementation plans in the `guides_as_first_class_entities_architecture_plan/` subdirectory:

| Issue | Plan Document | Dependencies | Description |
|-------|--------------|--------------|-------------|
| 1 | [Parser Crate](guides_as_first_class_entities_architecture_plan/guide-entities-01--parser-crate.md) | None | `crates/dapple-parser/` -- SUMMARY.md parser, fence directive tokenizer, block classifier |
| 2 | [Slugify Migration](guides_as_first_class_entities_architecture_plan/guide-entities-02--slugify-migration.md) | None | `rev.slugify()` function + Rust `slugify()` + update existing SQL files |
| 3 | [DDL Migrations](guides_as_first_class_entities_architecture_plan/guide-entities-03--ddl-migrations.md) | Issue 2 | 5 migrations: entity type, tags, directive types, path details, guide tables |
| 4 | [Data Models](guides_as_first_class_entities_architecture_plan/guide-entities-04--data-models.md) | Issue 3 | Rust DTOs, internal models, insert args, trait impls, fold logic |
| 5 | [SQL Queries](guides_as_first_class_entities_architecture_plan/guide-entities-05--sql-queries.md) | Issues 3, 4 | INSERT/SELECT/PATCH/DEPRECATE SQL files + Rust query functions |
| 6 | [API Layer](guides_as_first_class_entities_architecture_plan/guide-entities-06--api-layer.md) | Issues 4, 5 | Handlers, routes, schemas for `POST/GET/DELETE /api/guides/` |
| 7 | [Lifecycle Tests](guides_as_first_class_entities_architecture_plan/guide-entities-07--lifecycle-tests.md) | Issues 4, 5, 6 | Consumer endpoint, ScenarioContext, trials in user_lifecycle.rs |

Issues 1 and 2 can be implemented in parallel (no shared dependencies). Issues 3-7 form a sequential chain.

---

## 2. Context and Background

### Current State

- **Static guides** live in `docs/guides/` as mdbook projects (e.g., `journey-builder-guide/`, `journey-auditor-guide/`, `api-consumer-guide/`). Each has a `SUMMARY.md`, chapter `.md` files, and `book.toml` configuration.
- **Runtime objects** (playbooks, stages, steps, tasks, briefs, brief-packs, brief-groups, brief-units, inference profiles) are created exclusively via the REST API in `dapple-prompt-api`.
- **The `dapple-data` crate** provides the data access layer with unified versioning infrastructure (`rev.object_paths` -> `rev.versioned_objects` -> `rev.{entity}_path_details` -> `rev.{entities}`).
- **The `dapple` crate** is a stub CLI (`crates/dapple/src/lib.rs`) awaiting functionality.
- **Hats** (`rev.hats`) define role/responsibility profiles that are well-positioned to bridge content tags and data overlays.
- **The `guides` SQL directory** (`crates/dapple-data/assets/sql/guides/`) and **model directory** (`crates/dapple-data/src/model/guides/`) both exist but are empty.

### Design Decisions (Constraints)

These decisions were made through a structured design interview and are **non-negotiable** for this plan:

| # | Decision | Implication |
|---|----------|-------------|
| 1 | Guides and API are peer authoring paths | Both create the same runtime objects. Neither replaces the other. |
| 2 | Linking, not forking | Guide-derived objects stay connected to their source blocks. Re-ingestion creates new versions of the same objects via path mechanism. |
| 3 | Pin to version | Journeys pin to the playbook version they started with. Only new journeys pick up re-ingested versions. |
| 4 | Block API edits | Guide-derived objects are read-only via API. Edit the guide and re-ingest. |
| 5 | Soft delete on removal | When re-ingestion removes a block, derived objects are soft-deleted (deprecated/revoked). In-flight journeys unaffected. |
| 6 | Separate tags and overlays | Tags filter content blocks (guide authoring). Overlays filter data slots (task execution). Hats bridge both. |
| 7 | No tag inheritance | Every block declares its own tags. No container-level inheritance. |
| 8 | Tags are a lookup table | Referential integrity. Adding new tags requires a migration. |
| 9 | No new conduit pipeline | Guide content materializes into existing objects. Conduit is unchanged. |
| 10 | Custom fence directives | `:::directive { metadata }`->` |
| 11 | Fully annotated playbook derivation | Every stage, step, and task must be explicitly annotated. Nothing inferred. |
| 12 | Soft classification for guide types | Metadata field hinting purpose (workflow, reference). No enforcement. |
| 13 | Object references for composability | Guides reference other guides' derived objects by slug/ID. |
| 14 | Parser in dapple-data | Ingestion logic lives alongside the query layer. |
| 15 | Block-level provenance | Every derived object links back to the specific guide block that created it. |

### Relevant Existing Patterns

- **Unified Versioning:** `rev.object_paths` -> `rev.versioned_objects` -> `rev.{entity}_path_details` -> `rev.{entities}` (see `20260120000001_create_versioning_infrastructure.sql`)
- **Entity Types:** Lookup table `rev.entity_types` with constants in `model::versioned_objects::entity_type_keys`
- **Deprecation:** `rev.deprecation_levels` + `rev.deprecation_events` for soft deletes
- **Associative Dependents:** `brief_group_briefs`, `prompt_fluent_messages`, `hat_categories` -- no separate endpoints, created atomically with parent
- **Insert Pattern:** `args` CTE -> `entity_type` CTE -> `new_paths` CTE -> `numbered_paths` CTE -> `path_details` CTE -> `new_versions` CTE -> entity-specific CTE -> final SELECT (see `insert_hats.sql`)

---

## 3. Database Schema Changes (DDL)

### Phase 1: Guide Data Model

#### Migration 1a: Register Entity Types

```sql
-- Migration: 20260214000001_guide_entity_types.sql

-- Register guide-related entity types
INSERT INTO rev.entity_types (key, description) VALUES
    ('guide', 'Structured narrative documents that derive runtime objects');
```

#### Migration 1b: Guide Tags Lookup Table

```sql
-- Migration: 20260214000002_guide_tags.sql

-- Tags are an authoring-time concern for audience targeting.
-- Each tag maps to a BriefUnit type at ingestion time.
CREATE TABLE rev.guide_tags (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL,
    description TEXT,
    brief_unit_key TEXT,  -- Maps to the unit_key used when creating BriefUnits from tagged blocks
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (key)
);

-- Seed initial tags
INSERT INTO rev.guide_tags (key, description, brief_unit_key) VALUES
    ('agent', 'Content visible to AI agents during execution', 'agent_context'),
    ('product', 'Content visible to product consumers and end users', 'product_context'),
    ('coder', 'Content visible to code-generation agents', 'coder_context'),
    ('auditor', 'Content visible to auditing/compliance consumers', 'auditor_context'),
    ('planner', 'Content visible to planning/orchestration agents', 'planner_context'),
    ('universal', 'Content visible to all consumers (default for untagged blocks)', NULL);
```

#### Migration 1c: Guide Block Directive Types Lookup Table

```sql
-- Migration: 20260214000003_guide_block_directive_types.sql

-- Classifies the directive type of each block in a guide.
CREATE TABLE rev.guide_block_directive_types (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (key)
);

INSERT INTO rev.guide_block_directive_types (key, description) VALUES
    ('prose', 'Untagged or tagged prose content (:::block or plain markdown)'),
    ('stage', 'Playbook stage boundary (:::stage)'),
    ('step', 'Playbook step boundary (:::step)'),
    ('nushell', 'Nushell task script (:::nushell)'),
    ('brief_pack', 'Brief pack definition (:::brief-pack)'),
    ('inference_profile', 'Inference profile definition (:::inference-profile)'),
    ('ref', 'Reference to an existing object (:::ref)');
```

#### Migration 1d: Guide Path Details

```sql
-- Migration: 20260214000004_guide_path_details.sql

-- Path-level data for guides (immutable across versions).
-- title and slug are path-level because they identify the guide across versions.
CREATE TABLE rev.guide_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    title TEXT NOT NULL,
    slug TEXT NOT NULL,
    UNIQUE (title),
    UNIQUE (slug)
);
```

#### Migration 1e: Guide Entity Table + Chapters + Blocks

```sql
-- Migration: 20260214000005_guide_tables.sql

-- ============================================================================
-- Guide entity table (versioned, entity-specific fields only)
-- ============================================================================
CREATE TABLE rev.guides (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    versioned_object_id BIGINT NOT NULL REFERENCES rev.versioned_objects (id) UNIQUE,
    description TEXT,
    locale TEXT NOT NULL DEFAULT 'en',
    author TEXT,
    classification TEXT,  -- Soft hint: 'workflow', 'reference', 'tutorial', etc.
    source_path TEXT,     -- Filesystem path of the guide source (e.g., 'docs/guides/journey-builder-guide')
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guides_versioned_object_idx ON rev.guides (versioned_object_id);

-- ============================================================================
-- Guide Chapters (Owned Dependent of Guide)
-- Maps to a single .md source file in the guide.
-- ============================================================================
CREATE TABLE rev.guide_chapters (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    guide_id BIGINT NOT NULL REFERENCES rev.guides (id),
    parent_chapter_id BIGINT REFERENCES rev.guide_chapters (id),  -- NULL = top-level chapter
    title TEXT NOT NULL,
    source_file TEXT NOT NULL,  -- Relative path within guide (e.g., 'getting-started/intro.md')
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guide_chapters_guide_idx ON rev.guide_chapters (guide_id);
CREATE INDEX guide_chapters_parent_idx ON rev.guide_chapters (parent_chapter_id);

-- ============================================================================
-- Guide Blocks (Owned Dependent of Chapter)
-- The atomic content unit. Each block has a directive type and content.
-- ============================================================================
CREATE TABLE rev.guide_blocks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    chapter_id BIGINT NOT NULL REFERENCES rev.guide_chapters (id),
    directive_type_id SMALLINT NOT NULL REFERENCES rev.guide_block_directive_types (id),
    name TEXT,              -- Optional name from directive metadata (e.g., :::step { name: "Gather Info" })
    content TEXT NOT NULL,  -- Raw content within the fence block
    attributes JSONB NOT NULL DEFAULT '{}'::jsonb,  -- Parsed key-value attrs from directive metadata
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guide_blocks_chapter_idx ON rev.guide_blocks (chapter_id);
CREATE INDEX guide_blocks_directive_type_idx ON rev.guide_blocks (directive_type_id);

-- ============================================================================
-- Guide Block Tags (Associative: block <-> tag)
-- No separate endpoint. Created atomically with block insertion.
-- ============================================================================
CREATE TABLE rev.guide_block_tags (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    block_id BIGINT NOT NULL REFERENCES rev.guide_blocks (id),
    tag_id SMALLINT NOT NULL REFERENCES rev.guide_tags (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (block_id, tag_id)
);

CREATE INDEX guide_block_tags_block_idx ON rev.guide_block_tags (block_id);
CREATE INDEX guide_block_tags_tag_idx ON rev.guide_block_tags (tag_id);
```

### Phase 8: Provenance Tables

```sql
-- Migration: 20260214000006_guide_provenance.sql

-- ============================================================================
-- Guide Provenance (links guide blocks to derived runtime objects)
-- ============================================================================
CREATE TABLE rev.guide_block_provenances (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    block_id BIGINT NOT NULL REFERENCES rev.guide_blocks (id),
    derived_versioned_object_id BIGINT NOT NULL REFERENCES rev.versioned_objects (id),
    derived_entity_type_id SMALLINT NOT NULL REFERENCES rev.entity_types (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (block_id, derived_versioned_object_id)
);

CREATE INDEX guide_block_provenances_block_idx ON rev.guide_block_provenances (block_id);
CREATE INDEX guide_block_provenances_derived_idx ON rev.guide_block_provenances (derived_versioned_object_id);

-- ============================================================================
-- Guide-Derived Object Lock
-- Tracks which versioned objects are guide-derived and therefore read-only via API.
-- Presence in this table means the API should reject mutation requests.
-- ============================================================================
CREATE TABLE rev.guide_derived_locks (
    versioned_object_id BIGINT PRIMARY KEY REFERENCES rev.versioned_objects (id),
    guide_id BIGINT NOT NULL REFERENCES rev.guides (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guide_derived_locks_guide_idx ON rev.guide_derived_locks (guide_id);
```

### Phase 4: New BriefUnit Keys for Guide Content

```sql
-- Migration: 20260214000007_guide_brief_unit_keys.sql

-- No DDL changes needed. The brief_unit_types table was dropped in
-- 20260207000000_brief_unit_restructure.sql. BriefUnit types are now
-- determined by which subtype table row exists (markdown, notice, input, media).
--
-- Guide-derived content blocks create BriefUnit rows with unit_key values
-- like 'agent_context', 'product_context', etc. These are just TEXT values
-- in the existing unit_key column - no schema change required.
--
-- This migration is a documentation marker only.
```

---

## 4. Object Model Design

### Guide (Versioned Principal)

```rust
// Guide as a Versioned Principal Object using the unified versioning infrastructure.
// Relationship chain: object_paths -> guide_path_details -> versioned_objects -> guides

// === DTOs (Public) ===

pub struct Guide {
    pub object_id: Uuid,
    pub path_object_id: Uuid,
    pub title: String,          // From guide_path_details
    pub slug: String,           // From guide_path_details
    pub version: Version,       // From versioned_objects
    pub deprecation_level: Option<String>,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub chapters: Vec<GuideChapter>,  // Owned Dependents
    pub created_at: DateTime,
}

pub struct GuideChapter {
    pub object_id: Uuid,
    pub parent_chapter_object_id: Option<Uuid>,
    pub title: String,
    pub source_file: String,
    pub blocks: Vec<GuideBlock>,  // Owned Dependents
    pub created_at: DateTime,
}

pub struct GuideBlock {
    pub object_id: Uuid,
    pub directive_type: String,       // Key from guide_block_directive_types
    pub name: Option<String>,
    pub content: String,
    pub attributes: serde_json::Value,
    pub tags: Vec<String>,            // Keys from guide_tags
    pub provenance: Option<GuideBlockProvenance>,  // Linked derived object, if any
    pub created_at: DateTime,
}

pub struct GuideBlockProvenance {
    pub object_id: Uuid,
    pub derived_object_id: Uuid,      // The versioned_object's object_id
    pub derived_entity_type: String,  // e.g., "task", "brief", "playbook"
}
```

### Internal Models

```rust
// === Internal Models ===

pub(crate) struct GuideModel {
    pub id: i64,
    pub object_id: Uuid,
    pub versioned_object_id: i64,
    pub path_id: i64,
    pub path_object_id: Uuid,
    pub title: String,
    pub slug: String,
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patch: i32,
    pub deprecation_level_key: Option<String>,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub created_at: OffsetDateTime,
    pub chapters: Vec<GuideChapterModel>,
}

pub(crate) struct InsertedGuideModel {
    pub id: i64,
    pub object_id: Uuid,
    pub versioned_object_id: i64,
    pub path_id: i64,
    pub path_object_id: Uuid,
    pub title: String,
    pub slug: String,
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patch: i32,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub created_at: OffsetDateTime,
}

pub(crate) struct FlatGuideModel {
    // Guide fields
    pub id: i64,
    pub object_id: Uuid,
    pub versioned_object_id: i64,
    pub path_id: i64,
    pub path_object_id: Uuid,
    pub title: String,
    pub slug: String,
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patch: i32,
    pub deprecation_level_key: Option<String>,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub created_at: OffsetDateTime,
    // Chapter fields (prefixed)
    pub chapter_id: Option<i64>,
    pub chapter_object_id: Option<Uuid>,
    pub chapter_parent_chapter_id: Option<i64>,
    pub chapter_parent_chapter_object_id: Option<Uuid>,
    pub chapter_title: Option<String>,
    pub chapter_source_file: Option<String>,
    pub chapter_created_at: Option<OffsetDateTime>,
    // Block fields (prefixed)
    pub block_id: Option<i64>,
    pub block_object_id: Option<Uuid>,
    pub block_directive_type_key: Option<String>,
    pub block_name: Option<String>,
    pub block_content: Option<String>,
    pub block_attributes: Option<serde_json::Value>,
    pub block_tag_keys: Option<Vec<String>>,
    pub block_created_at: Option<OffsetDateTime>,
    // Provenance fields (prefixed)
    pub provenance_object_id: Option<Uuid>,
    pub provenance_derived_object_id: Option<Uuid>,
    pub provenance_derived_entity_type_key: Option<String>,
}
```

### Insert Args

```rust
pub(crate) struct InsertGuideArgs {
    pub titles: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    pub locales: Vec<String>,
    pub authors: Vec<Option<String>>,
    pub classifications: Vec<Option<String>>,
    pub source_paths: Vec<Option<String>>,
}

pub(crate) struct InsertGuideChapterArgs {
    pub guide_ids: Vec<i64>,
    pub parent_chapter_ids: Vec<Option<i64>>,
    pub titles: Vec<String>,
    pub source_files: Vec<String>,
}

pub(crate) struct InsertGuideBlockArgs {
    pub chapter_ids: Vec<i64>,
    pub directive_type_ids: Vec<i16>,
    pub names: Vec<Option<String>>,
    pub contents: Vec<String>,
    pub attributes: Vec<serde_json::Value>,
    pub tag_ids_per_block: Vec<Vec<i16>>,  // Each block has zero or more tags
}
```

### Object Classification

| Object | Classification | Versioned? | Parent |
|--------|---------------|-----------|--------|
| Guide | Versioned Principal | Yes (via unified versioning) | -- |
| GuideChapter | Owned Dependent | No (owned by Guide) | Guide |
| GuideBlock | Owned Dependent | No (owned by Chapter) | GuideChapter |
| GuideBlockTag | Associative Dependent | No | GuideBlock <-> GuideTag |
| GuideBlockProvenance | Owned Dependent | No | GuideBlock |
| GuideDerivedLock | Owned Dependent | No | Guide (references versioned_objects) |
| GuideTag | Lookup Table (seeded) | No | -- |
| GuideBlockDirectiveType | Lookup Table (seeded) | No | -- |

---

## 5. API Contract

### Guide CRUD Endpoints

Guides are system config entities with slugs and are versioned with path history.

```
POST   /api/guides       -- Create guide(s) with chapters and blocks
GET    /api/guides        -- Query guides (index)
GET    /api/guides/:id    -- Get single guide by object_id
PATCH  /api/guides        -- Create new version of existing guide (via path_object_id)
DELETE /api/guides        -- Deprecate guides (soft delete)
```

#### POST /api/guides

```rust
#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateGuideSetSchema {
    pub objs: Vec<CreateGuideSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateGuideSchema {
    pub title: String,
    pub description: Option<String>,
    pub locale: Option<String>,        // Defaults to "en"
    pub author: Option<String>,
    pub classification: Option<String>, // "workflow", "reference", "tutorial", etc.
    pub source_path: Option<String>,
    pub chapters: Vec<CreateGuideChapterSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateGuideChapterSchema {
    pub title: String,
    pub source_file: String,
    pub parent_index: Option<usize>,  // Index into the chapters array for nesting
    pub blocks: Vec<CreateGuideBlockSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateGuideBlockSchema {
    pub directive_type: String,        // Key from guide_block_directive_types
    pub name: Option<String>,
    pub content: String,
    pub attributes: Option<serde_json::Value>,
    pub tags: Vec<String>,            // Keys from guide_tags
}
```

#### GET /api/guides

```rust
#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetGuideSetSchema {
    ByObjectIds {
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
    BySlugs {
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
}
```

#### PATCH /api/guides

```rust
#[derive(JsonSchema, Serialize, Deserialize)]
pub struct PatchGuideSetSchema {
    pub objs: Vec<PatchGuideSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize)]
pub struct PatchGuideSchema {
    pub path_object_id: Uuid,           // Which guide path to create a new version for
    pub description: Option<String>,
    pub locale: Option<String>,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub chapters: Vec<CreateGuideChapterSchema>,  // Full chapter tree for new version
}
```

### Guide Import CLI Endpoint (Phase 3)

Not an HTTP endpoint. This is a CLI command that parses guide sources and calls the data layer directly.

```rust
// In crates/dapple/src/lib.rs (or a new subcommand module)
pub async fn import_guide(
    pool: &PgPool,
    guide_path: &Path,    // e.g., docs/guides/journey-builder-guide/
    options: ImportOptions,
) -> Result<GuideImportResult, ImportError>;

pub struct ImportOptions {
    pub dry_run: bool,
    pub force: bool,      // Re-ingest even if content unchanged
}

pub struct GuideImportResult {
    pub guide_object_id: Uuid,
    pub version: Version,
    pub chapters_created: usize,
    pub blocks_created: usize,
    pub objects_materialized: Vec<MaterializedObject>,
    pub objects_deprecated: Vec<DeprecatedObject>,
}
```

---

## 6. SQL Query Strategy

### Insert Guides (Phase 1)

```sql
-- File: assets/sql/guides/insert_guides.sql
-- Creates: object_path -> guide_path_details -> versioned_object -> guide
-- Follows the same CTE chain as insert_hats.sql

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],   -- title[]
        $2::TEXT[],   -- description[]
        $3::TEXT[],   -- locale[]
        $4::TEXT[],   -- author[]
        $5::TEXT[],   -- classification[]
        $6::TEXT[]    -- source_path[]
    ) WITH ORDINALITY AS o(
        title,
        description,
        locale,
        author,
        classification,
        source_path,
        ord
    )
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'guide'
),
new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT (SELECT id FROM entity_type)
    FROM args
    ORDER BY args.ord
    RETURNING id, object_id
),
numbered_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),
path_details AS (
    INSERT INTO rev.guide_path_details (path_id, title, slug)
    SELECT
        np.id,
        args.title,
        lower(regexp_replace(args.title, '\s+', '-', 'g'))
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, title, slug
),
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, version_major, version_minor, version_patch)
    SELECT np.id, 0, 0, 1
    FROM new_paths np
    ORDER BY np.id
    RETURNING id, object_id, path_id, version_major, version_minor, version_patch, created_at
),
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
),
new_guides AS (
    INSERT INTO rev.guides (versioned_object_id, description, locale, author, classification, source_path)
    SELECT
        nuv.id,
        args.description,
        COALESCE(args.locale, 'en'),
        args.author,
        args.classification,
        args.source_path
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, object_id, versioned_object_id, description, locale, author, classification, source_path, created_at
),
numbered_guides AS (
    SELECT ng.*, row_number() OVER (ORDER BY ng.id) AS ord
    FROM new_guides ng
)
SELECT
    nug.id,
    nug.object_id,
    nug.versioned_object_id,
    nuv.path_id,
    nup.object_id AS path_object_id,
    npd.title,
    npd.slug,
    nuv.version_major,
    nuv.version_minor,
    nuv.version_patch,
    nug.description,
    nug.locale,
    nug.author,
    nug.classification,
    nug.source_path,
    nug.created_at
FROM numbered_guides nug
JOIN numbered_versions nuv ON nuv.ord = nug.ord
JOIN numbered_paths nup ON nup.ord = nug.ord
JOIN numbered_path_details npd ON npd.ord = nug.ord
ORDER BY nug.id;
```

### Insert Guide Chapters (Phase 1)

```sql
-- File: assets/sql/guides/insert_guide_chapters.sql
-- Chapters are Owned Dependents of Guide. Inserted per-guide after guide creation.

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::BIGINT[],  -- guide_id[]
        $2::BIGINT[],  -- parent_chapter_id[] (NULL for top-level)
        $3::TEXT[],     -- title[]
        $4::TEXT[]      -- source_file[]
    ) WITH ORDINALITY AS o(
        guide_id,
        parent_chapter_id,
        title,
        source_file,
        ord
    )
),
new_chapters AS (
    INSERT INTO rev.guide_chapters (guide_id, parent_chapter_id, title, source_file)
    SELECT
        args.guide_id,
        args.parent_chapter_id,
        args.title,
        args.source_file
    FROM args
    ORDER BY args.ord
    RETURNING id, object_id, guide_id, parent_chapter_id, title, source_file, created_at
)
SELECT * FROM new_chapters ORDER BY id;
```

### Insert Guide Blocks with Tags (Phase 1)

```sql
-- File: assets/sql/guides/insert_guide_blocks.sql
-- Blocks are Owned Dependents of Chapter. Tags are Associative Dependents of Block.
-- This query inserts blocks only. Tags are inserted in a separate query
-- to avoid 2D array complexity.

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::BIGINT[],    -- chapter_id[]
        $2::SMALLINT[],  -- directive_type_id[]
        $3::TEXT[],       -- name[]
        $4::TEXT[],       -- content[]
        $5::JSONB[]       -- attributes[]
    ) WITH ORDINALITY AS o(
        chapter_id,
        directive_type_id,
        name,
        content,
        attributes,
        ord
    )
),
new_blocks AS (
    INSERT INTO rev.guide_blocks (chapter_id, directive_type_id, name, content, attributes)
    SELECT
        args.chapter_id,
        args.directive_type_id,
        args.name,
        args.content,
        args.attributes
    FROM args
    ORDER BY args.ord
    RETURNING id, object_id, chapter_id, directive_type_id, name, content, attributes, created_at
)
SELECT
    nb.id,
    nb.object_id,
    nb.chapter_id,
    nb.directive_type_id,
    dt.key as directive_type_key,
    nb.name,
    nb.content,
    nb.attributes,
    nb.created_at
FROM new_blocks nb
JOIN rev.guide_block_directive_types dt ON dt.id = nb.directive_type_id
ORDER BY nb.id;
```

### Insert Guide Block Tags (Phase 1)

```sql
-- File: assets/sql/guides/insert_guide_block_tags.sql

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::BIGINT[],    -- block_id[]
        $2::SMALLINT[]   -- tag_id[]
    ) WITH ORDINALITY AS o(
        block_id,
        tag_id,
        ord
    )
)
INSERT INTO rev.guide_block_tags (block_id, tag_id)
SELECT args.block_id, args.tag_id
FROM args
ORDER BY args.ord
RETURNING id, object_id, block_id, tag_id, created_at;
```

### Select Guides by Object IDs (Phase 1)

```sql
-- File: assets/sql/guides/select_guides_by_object_ids.sql

WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(object_id, ord)
)
SELECT
    g.id,
    g.object_id,
    g.versioned_object_id,
    vo.path_id,
    op.object_id AS path_object_id,
    gpd.title,
    gpd.slug,
    vo.version_major,
    vo.version_minor,
    vo.version_patch,
    dl.key AS deprecation_level_key,
    g.description,
    g.locale,
    g.author,
    g.classification,
    g.source_path,
    g.created_at,
    -- Chapter fields
    gc.id AS chapter_id,
    gc.object_id AS chapter_object_id,
    gc.parent_chapter_id AS chapter_parent_chapter_id,
    pc.object_id AS chapter_parent_chapter_object_id,
    gc.title AS chapter_title,
    gc.source_file AS chapter_source_file,
    gc.created_at AS chapter_created_at,
    -- Block fields
    gb.id AS block_id,
    gb.object_id AS block_object_id,
    bdt.key AS block_directive_type_key,
    gb.name AS block_name,
    gb.content AS block_content,
    gb.attributes AS block_attributes,
    ARRAY_AGG(DISTINCT gt.key) FILTER (WHERE gt.key IS NOT NULL) AS block_tag_keys,
    gb.created_at AS block_created_at,
    -- Provenance fields
    gbp.object_id AS provenance_object_id,
    vo_derived.object_id AS provenance_derived_object_id,
    et_derived.key AS provenance_derived_entity_type_key
FROM args
JOIN rev.guides g ON g.object_id = args.object_id
JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = vo.deprecation_level_id
LEFT JOIN rev.guide_chapters gc ON gc.guide_id = g.id
LEFT JOIN rev.guide_chapters pc ON pc.id = gc.parent_chapter_id
LEFT JOIN rev.guide_blocks gb ON gb.chapter_id = gc.id
LEFT JOIN rev.guide_block_directive_types bdt ON bdt.id = gb.directive_type_id
LEFT JOIN rev.guide_block_tags gbt ON gbt.block_id = gb.id
LEFT JOIN rev.guide_tags gt ON gt.id = gbt.tag_id
LEFT JOIN rev.guide_block_provenances gbp ON gbp.block_id = gb.id
LEFT JOIN rev.versioned_objects vo_derived ON vo_derived.id = gbp.derived_versioned_object_id
LEFT JOIN rev.entity_types et_derived ON et_derived.id = gbp.derived_entity_type_id
GROUP BY
    g.id, g.object_id, g.versioned_object_id, vo.path_id, op.object_id,
    gpd.title, gpd.slug, vo.version_major, vo.version_minor, vo.version_patch,
    dl.key, g.description, g.locale, g.author, g.classification, g.source_path,
    g.created_at,
    gc.id, gc.object_id, gc.parent_chapter_id, pc.object_id, gc.title,
    gc.source_file, gc.created_at,
    gb.id, gb.object_id, bdt.key, gb.name, gb.content, gb.attributes,
    gb.created_at,
    gbp.object_id, vo_derived.object_id, et_derived.key
ORDER BY g.object_id, gc.object_id, gb.object_id;
```

### Select Guides (All)

```sql
-- File: assets/sql/guides/select_guides.sql
-- Same as by_object_ids but without the args CTE filter.
-- Uses FilterOptions for LIMIT/OFFSET.
```

### Select Guides by Slugs

```sql
-- File: assets/sql/guides/select_guides_by_slugs.sql
-- Same structure, filtered by gpd.slug IN (UNNEST($1::TEXT[])).
```

### Select Guides by Path IDs

```sql
-- File: assets/sql/guides/select_guides_by_path_ids.sql
-- Same structure, filtered by op.object_id IN (UNNEST($1::UUID[])).
```

---

## 7. Implementation Guidance

### Phase 1: Guide Data Model

**Goal:** Guides as versioned principal objects with chapters, blocks, and tags. Full CRUD via API.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/migrations/20260214000001_guide_entity_types.sql` | Create | Entity type registration |
| `crates/dapple-data/migrations/20260214000002_guide_tags.sql` | Create | Tags lookup table + seed data |
| `crates/dapple-data/migrations/20260214000003_guide_block_directive_types.sql` | Create | Directive types lookup table + seed data |
| `crates/dapple-data/migrations/20260214000004_guide_path_details.sql` | Create | Path-level details table |
| `crates/dapple-data/migrations/20260214000005_guide_tables.sql` | Create | Guide, chapter, block, block_tag tables |
| `crates/dapple-data/src/model/guides.rs` | Create | All model structs (Guide, Chapter, Block DTOs + internal models) |
| `crates/dapple-data/src/model/guides/guide_chapters.rs` | Create | Chapter models (InsertedGuideChapterModel, etc.) |
| `crates/dapple-data/src/model/guides/guide_blocks.rs` | Create | Block models |
| `crates/dapple-data/src/model/guides/guide_block_tags.rs` | Create | Block tag models |
| `crates/dapple-data/src/model/mod.rs` | Modify | Add `pub mod guides;` |
| `crates/dapple-data/src/model/versioned_objects.rs` | Modify | Add `pub const GUIDE: &str = "guide";` to `entity_type_keys` |
| `crates/dapple-data/src/query/guides.rs` | Create | Insert, select, patch, deprecate functions |
| `crates/dapple-data/src/query/mod.rs` | Modify | Add `pub mod guides;` |
| `crates/dapple-data/assets/sql/guides/insert_guides.sql` | Create | Guide insert CTE |
| `crates/dapple-data/assets/sql/guides/insert_guide_chapters.sql` | Create | Chapter insert |
| `crates/dapple-data/assets/sql/guides/insert_guide_blocks.sql` | Create | Block insert |
| `crates/dapple-data/assets/sql/guides/insert_guide_block_tags.sql` | Create | Block tag insert |
| `crates/dapple-data/assets/sql/guides/select_guides_by_object_ids.sql` | Create | Full guide query with joins |
| `crates/dapple-data/assets/sql/guides/select_guides.sql` | Create | All guides |
| `crates/dapple-data/assets/sql/guides/select_guides_by_slugs.sql` | Create | By slug lookup |
| `crates/dapple-data/assets/sql/guides/select_guides_by_path_ids.sql` | Create | By path lookup |
| `crates/dapple-data/assets/sql/guides/deprecate_guides.sql` | Create | Soft delete |
| `crates/dapple-prompt-api/src/schema/guides.rs` | Create | Create/Get/Patch schemas |
| `crates/dapple-prompt-api/src/handler/guides.rs` | Create | POST/GET/PATCH/DELETE handlers |
| `crates/dapple-prompt-api/src/route/guides.rs` | Create | Route registration |
| `crates/dapple-prompt-api/src/handler/mod.rs` | Modify | Import guides module |
| `crates/dapple-prompt-api/src/route/mod.rs` | Modify | Import guides routes |
| `crates/dapple-prompt-api/src/schema/mod.rs` | Modify | Import guides schema |

**Key Considerations:**
- Guides are deep objects (guide -> chapters -> blocks -> tags). The POST handler must insert guide, then chapters (in tree order), then blocks, then block tags. This requires multiple insert calls within a single transaction (not a single CTE, since the hierarchy is three levels deep).
- Use `sqlx::Transaction` to wrap the multi-insert sequence: insert guide -> insert chapters (get IDs back) -> insert blocks (with chapter IDs) -> insert block tags (with block IDs).
- Chapter ordering is captured by UUIDv7 via `ORDER BY args.ord` in the insert. Chapter nesting uses `parent_chapter_id`.
- Block ordering within a chapter is captured by UUIDv7 ordering.
- The FlatGuideModel requires a GROUP BY with ARRAY_AGG for block tags. This is necessary because a single block can have multiple tags, producing multiple rows.

**Trait Implementations Required:**
- `InsertedModel`, `Keyed`, `RelationalObject`, `PrincipalObject`, `VersionedRelationalObject`, `UnifiedVersionedObject`, `Depreciable`, `FlatRelationalObject`, `DataTransferObject` on guide models (following the Hat/Directive pattern exactly).

### Phase 2: Custom Fence Parser

**Goal:** Parse `:::directive { metadata }` syntax from markdown files into structured data.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/parser/mod.rs` | Create | Parser module root |
| `crates/dapple-data/src/parser/fence.rs` | Create | `:::` fence directive tokenizer |
| `crates/dapple-data/src/parser/summary.rs` | Create | SUMMARY.md parser for chapter tree |
| `crates/dapple-data/src/parser/block.rs` | Create | Block classifier (directive type + metadata extractor) |
| `crates/dapple-data/src/lib.rs` | Modify | Add `pub mod parser;` |

**Parser Output Types:**

```rust
// In crates/dapple-data/src/parser/mod.rs

pub struct ParsedGuide {
    pub title: String,
    pub chapters: Vec<ParsedChapter>,
}

pub struct ParsedChapter {
    pub title: String,
    pub source_file: String,
    pub depth: usize,          // Nesting level (0 = top-level)
    pub parent_index: Option<usize>,  // Index into flat chapter list
    pub blocks: Vec<ParsedBlock>,
}

pub struct ParsedBlock {
    pub directive_type: String,        // "prose", "stage", "step", "nushell", etc.
    pub name: Option<String>,
    pub content: String,
    pub attributes: serde_json::Value,
    pub tags: Vec<String>,
}
```

**Key Considerations:**
- The fence parser must handle nesting (a `:::stage` contains `:::step` which contains `:::nushell`).
- `:::` (empty close) ends the current fence. The parser maintains a stack of open fences.
- Attributes use YAML-like key-value syntax inside `{ ... }` after the directive name.
- Plain markdown between fences becomes `prose` blocks.
- The parser lives in `dapple-data` (per design decision #14).
- No external parsing crate dependency -- this is simple enough to implement with a line-by-line state machine.

**Parsing Algorithm (High Level):**

1. Parse SUMMARY.md to get chapter tree (title, file path, depth, parent).
2. For each chapter file:
   a. Split into blocks by scanning for `:::directive` lines.
   b. Track fence nesting with a stack.
   c. For each block: extract directive type, attributes, tags, and content.
   d. Plain markdown between fences becomes `prose` blocks.

### Phase 3: Ingestion Pipeline

**Goal:** CLI command that reads guide source files and writes structured data to the database.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/ingest/mod.rs` | Create | Ingestion pipeline module |
| `crates/dapple-data/src/ingest/guide.rs` | Create | Guide ingestion orchestrator |
| `crates/dapple-data/src/lib.rs` | Modify | Add `pub mod ingest;` |
| `crates/dapple/src/lib.rs` | Modify | Add guide import CLI command |
| `crates/dapple/Cargo.toml` | Modify | Add clap, dapple-data, sqlx dependencies |

**Pipeline Steps:**

```rust
pub async fn ingest_guide(
    pool: &PgPool,
    guide_path: &Path,
    options: &ImportOptions,
) -> Result<GuideImportResult, ImportError> {
    // 1. Parse SUMMARY.md + chapter files -> ParsedGuide
    let parsed = parser::parse_guide(guide_path)?;

    // 2. Check if guide already exists (by slug derived from title)
    let slug = slugify(&parsed.title);
    let existing = query::guides::select_guides_by_slugs(pool, vec![slug.clone()]).await?;

    // 3. If exists: this is a re-ingestion (creates new version)
    //    If not: this is a fresh ingestion (creates path + v0.0.1)
    let mut tx = pool.begin().await?;

    let guide = if let Some(existing) = existing.first() {
        // Re-ingestion: PATCH to create new version
        ingest::patch_guide(&mut tx, existing, &parsed, options).await?
    } else {
        // Fresh: INSERT new guide
        ingest::create_guide(&mut tx, &parsed).await?
    };

    // 4. Materialize derived objects (Phases 4-7)
    // let materialized = materialize::materialize_guide(&mut tx, &guide).await?;

    tx.commit().await?;

    Ok(GuideImportResult { guide_object_id: guide.object_id, /* ... */ })
}
```

**Key Considerations:**
- The ingestion pipeline is a single transaction. If any step fails, everything rolls back.
- The pipeline uses `sqlx::Transaction` for all writes.
- Lookup table IDs (directive_type_id, tag_id) should be loaded once at pipeline start and cached in a `LookupCache` struct.
- Phase 3 creates the guide data model only. Materialization (deriving runtime objects) is Phase 4-7.

### Phase 4: Content Materialization (Tagged Blocks -> BriefUnits)

**Goal:** Tagged prose blocks become BriefUnits with audience-specific `unit_key` values.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/ingest/materialize.rs` | Create | Materialization orchestrator |
| `crates/dapple-data/src/ingest/materialize/content.rs` | Create | Block -> BriefUnit materialization |

**Logic:**

```rust
/// For each block with directive_type = 'prose' and one or more tags:
/// 1. Look up the tag's brief_unit_key from guide_tags
/// 2. Create a Brief (versioned) if needed, or find the existing one
/// 3. Create a BriefUnit with unit_key = tag's brief_unit_key
/// 4. The BriefUnit contains a markdown subtype with the block's content
/// 5. Record provenance: block -> derived brief/brief_unit
///
/// Untagged prose blocks: create BriefUnit with unit_key = NULL (universal context)
```

**Key Considerations:**
- Each unique (guide_path, tag) combination may produce a separate Brief for that audience.
- BriefUnits are Owned Dependents of Briefs. The Brief is the versioned principal.
- The `unit_key` value (e.g., `agent_context`, `product_context`) comes from the `guide_tags.brief_unit_key` column.
- BriefUnit content uses the `markdown` subtype (inserted into `rev.brief_unit_markdowns`).

### Phase 5: Task Materialization (:::nushell, :::inference-profile -> Tasks)

**Goal:** Directive blocks that define tasks become Task objects in the database.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/ingest/materialize/tasks.rs` | Create | Block -> Task materialization |

**Logic:**

For `:::nushell` blocks:
- Extract the nushell script content from the block.
- Create a Task (versioned) with `function = 'nu_pipeline'` and the script in `function_input`.
- Task name comes from the block's `name` attribute.
- Record provenance.

For `:::inference-profile` blocks:
- Parse the YAML-like content to extract model, temperature, prompt slug.
- Create an InferenceProfile (versioned) with the configuration.
- Create an associated Task if needed (the task that "runs" the inference).
- Record provenance.

### Phase 6: Brief Materialization (:::brief-pack -> BriefPacks)

**Goal:** `:::brief-pack` blocks define complete Brief/BriefGroup/BriefPack structures.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/ingest/materialize/briefs.rs` | Create | Block -> BriefPack tree materialization |

**Logic:**

The `:::brief-pack` block contains structured YAML defining groups and briefs:

```yaml
groups:
  - name: "Business Basics"
    briefs:
      - slug: business-name
        unit_key: input
        control: text
```

For each `:::brief-pack` block:
1. Parse the structured content into group/brief definitions.
2. For each brief: create Brief (versioned) + BriefUnits (based on the brief definition).
3. For each group: create BriefGroup (versioned) + `brief_group_briefs` associations.
4. Create BriefPack (versioned) + `brief_pack_groups` associations.
5. Record provenance from block to the BriefPack.

### Phase 7: Playbook Derivation (:::stage, :::step -> Playbook Structure)

**Goal:** Structural directives derive a Playbook with stages, steps, and wired tasks.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/ingest/materialize/playbooks.rs` | Create | Guide structure -> Playbook derivation |

**Logic:**

1. Scan chapters for `:::stage` blocks. Each stage maps to a Stage (versioned).
2. Within each stage, scan for `:::step` blocks. Each step maps to a Step (versioned).
3. Within each step, find task-producing blocks (`:::nushell`, `:::inference-profile`, `:::brief-pack`).
4. Wire tasks into steps via `playbook_tasks` associative table.
5. Wire steps into stages via `playbook_steps` associative table.
6. Wire stages into the playbook via `playbook_stages` associative table.
7. Create the Playbook (versioned) with the full hierarchy.
8. Record provenance from each structural block to its derived object.

**Key Considerations:**
- A guide with no `:::stage` blocks derives no playbook (this is fine -- it is a reference guide).
- Tasks materialized in Phases 5-6 must be created before the playbook wiring in Phase 7. The ingestion pipeline respects this ordering.
- The playbook's `source_path` metadata should reference the guide's slug for traceability.

### Phase 8: Provenance + API Blocking

**Goal:** Block-level provenance tracking. Guide-derived objects are read-only via API.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/migrations/20260214000006_guide_provenance.sql` | Create | Provenance and lock tables |
| `crates/dapple-data/src/query/guides/provenance.rs` | Create | Provenance insert/query functions |
| `crates/dapple-data/src/query/guides/locks.rs` | Create | Lock check functions |
| `crates/dapple-prompt-api/src/handler/*.rs` | Modify | Add lock checks to POST/PATCH/DELETE handlers for all entity types |

**API Blocking Logic:**

```rust
// In every POST/PATCH/DELETE handler for entities that can be guide-derived:
async fn check_not_guide_locked(pool: &PgPool, versioned_object_ids: &[i64]) -> Result<(), ApiError> {
    let locked = query::guides::select_locked_versioned_objects(pool, versioned_object_ids).await?;
    if !locked.is_empty() {
        return Err(ApiError::Conflict(format!(
            "Objects {} are guide-derived and cannot be modified via API. Edit the source guide and re-ingest.",
            locked.iter().map(|l| l.object_id.to_string()).collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(())
}
```

**Affected Handlers (add lock check to PATCH/DELETE):**
- `tasks`, `steps`, `stages`, `playbooks`, `prompts`, `fluent_messages`, `briefs`, `brief_groups`, `brief_packs`, `inference_profiles`

### Phase 9: Re-Ingestion Diffing

**Goal:** When a guide is re-ingested, compare new content against existing to create new versions and soft-delete removed objects.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/ingest/diff.rs` | Create | Diff engine for guide re-ingestion |

**Algorithm:**

1. Load existing guide version with all blocks and provenance.
2. Parse new guide source.
3. For each block in new version:
   a. Match to existing block by (directive_type, name, source_file position).
   b. If content changed: mark for re-materialization.
   c. If new: mark for creation.
4. For each block in old version not matched:
   a. Soft-delete derived objects (set deprecation_level to 'deprecated' or 'retired').
   b. Record deprecation event with reason "Removed during guide re-ingestion".
5. Create new guide version (PATCH).
6. Materialize new/changed blocks.
7. Update provenance records.

**Key Considerations:**
- Block matching uses a composite key: (directive_type_key, name, chapter source_file, position within chapter). Blocks without explicit names are matched by position.
- Content comparison uses SHA-256 hashing for efficient change detection.
- The diff runs within the same transaction as the re-ingestion write.

### Phase 10: Foundation Guide

**Goal:** Migrate conceptual `_foundation/` content to a reference guide in the database.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `docs/guides/_foundation/` | Modify | Add `:::block` directives to conceptual content |

**Work:**

1. Add `:::block` directives to foundation files that should be ingested (glossary, core execution model, briefs-and-packs concepts, versioning model).
2. Leave build-time files unchanged (LaTeX, CSS, Lua).
3. Run `dapple guide import docs/guides/_foundation/` to ingest.
4. The foundation guide will have `classification: "reference"` and no `:::stage` directives, so no playbook is derived.
5. Its content becomes BriefUnits available as context to all consumers.

### Phase 11: Object References (:::ref)

**Goal:** Guides can reference derived objects from other guides by type and slug.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/ingest/materialize/refs.rs` | Create | Reference resolution |

**Logic:**

```markdown
:::ref { type: playbook, slug: "onboarding" }
```

During materialization:
1. Parse the ref block's attributes (type + slug).
2. Look up the referenced object by entity type and slug (via path_details).
3. Validate the object exists and is not deprecated.
4. For playbook squashing (Issue #177): if a ref to a playbook appears inside a `:::stage`, inline that playbook's stages into the current playbook.
5. Record the reference in provenance (block -> referenced object).

### Phase 12: CI/CD Integration

**Goal:** Production ingestion runs automatically on merge to main.

**Work:**

This is a CI/CD pipeline configuration task, not a code change. The pipeline:

1. On merge to `main`: detect changes in `docs/guides/**/*.md` files.
2. For each changed guide: run `dapple guide import <path>`.
3. Report success/failure with materialized object counts.
4. The `dapple` CLI binary must be available in the CI/CD image.

**Key Considerations:**
- The CLI connects to the production database using `DATABASE_URL` from CI secrets.
- Dry-run mode (`--dry-run`) should be used in PR checks to validate parsing without writing.
- The pipeline should run `dapple guide import --all` to detect removals (guides that no longer exist in the repo).

### Phase 13: mdBook Preprocessor

**Goal:** Custom mdBook preprocessor that renders `:::` directives as styled HTML.

**New/Modified Files:**

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-mdbook-preprocessor/` | Create | New crate for mdBook preprocessor |
| `crates/dapple-mdbook-preprocessor/src/main.rs` | Create | Preprocessor binary |
| `crates/dapple-mdbook-preprocessor/src/lib.rs` | Create | Transformation logic |

**Key Considerations:**
- This is a standalone binary crate that mdBook invokes during build.
- It transforms `:::directive { ... }` blocks into HTML `<div>` elements with appropriate CSS classes.
- It reuses the fence parser from `dapple-data` (either as a library dependency or by extracting the parser into its own crate).
- `:::stage`, `:::step` become collapsible sections with styled headers.
- `:::nushell` blocks get syntax-highlighted code rendering.
- `:::block { tags: [...] }` blocks get subtle tag indicators.
- The preprocessor is configured in `book.toml`:

```toml
[preprocessor.dapple-directives]
command = "dapple-mdbook-preprocessor"
```

---

## 8. Testing Strategy

### Integration Test Trials (Phase 1)

```rust
// In crates/dapple-prompt-api/tests/consumer/guides.rs

pub struct GuidesEndpoint {
    base_url: String,
    context: Arc<Mutex<ScenarioContext>>,
}

impl Endpoint for GuidesEndpoint {
    type Object = dapple_data::model::guides::Guide;
    type CreateSchema = CreateGuideSchema;
    type CreateSetSchema = CreateGuideSetSchema;

    fn path(&self) -> &str { "guides" }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        CreateGuideSchema {
            title: Self::fake_name(),
            description: Some(Self::fake_description()),
            locale: Some("en".to_string()),
            author: Some("Test Author".to_string()),
            classification: Some("reference".to_string()),
            source_path: None,
            chapters: vec![
                CreateGuideChapterSchema {
                    title: "Introduction".to_string(),
                    source_file: "intro.md".to_string(),
                    parent_index: None,
                    blocks: vec![
                        CreateGuideBlockSchema {
                            directive_type: "prose".to_string(),
                            name: None,
                            content: "This is introductory content.".to_string(),
                            attributes: None,
                            tags: vec!["agent".to_string(), "product".to_string()],
                        },
                    ],
                },
                CreateGuideChapterSchema {
                    title: "Details".to_string(),
                    source_file: "details.md".to_string(),
                    parent_index: None,
                    blocks: vec![
                        CreateGuideBlockSchema {
                            directive_type: "nushell".to_string(),
                            name: Some("process-data".to_string()),
                            content: "def main [] { { payload: {}, signals: {} } }".to_string(),
                            attributes: None,
                            tags: vec!["coder".to_string()],
                        },
                    ],
                },
            ],
        }
    }
}
```

### ScenarioContext Updates

```rust
// In tests/consumer/mod.rs - add to ScenarioContext:
pub guide_batches: Vec<Vec<dapple_data::model::guides::Guide>>,
pub guide_named_batches: HashMap<String, BatchHandle<dapple_data::model::guides::Guide>>,
```

### Trial Definitions

```rust
// In tests/user_lifecycle.rs:

fn trial_create_guides(/* ... */) -> Trial {
    // No dependencies - guides are top-level system config
    // Create: 2 guides with chapters and blocks
    // Register: "init-guides" batch in ScenarioContext
}

fn trial_get_guides_by_object_ids(/* ... */) -> Trial {
    // Wait for: init-guides
    // GET by object_ids: verify POST response matches GET response
    // Verify: chapter ordering, block ordering, tag presence
}

fn trial_get_guides_by_slugs(/* ... */) -> Trial {
    // Wait for: init-guides
    // GET by slugs: verify slug lookup works
}

fn trial_get_guides_all(/* ... */) -> Trial {
    // Wait for: init-guides
    // GET all: verify guides appear in results
}

fn trial_patch_guides(/* ... */) -> Trial {
    // Wait for: init-guides
    // PATCH: Update description (creates new version 0.0.2)
    // Verify: New version exists, old version unchanged
}

fn trial_deprecate_guides(/* ... */) -> Trial {
    // Wait for: init-guides
    // DELETE with deprecation level
    // Verify: Guide is deprecated, not destroyed
}
```

### Parser Unit Tests (Phase 2)

```rust
// In crates/dapple-data/src/parser/tests/

#[test]
fn test_parse_simple_block() {
    let input = r#"
:::block { tags: [agent, product] }
This is some content.
:::
"#;
    let blocks = parse_blocks(input);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].directive_type, "block");
    assert_eq!(blocks[0].tags, vec!["agent", "product"]);
    assert_eq!(blocks[0].content.trim(), "This is some content.");
}

#[test]
fn test_parse_nested_directives() {
    let input = r#"
:::stage { name: "Onboarding" }
# Onboarding

:::step { name: "Gather Info" }

:::nushell { tags: [coder], name: "process" }
def main [] { { payload: {}, signals: {} } }
:::

:::  <!-- end step -->

:::  <!-- end stage -->
"#;
    let blocks = parse_blocks(input);
    // Should produce: stage-open, prose, step-open, nushell, step-close, stage-close
    // Implementation detail: how nesting is represented in ParsedBlock
}

#[test]
fn test_parse_summary_md() {
    let summary = r#"
# Summary

[Preface](./preface.md)

- [Introduction](./getting-started/intro.md)

# Getting Started

- [Glossary](./getting-started/glossary.md)
- [Concepts](./getting-started/concepts.md)
"#;
    let chapters = parse_summary(summary);
    assert_eq!(chapters.len(), 4); // preface, intro, glossary, concepts
    assert_eq!(chapters[0].title, "Preface");
    assert_eq!(chapters[0].source_file, "preface.md");
}
```

### Ingestion Pipeline Tests (Phase 3)

```rust
// In crates/dapple-data/tests/ or inline in ingest module

#[sqlx::test(migrations = "migrations")]
async fn test_ingest_fresh_guide(pool: PgPool) {
    // Create a temp directory with a minimal guide structure
    // Run ingest_guide()
    // Verify guide, chapters, blocks, tags in database
}

#[sqlx::test(migrations = "migrations")]
async fn test_ingest_re_ingestion(pool: PgPool) {
    // Ingest a guide
    // Modify content
    // Re-ingest
    // Verify new version created, old version unchanged
}
```

### Verification Points

1. **POST/GET consistency:** POST response order matches GET response for chapters and blocks.
2. **Versioning:** Fresh ingestion creates v0.0.1. Re-ingestion creates v0.0.2. Path is shared.
3. **Tag integrity:** Each block's tags are stored correctly and returned in GET.
4. **Chapter nesting:** `parent_chapter_object_id` correctly reflects the chapter tree.
5. **Provenance:** After materialization (Phase 4+), derived objects link back to source blocks.
6. **API blocking:** After Phase 8, attempting to PATCH a guide-derived task returns 409 Conflict.
7. **Soft delete:** Deprecating a guide does not destroy chapters, blocks, or derived objects.
8. **Re-ingestion removal:** Removing a block in re-ingestion deprecates its derived objects.

---

## 9. AGENTS.md Updates

The following should be added to `AGENTS.md` after Phase 1 is implemented:

```markdown
## Guide Ingestion Pattern

When implementing features that involve guides:

1. **Guides are versioned principals** -- They use the unified versioning infrastructure
   (object_paths -> guide_path_details -> versioned_objects -> guides).

2. **Chapters and blocks are owned dependents** -- They are created atomically within a
   transaction when a guide is inserted. No separate endpoints for chapters or blocks.

3. **Tags are from a lookup table** -- Do not create new tags without a migration.
   Tag keys map to BriefUnit unit_key values at materialization time.

4. **Guide-derived objects are read-only via API** -- Check `rev.guide_derived_locks`
   before allowing PATCH/DELETE on tasks, steps, stages, playbooks, briefs, brief_groups,
   brief_packs, and inference_profiles. Return 409 Conflict if locked.

5. **Re-ingestion creates new versions** -- Never UPDATE existing guide data. Use the
   path versioning mechanism to create v0.0.2, v0.0.3, etc.

6. **Block-level provenance is mandatory** -- Every materialized object must have a
   `guide_block_provenances` entry linking it to the source block.

7. **Playbook derivation requires explicit annotation** -- Only chapters with `:::stage`
   directives produce Playbook stages. Nothing is inferred.

See `docs/plans/guides_as_first_class_entities_architecture_plan.md` for full details.
```

---

## 10. Open Questions and Risks

### Open Questions

1. **Chapter identity during re-ingestion:** When a chapter is renamed or moved to a different position in SUMMARY.md, should we match by source_file path (stable) or by title (human-readable)? **Recommendation:** Match by `source_file` since it is the filesystem identity of the chapter.

2. **Block identity without names:** Not all blocks have explicit `name` attributes. How do we match unnamed blocks during re-ingestion? **Recommendation:** Use (chapter_source_file, directive_type, position_within_chapter) as the composite key. This means reordering blocks within a chapter will cause a "remove old + create new" cycle rather than an update-in-place.

3. **Transaction size for large guides:** A guide with 50 chapters and 500 blocks will produce a large transaction. Is this acceptable? **Recommendation:** Yes, for initial implementation. PostgreSQL handles transactions of this size routinely. If performance becomes an issue, batch the chapter/block inserts in groups of 100.

4. **Concurrent re-ingestion:** What happens if two CI/CD pipelines try to re-ingest the same guide simultaneously? **Recommendation:** The path-level UNIQUE constraint on `guide_path_details.slug` will cause one to fail at the path lookup/version increment stage. This is acceptable -- the failed pipeline retries.

5. **Guide deletion impact:** If a guide is deprecated, should all its derived objects also be deprecated? **Recommendation:** Yes, cascade deprecation to all derived objects via the `guide_derived_locks` table. This is implemented in Phase 8.

### Risks

1. **Parser complexity for edge cases:** Markdown is notoriously tricky to parse. Custom fences nested inside code blocks, block quotes, or other markdown constructs could cause parser failures. **Mitigation:** The parser operates at the line level and only matches `:::` at the start of a line (with optional leading whitespace). Content inside code blocks (triple backticks) is treated as opaque.

2. **Materialization ordering dependencies:** Phase 7 (playbook derivation) depends on Phases 5-6 (task and brief materialization) completing first. Within a single ingestion transaction, this ordering must be enforced. **Mitigation:** The ingestion pipeline runs materialization in strict phase order within the transaction.

3. **Schema evolution of :::brief-pack content:** The YAML content inside `:::brief-pack` blocks defines a mini-schema (groups, briefs, unit types, controls). If the Brief/BriefUnit schema evolves, the parser needs to evolve too. **Mitigation:** Version the `:::brief-pack` content schema with an explicit `version` attribute in the directive metadata. Default to v1 if not specified.

4. **Large guide SELECT queries:** The `select_guides_by_object_ids.sql` query joins guides with chapters, blocks, tags, and provenance. For a guide with many chapters and blocks, this produces a large result set. **Mitigation:** Use ARRAY_AGG for tags to reduce row multiplication. Consider splitting chapter/block fetching into separate queries if performance requires it (similar to how BriefUnits are fetched separately from Briefs).

5. **Backward compatibility of `dapple` CLI:** The CLI crate is currently a stub. Adding substantial functionality (clap, sqlx, database connectivity) changes its dependency footprint significantly. **Mitigation:** This is expected. The CLI has always been intended to grow. Ensure the binary is lean (no unnecessary feature flags).

---

## 11. Phase Dependency Graph

```
Phase 1: Guide Data Model (DDL + model + query + handler + tests)
    |
    v
Phase 2: Custom Fence Parser (parser module, unit tests)
    |
    v
Phase 3: Ingestion Pipeline (CLI + ingest module, integration tests)
    |
    +-------+-------+-------+
    |       |       |       |
    v       v       v       v
Phase 4  Phase 5  Phase 6  Phase 7
Content  Task     Brief    Playbook
Matrlzn  Matrlzn  Matrlzn  Derivation
    |       |       |       |
    +-------+-------+-------+
    |
    v
Phase 8: Provenance + API Blocking
    |
    v
Phase 9: Re-Ingestion Diffing
    |
    +-------+
    |       |
    v       v
Phase 10  Phase 11
Foundation Object Refs
Guide
    |       |
    +-------+
    |
    v
Phase 12: CI/CD Integration
    |
    v
Phase 13: mdBook Preprocessor
```

**Critical path:** 1 -> 2 -> 3 -> (4,5,6) -> 7 -> 8 -> 9

**Parallel work streams:**
- Phases 4, 5, 6 can proceed in parallel once Phase 3 is done.
- Phase 7 depends on 4, 5, 6 because playbook derivation wires in materialized tasks/briefs.
- Phases 10 and 11 can proceed in parallel once Phase 9 is done.
- Phase 13 can proceed independently at any time after Phase 2 (shares the parser).

---

## 12. File Inventory Summary

### New Migrations (in `crates/dapple-data/migrations/`)

| File | Phase | Description |
|------|-------|-------------|
| `20260214000001_guide_entity_types.sql` | 1 | Register 'guide' entity type |
| `20260214000002_guide_tags.sql` | 1 | Tags lookup table + seed |
| `20260214000003_guide_block_directive_types.sql` | 1 | Directive types lookup + seed |
| `20260214000004_guide_path_details.sql` | 1 | Path-level guide details |
| `20260214000005_guide_tables.sql` | 1 | Guide, chapter, block, block_tag tables |
| `20260214000006_guide_provenance.sql` | 8 | Provenance + derived lock tables |

### New SQL Files (in `crates/dapple-data/assets/sql/guides/`)

| File | Phase | Description |
|------|-------|-------------|
| `insert_guides.sql` | 1 | Insert guide with unified versioning CTE chain |
| `insert_guide_chapters.sql` | 1 | Insert chapters as owned dependents |
| `insert_guide_blocks.sql` | 1 | Insert blocks as owned dependents |
| `insert_guide_block_tags.sql` | 1 | Insert block-tag associations |
| `select_guides_by_object_ids.sql` | 1 | Full guide query with chapter/block/tag/provenance joins |
| `select_guides.sql` | 1 | All guides with FilterOptions |
| `select_guides_by_slugs.sql` | 1 | By slug lookup |
| `select_guides_by_path_ids.sql` | 1 | By path object_id lookup |
| `deprecate_guides.sql` | 1 | Soft delete via deprecation |
| `patch_guides.sql` | 1 | Create new version of existing guide |
| `insert_guide_block_provenances.sql` | 8 | Insert provenance records |
| `insert_guide_derived_locks.sql` | 8 | Insert derived locks |
| `select_locked_versioned_objects.sql` | 8 | Check if objects are guide-locked |

### New Rust Files (in `crates/dapple-data/src/`)

| File | Phase | Description |
|------|-------|-------------|
| `model/guides.rs` | 1 | Guide DTOs, internal models, insert args |
| `model/guides/guide_chapters.rs` | 1 | Chapter models |
| `model/guides/guide_blocks.rs` | 1 | Block models |
| `model/guides/guide_block_tags.rs` | 1 | Block tag models |
| `model/guides/guide_block_provenances.rs` | 8 | Provenance models |
| `query/guides.rs` | 1 | Guide CRUD query functions |
| `query/guides/provenance.rs` | 8 | Provenance query functions |
| `query/guides/locks.rs` | 8 | Lock check query functions |
| `parser/mod.rs` | 2 | Parser module root |
| `parser/fence.rs` | 2 | Fence directive tokenizer |
| `parser/summary.rs` | 2 | SUMMARY.md parser |
| `parser/block.rs` | 2 | Block classifier |
| `ingest/mod.rs` | 3 | Ingestion pipeline module |
| `ingest/guide.rs` | 3 | Guide ingestion orchestrator |
| `ingest/diff.rs` | 9 | Re-ingestion diff engine |
| `ingest/materialize.rs` | 4 | Materialization orchestrator |
| `ingest/materialize/content.rs` | 4 | Prose -> BriefUnit |
| `ingest/materialize/tasks.rs` | 5 | Nushell/InferenceProfile -> Task |
| `ingest/materialize/briefs.rs` | 6 | BriefPack -> Brief hierarchy |
| `ingest/materialize/playbooks.rs` | 7 | Stage/Step -> Playbook |
| `ingest/materialize/refs.rs` | 11 | Object reference resolution |

### New Rust Files (in `crates/dapple-prompt-api/`)

| File | Phase | Description |
|------|-------|-------------|
| `src/schema/guides.rs` | 1 | Create/Get/Patch schemas |
| `src/handler/guides.rs` | 1 | POST/GET/PATCH/DELETE handlers |
| `src/route/guides.rs` | 1 | Route registration |
| `tests/consumer/guides.rs` | 1 | Test consumer endpoint |

### Modified Rust Files

| File | Phase | Description |
|------|-------|-------------|
| `crates/dapple-data/src/model/mod.rs` | 1 | Add `pub mod guides;` |
| `crates/dapple-data/src/model/versioned_objects.rs` | 1 | Add `GUIDE` constant |
| `crates/dapple-data/src/query/mod.rs` | 1 | Add `pub mod guides;` |
| `crates/dapple-data/src/lib.rs` | 2-3 | Add `pub mod parser;` and `pub mod ingest;` |
| `crates/dapple-prompt-api/src/handler/mod.rs` | 1 | Import guides handler |
| `crates/dapple-prompt-api/src/route/mod.rs` | 1 | Import guides routes |
| `crates/dapple-prompt-api/src/schema/mod.rs` | 1 | Import guides schema |
| `crates/dapple-prompt-api/tests/consumer/mod.rs` | 1 | Add guide batches to ScenarioContext |
| `crates/dapple-prompt-api/tests/user_lifecycle.rs` | 1 | Add guide lifecycle trials |
| `crates/dapple/src/lib.rs` | 3 | Add CLI command for guide import |
| `crates/dapple/Cargo.toml` | 3 | Add dependencies |
| `crates/dapple-prompt-api/src/handler/tasks.rs` | 8 | Add lock check |
| `crates/dapple-prompt-api/src/handler/steps.rs` | 8 | Add lock check |
| `crates/dapple-prompt-api/src/handler/stages.rs` | 8 | Add lock check |
| `crates/dapple-prompt-api/src/handler/playbooks.rs` | 8 | Add lock check |
| `crates/dapple-prompt-api/src/handler/briefs.rs` | 8 | Add lock check |
| `crates/dapple-prompt-api/src/handler/brief_groups.rs` | 8 | Add lock check |
| `crates/dapple-prompt-api/src/handler/brief_packs.rs` | 8 | Add lock check |
| `crates/dapple-prompt-api/src/handler/inference_profiles.rs` | 8 | Add lock check |

### New Crate (Phase 13)

| Crate | Description |
|-------|-------------|
| `crates/dapple-mdbook-preprocessor/` | mdBook preprocessor for rendering `:::` directives |

---

## 13. Anti-Patterns Specific to This Feature

- **Creating separate endpoints for chapters, blocks, or tags.** These are all owned dependents of Guide. They are created/queried as part of the Guide's POST/GET response. No `/api/guide-chapters/` or `/api/guide-blocks/` endpoints.
- **Storing tags inline as a TEXT[] column on guide_blocks.** Tags must use a lookup table with referential integrity. The `guide_block_tags` associative table enforces this.
- **Inferring playbook structure from markdown headers.** Playbook derivation is fully annotated. Only explicit `:::stage` and `:::step` directives create playbook structure.
- **Creating UPDATE operations on guide data.** Guides are versioned. Changes create new versions, never mutate existing data.
- **Running materialization outside of the ingestion transaction.** All derived objects must be created atomically within the same transaction as the guide write.
- **Using tag inheritance from chapters to blocks.** Every block declares its own tags. No inheritance.
- **Creating `get_guide_tag_internal_ids` helper queries.** Resolve tag keys to IDs via CTE joins in the insert SQL, not via separate lookup queries.
