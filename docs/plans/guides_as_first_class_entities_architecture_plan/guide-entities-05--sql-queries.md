# Issue 5: Guide SQL Queries and Rust Query Functions

**Status:** Draft
**Created:** 2026-02-13
**Dependencies:** Issue 3 (DDL), Issue 4 (Models)
**Blocks:** Issue 6 (API Layer)

---

## 1. Summary

Create all SQL query files and corresponding Rust query functions for guide CRUD operations. This includes:

- **INSERT:** Guide (with versioning CTE chain), chapters, blocks, block tags
- **SELECT:** By object IDs, by slugs, by path IDs, all (with FilterOptions)
- **PATCH:** Create new version of existing guide
- **DEPRECATE:** Soft delete via deprecation events

---

## 2. File Inventory

### SQL Files

All SQL files go in `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/assets/sql/guides/`.

| File | Description |
|------|-------------|
| `insert_guides.sql` | Insert guide with full unified versioning CTE chain |
| `insert_guide_chapters.sql` | Insert chapters as owned dependents |
| `insert_guide_blocks.sql` | Insert blocks as owned dependents |
| `insert_guide_block_tags.sql` | Insert block-tag associations |
| `select_guides_by_object_ids.sql` | Full guide with chapters/blocks/tags by object_id |
| `select_guides.sql` | All guides with FilterOptions |
| `select_guides_by_slugs.sql` | By slug lookup |
| `select_guides_by_path_ids.sql` | By path object_id lookup |
| `patch_guides.sql` | Create new version of existing guide |
| `deprecate_guides.sql` | Soft delete via deprecation |

### Rust Files

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/query/guides.rs` | Create | All guide query functions |
| `crates/dapple-data/src/query.rs` | Modify | Add `pub mod guides;` |

---

## 3. SQL Query Definitions

### 3.1 insert_guides.sql

**File:** `crates/dapple-data/assets/sql/guides/insert_guides.sql`

This follows the exact CTE chain from `insert_hats.sql` at `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/assets/sql/hats/insert_hats.sql`.

```sql
-- Insert guides using the unified versioning infrastructure
-- Creates: object_path -> guide_path_details -> versioned_object -> guide
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
-- Get the entity type ID for guides
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'guide'
),
-- Create object paths (ORDER BY ensures sequential id assignment)
new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT (SELECT id FROM entity_type)
    FROM args
    ORDER BY args.ord
    RETURNING id, object_id
),
-- Number the paths by id to correlate with args.ord
numbered_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),
-- Create path details (title, slug)
path_details AS (
    INSERT INTO rev.guide_path_details (path_id, title, slug)
    SELECT
        np.id,
        args.title,
        rev.slugify(args.title)
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, title, slug
),
-- Create versioned objects (version 0.0.1)
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, version_major, version_minor, version_patch)
    SELECT np.id, 0, 0, 1
    FROM new_paths np
    ORDER BY np.id
    RETURNING id, object_id, path_id, version_major, version_minor, version_patch, created_at
),
-- Number versions to preserve ordinality
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),
-- Number path details to preserve ordinality
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
),
-- Create guides
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
-- Number guides to preserve ordinality
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

**Note:** This uses `rev.slugify(args.title)` from Issue 2 instead of the inline `lower(regexp_replace(...))`.

---

### 3.2 insert_guide_chapters.sql

**File:** `crates/dapple-data/assets/sql/guides/insert_guide_chapters.sql`

```sql
-- Insert guide chapters as owned dependents of a guide.
-- Chapters are inserted in topological order (parents before children).
-- The parent_chapter_id is NULL for top-level chapters.
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

---

### 3.3 insert_guide_blocks.sql

**File:** `crates/dapple-data/assets/sql/guides/insert_guide_blocks.sql`

```sql
-- Insert guide blocks as owned dependents of chapters.
-- Tags are inserted separately via insert_guide_block_tags.sql.
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
    dt.key AS directive_type_key,
    nb.name,
    nb.content,
    nb.attributes,
    nb.created_at
FROM new_blocks nb
JOIN rev.guide_block_directive_types dt ON dt.id = nb.directive_type_id
ORDER BY nb.id;
```

---

### 3.4 insert_guide_block_tags.sql

**File:** `crates/dapple-data/assets/sql/guides/insert_guide_block_tags.sql`

```sql
-- Insert block-tag associations.
-- Each pair (block_id, tag_id) creates one row.
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

---

### 3.5 select_guides_by_object_ids.sql

**File:** `crates/dapple-data/assets/sql/guides/select_guides_by_object_ids.sql`

```sql
-- Select guides by object IDs with full chapter/block/tag hierarchy.
-- Uses ARRAY_AGG for block tags to produce one row per block.
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
    dl.key AS "deprecation_level_key?",
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
    gb.created_at AS block_created_at,
    -- Block tags (aggregated into array)
    ARRAY_REMOVE(ARRAY_AGG(DISTINCT gt.key), NULL) AS block_tag_keys
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
GROUP BY
    g.id, g.object_id, g.versioned_object_id, vo.path_id, op.object_id,
    gpd.title, gpd.slug, vo.version_major, vo.version_minor, vo.version_patch,
    dl.key, g.description, g.locale, g.author, g.classification, g.source_path,
    g.created_at,
    gc.id, gc.object_id, gc.parent_chapter_id, pc.object_id, gc.title,
    gc.source_file, gc.created_at,
    gb.id, gb.object_id, bdt.key, gb.name, gb.content, gb.attributes,
    gb.created_at
ORDER BY g.object_id, gc.object_id, gb.object_id
LIMIT $2 OFFSET $3;
```

**Note:** The `ARRAY_REMOVE(..., NULL)` handles the case where a block has no tags (LEFT JOIN produces NULL).

---

### 3.6 select_guides.sql

**File:** `crates/dapple-data/assets/sql/guides/select_guides.sql`

Same structure as `select_guides_by_object_ids.sql` but without the `args` CTE filter:

```sql
-- Select all guides with FilterOptions.
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
    dl.key AS "deprecation_level_key?",
    g.description,
    g.locale,
    g.author,
    g.classification,
    g.source_path,
    g.created_at,
    gc.id AS chapter_id,
    gc.object_id AS chapter_object_id,
    gc.parent_chapter_id AS chapter_parent_chapter_id,
    pc.object_id AS chapter_parent_chapter_object_id,
    gc.title AS chapter_title,
    gc.source_file AS chapter_source_file,
    gc.created_at AS chapter_created_at,
    gb.id AS block_id,
    gb.object_id AS block_object_id,
    bdt.key AS block_directive_type_key,
    gb.name AS block_name,
    gb.content AS block_content,
    gb.attributes AS block_attributes,
    gb.created_at AS block_created_at,
    ARRAY_REMOVE(ARRAY_AGG(DISTINCT gt.key), NULL) AS block_tag_keys
FROM rev.guides g
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
GROUP BY
    g.id, g.object_id, g.versioned_object_id, vo.path_id, op.object_id,
    gpd.title, gpd.slug, vo.version_major, vo.version_minor, vo.version_patch,
    dl.key, g.description, g.locale, g.author, g.classification, g.source_path,
    g.created_at,
    gc.id, gc.object_id, gc.parent_chapter_id, pc.object_id, gc.title,
    gc.source_file, gc.created_at,
    gb.id, gb.object_id, bdt.key, gb.name, gb.content, gb.attributes,
    gb.created_at
ORDER BY g.object_id, gc.object_id, gb.object_id
LIMIT $1 OFFSET $2;
```

---

### 3.7 select_guides_by_slugs.sql

Same as `select_guides_by_object_ids.sql` but joins through slugs:

```sql
WITH args AS (
    SELECT *
    FROM UNNEST($1::TEXT[]) WITH ORDINALITY AS o(slug, ord)
)
SELECT
    -- ... identical column list as select_guides_by_object_ids.sql ...
FROM args
JOIN rev.guide_path_details gpd ON gpd.slug = args.slug
JOIN rev.object_paths op ON op.id = gpd.path_id
JOIN rev.versioned_objects vo ON vo.path_id = op.id
JOIN rev.guides g ON g.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = vo.deprecation_level_id
LEFT JOIN rev.guide_chapters gc ON gc.guide_id = g.id
LEFT JOIN rev.guide_chapters pc ON pc.id = gc.parent_chapter_id
LEFT JOIN rev.guide_blocks gb ON gb.chapter_id = gc.id
LEFT JOIN rev.guide_block_directive_types bdt ON bdt.id = gb.directive_type_id
LEFT JOIN rev.guide_block_tags gbt ON gbt.block_id = gb.id
LEFT JOIN rev.guide_tags gt ON gt.id = gbt.tag_id
GROUP BY /* same as above */
ORDER BY g.object_id, gc.object_id, gb.object_id
LIMIT $2 OFFSET $3;
```

**Important:** When looking up by slug, we may get multiple versions of the same path. To return only the latest version, add a `DISTINCT ON` or a subquery to pick the max version. For the initial implementation, return all versions and let the caller filter if needed.

---

### 3.8 select_guides_by_path_ids.sql

Same pattern, joining through path object_ids:

```sql
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_object_id, ord)
)
SELECT /* same column list */
FROM args
JOIN rev.object_paths op ON op.object_id = args.path_object_id
JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
JOIN rev.versioned_objects vo ON vo.path_id = op.id
JOIN rev.guides g ON g.versioned_object_id = vo.id
/* ... same LEFT JOINs ... */
```

---

### 3.9 deprecate_guides.sql

**File:** `crates/dapple-data/assets/sql/guides/deprecate_guides.sql`

Follows the exact pattern from `deprecate_hats.sql` at `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/assets/sql/hats/deprecate_hats.sql`.

```sql
-- Deprecate guides by path object IDs.
-- Creates deprecation events for the latest version of each guide path.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::TEXT[]
    ) WITH ORDINALITY AS o(
        path_object_id,
        reason,
        ord
    )
),
latest_versions AS (
    SELECT DISTINCT ON (op.object_id)
        op.object_id AS path_object_id,
        vo.id AS versioned_object_id,
        vo.deprecation_level_id AS from_level_id
    FROM args
    JOIN rev.object_paths op ON op.object_id = args.path_object_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    ORDER BY op.object_id, vo.version_major DESC, vo.version_minor DESC, vo.version_patch DESC
),
resolved AS (
    SELECT
        lv.versioned_object_id,
        lv.from_level_id,
        lv.path_object_id,
        args.reason
    FROM args
    JOIN latest_versions lv ON lv.path_object_id = args.path_object_id
),
events AS (
    INSERT INTO rev.deprecation_events (
        versioned_object_id, from_level_id, to_level_id, reason, deprecated_by_user_id
    )
    SELECT
        resolved.versioned_object_id,
        resolved.from_level_id,
        $3::SMALLINT,
        resolved.reason,
        $4::BIGINT
    FROM resolved
    RETURNING *
),
updated AS (
    UPDATE rev.versioned_objects vo
    SET deprecation_level_id = events.to_level_id
    FROM events
    WHERE vo.id = events.versioned_object_id
)
SELECT resolved.path_object_id
FROM resolved
ORDER BY resolved.path_object_id;
```

---

### 3.10 patch_guides.sql

Follows the patch pattern for versioned objects -- creates a new version of an existing guide path.

```sql
-- Create new versions of existing guides (PATCH).
-- Increments the patch version number.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],   -- path_object_id[]
        $2::TEXT[],    -- description[]
        $3::TEXT[],    -- locale[]
        $4::TEXT[],    -- author[]
        $5::TEXT[],    -- classification[]
        $6::TEXT[]     -- source_path[]
    ) WITH ORDINALITY AS o(
        path_object_id,
        description,
        locale,
        author,
        classification,
        source_path,
        ord
    )
),
paths AS (
    SELECT
        op.id AS path_id,
        op.object_id AS path_object_id,
        gpd.title,
        gpd.slug,
        COALESCE(MAX(vo.version_major), 0) AS latest_major,
        COALESCE(MAX(vo.version_minor), 0) AS latest_minor,
        COALESCE(MAX(vo.version_patch), 0) AS latest_patch,
        args.ord
    FROM args
    JOIN rev.object_paths op ON op.object_id = args.path_object_id
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
    LEFT JOIN rev.versioned_objects vo ON vo.path_id = op.id
    GROUP BY op.id, op.object_id, gpd.title, gpd.slug, args.ord
),
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, version_major, version_minor, version_patch)
    SELECT
        paths.path_id,
        paths.latest_major,
        paths.latest_minor,
        paths.latest_patch + 1
    FROM paths
    ORDER BY paths.ord
    RETURNING id, object_id, path_id, version_major, version_minor, version_patch, created_at
),
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
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
    paths.path_object_id,
    paths.title,
    paths.slug,
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
JOIN paths ON paths.ord = nug.ord
ORDER BY nug.id;
```

---

## 4. Rust Query Functions

### File: `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/src/query/guides.rs`

### Module Registration

Add to `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/src/query.rs` (in alphabetical order, after `pub mod fluent_messages;`):

```rust
pub mod guides;
```

### Function Signatures

The query module should expose these public functions following the hat query pattern:

```rust
use crate::error::DataError;
use crate::model::guides::*;
use crate::{DbPool, FilterOptions};
use sqlx::types::Uuid;

// === SELECT functions ===

pub async fn select_guides(pool: &DbPool, filter_ops: FilterOptions) -> Result<Vec<Guide>, DataError>;

pub async fn select_guides_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Guide>, DataError>;

pub async fn select_guide_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<Guide, DataError>;

pub async fn select_guides_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<Guide>, DataError>;

pub async fn select_guides_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<Guide>, DataError>;

// === INSERT function ===

pub async fn insert_guides(
    pool: &DbPool,
    args: Vec<InsertGuideWithChildrenArgs>,
) -> Result<Vec<Guide>, DataError>;

// === DEPRECATE function ===

pub struct DeprecateGuidesArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<i64>,
}

pub async fn deprecate_guides(
    pool: &DbPool,
    args: DeprecateGuidesArgs,
) -> Result<Vec<Guide>, DataError>;
```

### Insert Implementation (Transaction-Based)

The insert function must handle the three-level hierarchy within a single transaction:

```rust
/// High-level insert args that include chapter and block data.
/// This is what the handler builds from the CreateGuideSetSchema.
pub struct InsertGuideWithChildrenArgs {
    pub title: String,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub chapters: Vec<InsertGuideChapterWithBlocksArgs>,
}

pub struct InsertGuideChapterWithBlocksArgs {
    pub title: String,
    pub source_file: String,
    pub parent_index: Option<usize>,  // Index into chapters array for nesting
    pub blocks: Vec<InsertGuideBlockWithTagsArgs>,
}

pub struct InsertGuideBlockWithTagsArgs {
    pub directive_type_key: String,
    pub name: Option<String>,
    pub content: String,
    pub attributes: serde_json::Value,
    pub tag_keys: Vec<String>,
}
```

The transaction flow:

```rust
pub(crate) async fn insert_guide_models(
    pool: &DbPool,
    objs: Vec<InsertGuideWithChildrenArgs>,
) -> Result<Vec<GuideModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;

    // 1. Load lookup tables once
    let directive_types = load_directive_type_lookup(&mut tx).await?;
    let tag_lookup = load_tag_lookup(&mut tx).await?;

    // 2. Insert guide shells (creates path + versioned_object + guide)
    let guide_args = InsertGuideArgs {
        titles: objs.iter().map(|o| o.title.clone()).collect(),
        descriptions: objs.iter().map(|o| o.description.clone()).collect(),
        locales: objs.iter().map(|o| o.locale.clone()).collect(),
        authors: objs.iter().map(|o| o.author.clone()).collect(),
        classifications: objs.iter().map(|o| o.classification.clone()).collect(),
        source_paths: objs.iter().map(|o| o.source_path.clone()).collect(),
    };

    let inserted_guides = sqlx::query_file_as!(
        InsertedGuideModel,
        "assets/sql/guides/insert_guides.sql",
        &guide_args.titles,
        &guide_args.descriptions as &[Option<String>],
        &guide_args.locales,
        &guide_args.authors as &[Option<String>],
        &guide_args.classifications as &[Option<String>],
        &guide_args.source_paths as &[Option<String>],
    )
    .fetch_all(&mut *tx)
    .await?;

    // 3. For each guide, insert chapters (two-pass for parent references)
    let mut all_guide_models = Vec::new();
    for (guide_obj, inserted_guide) in objs.iter().zip(inserted_guides.iter()) {
        let chapter_models = insert_chapters_for_guide(
            &mut tx,
            inserted_guide.id,
            &guide_obj.chapters,
            &directive_types,
            &tag_lookup,
        ).await?;

        let guide_model = GuideModel {
            id: inserted_guide.id,
            object_id: inserted_guide.object_id,
            versioned_object_id: inserted_guide.versioned_object_id,
            path_id: inserted_guide.path_id,
            path_object_id: inserted_guide.path_object_id,
            title: inserted_guide.title.clone(),
            slug: inserted_guide.slug.clone(),
            version_major: inserted_guide.version_major,
            version_minor: inserted_guide.version_minor,
            version_patch: inserted_guide.version_patch,
            deprecation_level_key: None,
            description: inserted_guide.description.clone(),
            locale: inserted_guide.locale.clone(),
            author: inserted_guide.author.clone(),
            classification: inserted_guide.classification.clone(),
            source_path: inserted_guide.source_path.clone(),
            created_at: inserted_guide.created_at,
            chapters: chapter_models,
        };
        all_guide_models.push(guide_model);
    }

    tx.commit().await?;
    Ok(all_guide_models)
}

/// Insert chapters for a single guide, handling parent-child nesting.
async fn insert_chapters_for_guide(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    guide_id: i64,
    chapters: &[InsertGuideChapterWithBlocksArgs],
    directive_types: &HashMap<String, i16>,
    tag_lookup: &HashMap<String, i16>,
) -> Result<Vec<GuideChapterModel>, DataError> {
    // First pass: insert all chapters with parent_chapter_id = NULL
    // to get their IDs, then update parent references.
    //
    // Alternative: topological sort and insert in order.
    // For simplicity, insert top-level first, then nested.

    let chapter_args = InsertGuideChapterArgs {
        guide_ids: vec![guide_id; chapters.len()],
        parent_chapter_ids: vec![None; chapters.len()],  // All NULL initially
        titles: chapters.iter().map(|c| c.title.clone()).collect(),
        source_files: chapters.iter().map(|c| c.source_file.clone()).collect(),
    };

    let inserted_chapters = sqlx::query_file_as!(
        InsertedGuideChapterModel,
        "assets/sql/guides/insert_guide_chapters.sql",
        &chapter_args.guide_ids,
        &chapter_args.parent_chapter_ids as &[Option<i64>],
        &chapter_args.titles,
        &chapter_args.source_files,
    )
    .fetch_all(&mut **tx)
    .await?;

    // TODO: Update parent_chapter_id for nested chapters using the parent_index mapping
    // For each chapter with parent_index, UPDATE SET parent_chapter_id = inserted_chapters[parent_index].id

    // Insert blocks for each chapter
    let mut chapter_models = Vec::new();
    for (ch_args, inserted_ch) in chapters.iter().zip(inserted_chapters.iter()) {
        let block_models = insert_blocks_for_chapter(
            tx,
            inserted_ch.id,
            &ch_args.blocks,
            directive_types,
            tag_lookup,
        ).await?;

        chapter_models.push(GuideChapterModel {
            id: inserted_ch.id,
            object_id: inserted_ch.object_id,
            guide_id: inserted_ch.guide_id,
            parent_chapter_id: inserted_ch.parent_chapter_id,
            parent_chapter_object_id: None,  // Filled in during SELECT
            title: inserted_ch.title.clone(),
            source_file: inserted_ch.source_file.clone(),
            created_at: inserted_ch.created_at,
            blocks: block_models,
        });
    }

    Ok(chapter_models)
}
```

---

## 5. Lookup Table Loading

The insert function needs to resolve text keys to internal IDs for directive types and tags:

```rust
use std::collections::HashMap;

async fn load_directive_type_lookup(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<HashMap<String, i16>, DataError> {
    let rows = sqlx::query_as::<_, (i16, String)>(
        "SELECT id, key FROM rev.guide_block_directive_types"
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows.into_iter().map(|(id, key)| (key, id)).collect())
}

async fn load_tag_lookup(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<HashMap<String, i16>, DataError> {
    let rows = sqlx::query_as::<_, (i16, String)>(
        "SELECT id, key FROM rev.guide_tags"
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows.into_iter().map(|(id, key)| (key, id)).collect())
}
```

**Note:** These are loaded once per insert transaction, not per guide. This is acceptable because they read from small lookup tables.

---

## 6. Verification Steps

1. `cargo build -p dapple-data` compiles with all SQL files (requires running migrations first)
2. `cargo sqlx prepare --workspace` generates correct offline metadata
3. Insert a guide with chapters and blocks, then SELECT it back and verify:
   - Guide-level fields match
   - Chapters are ordered by object_id
   - Blocks within chapters are ordered by object_id
   - Block tags are present as string arrays
4. PATCH an existing guide and verify version increments from 0.0.1 to 0.0.2
5. Deprecate a guide and verify the deprecation_level_key appears in SELECT results

---

## 7. Notes for Implementation Agent

- **The chapter parent_index resolution** is the trickiest part. The `CreateGuideChapterSchema` uses `parent_index: Option<usize>` (index into the chapters array). The insert function must resolve this to `parent_chapter_id` using the inserted chapter IDs. Consider inserting all chapters with `parent_chapter_id = NULL`, then doing a batch UPDATE for nested chapters.
- **Use `query_file_as!` macro** for all SQL files (not `query_as!` with inline SQL).
- **The SELECT queries use `$2 OFFSET $3`** (or `$1 OFFSET $2` for `select_guides.sql`). These correspond to `FilterOptions.sql_values()` which returns `(limit, offset)`.
- **The ARRAY_AGG with ARRAY_REMOVE** handles NULL tags. When a block has no tags, the LEFT JOIN produces NULL rows, and ARRAY_AGG would include NULL. ARRAY_REMOVE strips it.
- **Do not create separate query files for chapters or blocks.** They are owned dependents inserted within the guide's transaction. The only standalone SQL files are for guide-level operations.
