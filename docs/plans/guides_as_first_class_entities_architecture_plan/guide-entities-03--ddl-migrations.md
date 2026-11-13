# Issue 3: Guide DDL Migrations

**Status:** Draft
**Created:** 2026-02-13
**Dependencies:** Issue 2 (Slugify function -- `rev.slugify()` must exist before guide inserts use it)
**Blocks:** Issue 4 (Models), Issue 5 (SQL Queries)

---

## 1. Summary

Create five database migration files that establish the complete guide data model within the unified versioning infrastructure. The migrations register the `guide` entity type, create lookup tables for tags and directive types, create the `guide_path_details` table, and create the entity-specific tables for guides, chapters, blocks, and block tags.

All tables are created in the `rev` schema, consistent with the existing versioning infrastructure.

---

## 2. Migration Files

### Migration Ordering

| Order | File | Description |
|-------|------|-------------|
| 1 | `20260214000002_guide_entity_type.sql` | Register 'guide' entity type |
| 2 | `20260214000003_guide_tags.sql` | Tags lookup table + seed data |
| 3 | `20260214000004_guide_block_directive_types.sql` | Directive types lookup + seed data |
| 4 | `20260214000005_guide_path_details.sql` | Path-level guide details |
| 5 | `20260214000006_guide_tables.sql` | Guide, chapter, block, block_tag tables |

Note: Migration `20260214000001` is reserved for the slugify function (Issue 2). The guide migrations start at `000002`.

---

### Migration 1: Entity Type Registration

**File:** `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/migrations/20260214000002_guide_entity_type.sql`

```sql
-- Register 'guide' entity type for the unified versioning infrastructure.
-- This follows the exact pattern used by 20260123000001_directive_entity_type.sql.
INSERT INTO rev.entity_types (key, description) VALUES
    ('guide', 'Structured narrative documents that derive runtime objects');
```

**Pattern reference:** This is identical in structure to `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/migrations/20260123000001_directive_entity_type.sql`.

**Verification after migration:**
```sql
SELECT * FROM rev.entity_types WHERE key = 'guide';
-- Should return one row with key='guide'
```

---

### Migration 2: Guide Tags Lookup Table

**File:** `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/migrations/20260214000003_guide_tags.sql`

```sql
-- Guide tags are an authoring-time concern for audience targeting.
-- Each tag optionally maps to a brief_unit key used when materializing
-- tagged blocks into BriefUnit objects.
--
-- Tags are a lookup table with referential integrity. Adding new tags
-- requires a migration (Design Decision #8).
CREATE TABLE rev.guide_tags (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL,
    description TEXT,
    brief_unit_key TEXT,
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

**Design notes:**
- `SMALLINT` PK because this is a small lookup table (expected <50 rows total).
- `brief_unit_key` is nullable -- the `universal` tag has no specific BriefUnit mapping.
- The `UNIQUE (key)` constraint ensures tag keys are globally unique.

**Verification:**
```sql
SELECT count(*) FROM rev.guide_tags;
-- Should return 6
```

---

### Migration 3: Guide Block Directive Types Lookup Table

**File:** `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/migrations/20260214000004_guide_block_directive_types.sql`

```sql
-- Classifies the directive type of each block in a guide.
-- This is used by the block classifier in the parser and by the
-- materialization pipeline to determine what runtime object to create.
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

**Verification:**
```sql
SELECT count(*) FROM rev.guide_block_directive_types;
-- Should return 7
```

---

### Migration 4: Guide Path Details

**File:** `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/migrations/20260214000005_guide_path_details.sql`

```sql
-- Path-level data for guides.
-- Title and slug are path-level because they identify the guide across versions.
-- A guide's title and slug never change when a new version is created -- only
-- the versioned content (chapters, blocks) changes.
--
-- This follows the exact pattern used by rev.hat_path_details, rev.task_path_details, etc.
-- See: crates/dapple-data/migrations/20260120000002_create_path_detail_tables.sql
CREATE TABLE rev.guide_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    title TEXT NOT NULL,
    slug TEXT NOT NULL,
    UNIQUE (title),
    UNIQUE (slug)
);
```

**Pattern reference:** This matches the structure of `rev.hat_path_details`:
```sql
-- From 20260120000002_create_path_detail_tables.sql:
CREATE TABLE rev.hat_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    UNIQUE (name),
    UNIQUE (slug)
);
```

The only difference is the column name: `title` instead of `name`, since guides use "title" terminology (matching mdBook convention).

---

### Migration 5: Guide Entity Tables

**File:** `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/migrations/20260214000006_guide_tables.sql`

```sql
-- ============================================================================
-- Guide entity table (versioned, entity-specific fields only)
-- ============================================================================
-- The guide table stores version-specific data. Path-level data (title, slug)
-- lives in guide_path_details. Version metadata (version numbers, deprecation)
-- lives in versioned_objects. The guide table only contains fields that can
-- change between versions.
CREATE TABLE rev.guides (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    versioned_object_id BIGINT NOT NULL REFERENCES rev.versioned_objects (id) UNIQUE,
    description TEXT,
    locale TEXT NOT NULL DEFAULT 'en',
    author TEXT,
    classification TEXT,
    source_path TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guides_versioned_object_idx ON rev.guides (versioned_object_id);

-- ============================================================================
-- Guide Chapters (Owned Dependent of Guide)
-- ============================================================================
-- Maps to a single .md source file in the guide. Chapters are owned by a guide
-- version and are recreated (not updated) when a new guide version is ingested.
CREATE TABLE rev.guide_chapters (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    guide_id BIGINT NOT NULL REFERENCES rev.guides (id),
    parent_chapter_id BIGINT REFERENCES rev.guide_chapters (id),
    title TEXT NOT NULL,
    source_file TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guide_chapters_guide_idx ON rev.guide_chapters (guide_id);
CREATE INDEX guide_chapters_parent_idx ON rev.guide_chapters (parent_chapter_id);

-- ============================================================================
-- Guide Blocks (Owned Dependent of Chapter)
-- ============================================================================
-- The atomic content unit. Each block has a directive type (prose, stage, step,
-- nushell, etc.) and content. Blocks are ordered by object_id (UUIDv7).
CREATE TABLE rev.guide_blocks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    chapter_id BIGINT NOT NULL REFERENCES rev.guide_chapters (id),
    directive_type_id SMALLINT NOT NULL REFERENCES rev.guide_block_directive_types (id),
    name TEXT,
    content TEXT NOT NULL,
    attributes JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guide_blocks_chapter_idx ON rev.guide_blocks (chapter_id);
CREATE INDEX guide_blocks_directive_type_idx ON rev.guide_blocks (directive_type_id);

-- ============================================================================
-- Guide Block Tags (Associative: block <-> tag)
-- ============================================================================
-- No separate endpoint. Created atomically with block insertion.
-- A block can have zero or more tags from the guide_tags lookup table.
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

---

## 3. Schema Diagram

```
rev.entity_types
    |
    v (entity_type_id)
rev.object_paths
    |
    +---> rev.guide_path_details (path_id PK/FK)
    |         title, slug
    |
    v (path_id)
rev.versioned_objects
    |         version_major, version_minor, version_patch
    |         deprecation_level_id
    |
    v (versioned_object_id)
rev.guides
    |         description, locale, author, classification, source_path
    |
    v (guide_id)
rev.guide_chapters
    |         title, source_file, parent_chapter_id (self-ref)
    |
    v (chapter_id)
rev.guide_blocks
    |         directive_type_id -> rev.guide_block_directive_types
    |         name, content, attributes
    |
    v (block_id)
rev.guide_block_tags
              tag_id -> rev.guide_tags
```

---

## 4. Convention Compliance Checklist

| Convention | Status |
|-----------|--------|
| Primary keys: `BIGINT GENERATED ALWAYS AS IDENTITY` | Applied to all tables |
| Lookup table PKs: `SMALLINT GENERATED ALWAYS AS IDENTITY` | Applied to guide_tags and guide_block_directive_types |
| `object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE` | Applied to guides, guide_chapters, guide_blocks, guide_block_tags |
| `TIMESTAMPTZ` (not `timestamp`) | All `created_at` columns |
| `TEXT` (not `varchar`) | All string columns |
| Foreign keys named `{table_singular}_id` | `guide_id`, `chapter_id`, `block_id`, `tag_id`, `directive_type_id` |
| Explicit indexes on all foreign keys | All FK columns indexed |
| `versioned_object_id` references `rev.versioned_objects(id)` with `UNIQUE` | Applied to `rev.guides` |
| Path details table: `path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths(id)` | Applied to `guide_path_details` |
| Associative table: composite `UNIQUE` constraint | `UNIQUE (block_id, tag_id)` on `guide_block_tags` |
| Seed data in migration files | Tags and directive types seeded in their respective migrations |
| No `UPDATE` operations | Append-only -- new versions create new guide + chapters + blocks rows |
| No `serial` type | Using `GENERATED ALWAYS AS IDENTITY` |
| No `varchar` or `char` | Using `TEXT` |
| No `timestamp` without timezone | Using `TIMESTAMPTZ` |

---

## 5. Relationship to Existing Infrastructure

### How Guide Inserts Will Work (CTE Chain)

The insert query (Issue 5) will follow the same CTE chain as `insert_hats.sql`:

```
args CTE
  -> entity_type CTE (SELECT id FROM rev.entity_types WHERE key = 'guide')
  -> new_paths CTE (INSERT INTO rev.object_paths)
  -> numbered_paths CTE
  -> path_details CTE (INSERT INTO rev.guide_path_details)
  -> new_versions CTE (INSERT INTO rev.versioned_objects)
  -> numbered_versions CTE
  -> new_guides CTE (INSERT INTO rev.guides)
  -> final SELECT joining all CTEs
```

### How Guide Selects Will Work

Select queries join through the versioning chain:

```sql
FROM rev.guides g
JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = vo.deprecation_level_id
LEFT JOIN rev.guide_chapters gc ON gc.guide_id = g.id
LEFT JOIN rev.guide_blocks gb ON gb.chapter_id = gc.id
-- ... etc.
```

---

## 6. Verification Steps

After running all five migrations:

```bash
cargo sqlx migrate run
cargo sqlx migrate info  # Verify all 5 migrations show as applied
```

### Table Existence Checks

```sql
SELECT table_name FROM information_schema.tables
WHERE table_schema = 'rev' AND table_name LIKE 'guide%'
ORDER BY table_name;
```

Expected:
```
guide_block_directive_types
guide_block_tags
guide_blocks
guide_chapters
guide_path_details
guide_tags
guides
```

### Seed Data Checks

```sql
SELECT key FROM rev.entity_types WHERE key = 'guide';
-- Expected: 1 row

SELECT count(*) FROM rev.guide_tags;
-- Expected: 6

SELECT count(*) FROM rev.guide_block_directive_types;
-- Expected: 7
```

### Foreign Key Checks

```sql
-- Verify guide_path_details can reference object_paths
INSERT INTO rev.object_paths (entity_type_id)
    SELECT id FROM rev.entity_types WHERE key = 'guide'
    RETURNING id, object_id;
-- Should succeed

-- Verify guide_chapters self-reference works
-- (tested implicitly via the insert queries in Issue 5)
```

### Index Checks

```sql
SELECT indexname FROM pg_indexes WHERE tablename = 'guides';
-- Expected: guides_pkey, guides_object_id_key, guides_versioned_object_idx

SELECT indexname FROM pg_indexes WHERE tablename = 'guide_chapters';
-- Expected: guide_chapters_pkey, guide_chapters_object_id_key,
--           guide_chapters_guide_idx, guide_chapters_parent_idx

SELECT indexname FROM pg_indexes WHERE tablename = 'guide_blocks';
-- Expected: guide_blocks_pkey, guide_blocks_object_id_key,
--           guide_blocks_chapter_idx, guide_blocks_directive_type_idx

SELECT indexname FROM pg_indexes WHERE tablename = 'guide_block_tags';
-- Expected: guide_block_tags_pkey, guide_block_tags_object_id_key,
--           guide_block_tags_block_id_tag_id_key,
--           guide_block_tags_block_idx, guide_block_tags_tag_idx
```

---

## 7. Notes for Implementation Agent

- **Run migrations in order.** Each migration depends on the previous one (guide_tables references guide_block_directive_types and guide_tags).
- **Do not add `ALTER TABLE ... OWNER TO dapple`** statements unless they exist in other recent migrations. The directive path details migration (`20260123000002`) includes this, but newer migrations do not. Follow the most recent convention.
- **The `parent_chapter_id` self-reference** on `guide_chapters` allows chapter nesting. When inserting chapters, top-level chapters (no parent) are inserted first, then nested chapters reference the newly created parent IDs. This requires the insert to be done in two passes within a transaction (top-level first, then nested), OR all chapters are inserted with `parent_chapter_id = NULL` first, then updated. The recommended approach is topological ordering in the insert query (see Issue 5).
- **The `attributes JSONB` column** on `guide_blocks` stores the raw parsed attributes from the `:::directive { ... }` metadata. This is a schemaless field by design -- different directive types have different attribute shapes.
- **Do not create the provenance tables** (`guide_block_provenances`, `guide_derived_locks`) in this issue. Those belong to Phase 8 of the architecture plan and will be a separate migration after the materialization pipeline is built.
