# Guide System Evolution Architecture Plan

**Status:** Draft
**Created:** 2026-02-14
**Last Updated:** 2026-02-14
**Target Version:** 0.3.0
**Dependencies:** Existing guide infrastructure (migrations 20260214000002-000007), dapple-parser crate, asset pipeline (drafts/chunks/assets)

---

## Executive Summary

The guide system today stores authored content as versioned entities with chapters, blocks, and tags, and provides CRUD endpoints for managing them. However, ingestion is still a manual JSON-assembly process: an operator must parse a guide directory, construct the `CreateGuideSetSchema` payload by hand, and POST it. The parser (`dapple-parser`) reads `book.toml` (an mdBook remnant) rather than the project's own `guide.toml` format, and has no `toml` dependency for proper parsing.

This plan defines five workstreams that close the gap between content authoring and runtime consumption:

1. **Parser Modernization** -- Drop `book.toml`, adopt `guide.toml` with proper TOML parsing, enrich `ParsedGuide` with full metadata.
2. **Tarball Ingest** -- A single `POST /api/guides/ingest` endpoint that accepts a `.tar.gz` guide directory, parses it, materializes all derived objects (playbooks, brief packs, tasks, inference profiles), stores assets, and creates the guide entity -- all atomically.
3. **Guide Assets** -- An associative table linking guides to assets with role classification, plus relative-path resolution during ingest.
4. **Promotional Cards** -- A first-class data structure for guide display cards extracted from `guide.toml`'s `[card]` section, stored per-version and queryable for UI carousels.
5. **PDF Rendering** -- Replace the mdbook-pandoc-xelatex pipeline with a pure-Rust PDF renderer (`oxidize-pdf` or similar), driven by named render profiles that filter blocks by audience tags and apply branded templates.

These workstreams transform guides from passive content storage into the system's primary content ingestion mechanism, where a single tarball upload produces a complete, runnable guide with all derived objects.

---

## Table of Contents

1. [Workstream Overview and Dependency Graph](#1-workstream-overview-and-dependency-graph)
2. [Workstream 1: Parser Modernization](#2-workstream-1-parser-modernization)
3. [Workstream 2: Tarball Ingest Endpoint](#3-workstream-2-tarball-ingest-endpoint)
4. [Workstream 3: Guide Assets](#4-workstream-3-guide-assets)
5. [Workstream 4: Promotional Cards](#5-workstream-4-promotional-cards)
6. [Workstream 5: PDF Rendering](#6-workstream-5-pdf-rendering)
7. [Cross-Cutting Concerns](#7-cross-cutting-concerns)
8. [Migration Ordering](#8-migration-ordering)
9. [Testing Strategy](#9-testing-strategy)
10. [AGENTS.md Updates](#10-agentsmd-updates)
11. [Open Questions and Risks](#11-open-questions-and-risks)
12. [Addendum: Fluent-Native Guide Structure](#12-addendum-fluent-native-guide-structure)
13. [Addendum: Shared Content Library](#13-addendum-shared-content-library)
14. [Addendum: Dapple SDK Crate & Code Generation](#14-addendum-dapple-sdk-crate--code-generation)

---

## 1. Workstream Overview and Dependency Graph

```
                    +-----------------------+
                    | 1. Parser             |
                    |    Modernization      |
                    +-----------+-----------+
                                |
               +----------------+----------------+
               |                                 |
   +-----------v-----------+       +-------------v-----------+
   | 3. Guide Assets       |       | 4. Promotional Cards    |
   |    (can start early)  |       |    (can start early)    |
   +-----------+-----------+       +-------------+-----------+
               |                                 |
               +----------------+----------------+
                                |
                    +-----------v-----------+
                    | 2. Tarball Ingest     |
                    |    (depends on 1,3,4) |
                    +-----------+-----------+
                                |
                    +-----------v-----------+
                    | 5. PDF Rendering      |
                    |    (independent but   |
                    |     benefits from all) |
                    +-----------------------+
```

**Execution order:**
- **Phase A:** Workstreams 1, 3, and 4 can proceed in parallel (no inter-dependencies).
- **Phase B:** Workstream 2 depends on all three being complete.
- **Phase C:** Workstream 5 can begin at any time but reaches full potential after Phase B.

---

## 2. Workstream 1: Parser Modernization

### Context

The parser currently:
- Reads `book.toml` for the guide title using manual line-by-line string splitting
- Has no `toml` crate dependency
- Produces a `ParsedGuide` with only a `title` field -- no description, locale, author, classification, or card metadata
- Looks for `SUMMARY.md` in `src/SUMMARY.md` first (mdBook convention), then guide root

### Changes

#### 2.1 Add `toml` dependency

```toml
# crates/dapple-parser/Cargo.toml
[dependencies]
toml = "0.8"
```

#### 2.2 New `guide_toml.rs` module

Create `crates/dapple-parser/src/guide_toml.rs` to handle `guide.toml` parsing.

```rust
use serde::{Deserialize, Serialize};

/// Parsed representation of guide.toml.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideToml {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub classification: Option<String>,
    #[serde(default)]
    pub card: Option<GuideCardToml>,
}

/// The [card] section of guide.toml.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideCardToml {
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub image: Option<String>,  // Relative path to image file
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub accent: Option<String>,
}

fn default_locale() -> String {
    "en".to_string()
}

/// Parse guide.toml from file contents.
pub fn parse_guide_toml(content: &str) -> Result<GuideToml, crate::ParseError> {
    toml::from_str(content).map_err(|e| crate::ParseError::InvalidGuideToml {
        detail: e.to_string(),
    })
}
```

#### 2.3 Extend `ParsedGuide`

```rust
// crates/dapple-parser/src/types.rs

/// Full metadata from guide.toml, available on the ParsedGuide.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideMetadata {
    pub title: String,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub card: Option<ParsedCard>,
}

/// Card metadata extracted from guide.toml [card] section.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedCard {
    pub subtitle: Option<String>,
    pub image: Option<String>,   // Relative path, not yet resolved
    pub category: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
}

/// The complete parsed representation of a guide directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedGuide {
    /// Guide title (from guide.toml, or fallback chain).
    pub title: String,
    /// Full metadata from guide.toml.
    pub metadata: GuideMetadata,
    /// Ordered list of chapters with their parsed blocks.
    pub chapters: Vec<ParsedChapter>,
    /// Non-markdown files found in the guide directory (relative paths).
    /// Populated during parse if asset discovery is enabled.
    pub asset_paths: Vec<String>,
}
```

#### 2.4 Update `parse_guide()` in `lib.rs`

The entry point changes to:

1. Look for `guide.toml` first (required). If absent, fall back to `book.toml` with a deprecation warning.
2. Parse `guide.toml` with the `toml` crate to produce `GuideMetadata`.
3. Look for `SUMMARY.md` at guide root first (guide convention), then `src/SUMMARY.md` (legacy).
4. Discover non-markdown files for the asset list.

```rust
pub fn parse_guide(guide_dir: &Path) -> Result<ParsedGuide, ParseError> {
    // 1. Parse guide.toml (preferred) or book.toml (fallback)
    let metadata = parse_guide_metadata(guide_dir)?;

    // 2. Read SUMMARY.md -- guide root first, then src/ (reverse of current)
    let summary_path = resolve_summary_path(guide_dir)?;
    let summary_content = std::fs::read_to_string(&summary_path)
        .map_err(|e| ParseError::Io { path: summary_path.display().to_string(), source: e })?;

    // 3. Parse chapters (unchanged logic)
    let chapter_stubs = summary::parse_summary(&summary_content)?;
    let base_dir = summary_path.parent().unwrap_or(guide_dir);
    let chapters = parse_chapter_files(base_dir, &chapter_stubs)?;

    // 4. Discover asset files (non-.md files in the directory tree)
    let asset_paths = discover_assets(guide_dir);

    Ok(ParsedGuide {
        title: metadata.title.clone(),
        metadata,
        chapters,
        asset_paths,
    })
}
```

#### 2.5 New `ParseError` variants

```rust
#[error("guide.toml not found in guide directory: {0}")]
MissingGuideToml(String),

#[error("Invalid guide.toml: {detail}")]
InvalidGuideToml { detail: String },
```

#### 2.6 Files to Create/Modify

| Action | File |
|--------|------|
| Create | `crates/dapple-parser/src/guide_toml.rs` |
| Modify | `crates/dapple-parser/src/lib.rs` -- new entry point logic |
| Modify | `crates/dapple-parser/src/types.rs` -- add `GuideMetadata`, `ParsedCard`, extend `ParsedGuide` |
| Modify | `crates/dapple-parser/src/error.rs` -- add new error variants |
| Modify | `crates/dapple-parser/Cargo.toml` -- add `toml = "0.8"` |

---

## 3. Workstream 2: Tarball Ingest Endpoint

### Context

Today, ingestion requires manually constructing JSON from parsed guide output. The tarball ingest endpoint replaces this with a single atomic upload.

### 3.1 API Contract

**Create new guide:**
```
POST /api/guides/ingest
Content-Type: multipart/form-data

Fields:
  file: <guide.tar.gz>        (required)
  dry_run: true|false          (optional, default false)
  source_path: "public/consumer" (optional, overrides directory-derived path)
```

**Create new version of existing guide:**
```
PATCH /api/guides/{path_object_id}/ingest
Content-Type: multipart/form-data

Fields:
  file: <guide.tar.gz>        (required)
  dry_run: true|false          (optional, default false)
```

**Response (success):**
```json
{
  "obj": {
    "object_id": "...",
    "path_object_id": "...",
    "title": "Essay Helper",
    "slug": "essay-helper",
    "version": "0.0.1",
    "chapters": [...],
    "card": { "subtitle": "...", "image_asset_object_id": "...", "category": "researcher" },
    "assets": [{ "object_id": "...", "role": "card_image", "mime_type": "image/jpeg" }],
    "derived_objects": {
      "playbook_object_id": "...",
      "brief_pack_object_ids": ["..."],
      "task_object_ids": ["..."],
      "inference_profile_object_ids": ["..."]
    }
  }
}
```

**Response (dry_run):**
```json
{
  "valid": true,
  "errors": [],
  "warnings": ["Chapter 'advanced.md' referenced in SUMMARY.md but file is empty"],
  "summary": {
    "title": "Essay Helper",
    "chapters": 6,
    "blocks": 24,
    "assets": 3,
    "derived_playbook_stages": 5,
    "derived_brief_packs": 1,
    "derived_tasks": 2,
    "derived_inference_profiles": 1
  }
}
```

### 3.2 Request Schemas

```rust
// crates/dapple-prompt-api/src/schema/guide_ingest.rs

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Query parameters for ingest endpoints.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct IngestQueryParams {
    /// When true, validate the guide without persisting anything.
    #[serde(default)]
    pub dry_run: bool,
    /// Override the source_path derived from the tarball directory structure.
    pub source_path: Option<String>,
}

/// Dry-run validation result.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct IngestValidationResult {
    pub valid: bool,
    pub errors: Vec<IngestError>,
    pub warnings: Vec<String>,
    pub summary: Option<IngestSummary>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct IngestError {
    pub location: String,  // e.g., "chapter:choosing-your-topic.md:line:7"
    pub message: String,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct IngestSummary {
    pub title: String,
    pub chapters: usize,
    pub blocks: usize,
    pub assets: usize,
    pub derived_playbook_stages: usize,
    pub derived_brief_packs: usize,
    pub derived_tasks: usize,
    pub derived_inference_profiles: usize,
}

/// Ingest response wrapping the created guide plus derived object references.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct IngestGuideResponse {
    pub guide: Guide,
    pub card: Option<GuideCard>,
    pub assets: Vec<GuideAssetRef>,
    pub derived_objects: DerivedObjectRefs,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct DerivedObjectRefs {
    pub playbook_object_id: Option<Uuid>,
    pub brief_pack_object_ids: Vec<Uuid>,
    pub task_object_ids: Vec<Uuid>,
    pub inference_profile_object_ids: Vec<Uuid>,
}
```

### 3.3 Transform Layer Architecture

The transform layer is a new module that converts `ParsedGuide` AST nodes into domain `Create*Schema` payloads. It lives in a new crate or module: `crates/dapple-parser/src/transform/` (preferred since it only depends on types, not on the database).

```
ParsedGuide
    |
    v
transform::plan_guide(parsed, guide_dir) -> IngestPlan
    |
    v
IngestPlan {
    guide_args: InsertGuideWithChildrenArgs,
    card: Option<CardArgs>,
    asset_files: Vec<AssetFileEntry>,        // path + mime_type + bytes
    derived_brief_packs: Vec<BriefPackDef>,   // from :::brief-pack blocks
    derived_playbook: Option<PlaybookDef>,     // from :::stage/:::step hierarchy
    derived_tasks: Vec<TaskDef>,              // from :::nushell blocks
    derived_inference_profiles: Vec<InferenceProfileDef>,
    derived_refs: Vec<RefDef>,                // from :::ref blocks
}
```

**Directive-to-domain mapping:**

| Directive | Transform Action |
|-----------|-----------------|
| `prose` | Stored as `GuideBlock` only, no derived object |
| `stage` | Collects into `PlaybookDef.stages[]` with nested steps |
| `step` | Collects into parent stage's `PlaybookDef.stages[n].steps[]` |
| `nushell` (role: default) | Creates `TaskDef` with nushell script content |
| `nushell` (role: directive) | Attaches flow control to the step/stage named by `for` attribute |
| `brief_pack` | Creates `BriefPackDef` from structured content |
| `inference_profile` | Creates `InferenceProfileDef` from attributes |
| `ref` | Creates `RefDef` for cross-reference resolution |

**Intermediate definition types** (not database models -- pure data):

```rust
// crates/dapple-parser/src/transform/mod.rs

pub struct PlaybookDef {
    pub name: String,        // From guide title or explicit attribute
    pub stages: Vec<StageDef>,
}

pub struct StageDef {
    pub name: String,
    pub description: String,
    pub flow_control: Option<serde_json::Value>,  // Expanded JSONLogic
    pub steps: Vec<StepDef>,
    pub source_block_index: BlockIndex,  // For provenance tracking
}

pub struct StepDef {
    pub name: String,
    pub description: String,
    pub flow_control: Option<serde_json::Value>,
    pub task_ref: Option<String>,  // Name of :::nushell block to link
    pub source_block_index: BlockIndex,
}

pub struct TaskDef {
    pub name: String,
    pub script: String,
    pub source_block_index: BlockIndex,
}

pub struct BriefPackDef {
    pub name: String,
    pub content: String,       // Raw content for materialization
    pub source_block_index: BlockIndex,
}

pub struct InferenceProfileDef {
    pub name: String,
    pub attributes: serde_json::Value,
    pub source_block_index: BlockIndex,
}

pub struct RefDef {
    pub slug: String,
    pub ref_type: String,  // "guide", "playbook", etc.
    pub source_block_index: BlockIndex,
}

pub struct AssetFileEntry {
    pub relative_path: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// Location within the parsed guide for provenance.
pub struct BlockIndex {
    pub chapter_index: usize,
    pub block_index: usize,
}
```

### 3.4 Ingest Handler Flow

The handler orchestrates the full pipeline within a single database transaction:

```rust
// crates/dapple-prompt-api/src/handler/guide_ingest.rs (pseudocode)

pub async fn guide_ingest_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Query(params): Query<IngestQueryParams>,
    mut multipart: Multipart,
) -> impl IntoApiResponse {
    // 1. Extract tarball from multipart
    let tarball_bytes = extract_tarball_field(&mut multipart).await?;

    // 2. Decompress to temp directory
    let temp_dir = decompress_tarball(&tarball_bytes)?;

    // 3. Parse the guide directory
    let parsed = dapple_parser::parse_guide(temp_dir.path())?;

    // 4. Plan the ingest (transform ParsedGuide -> IngestPlan)
    let plan = transform::plan_guide(&parsed, temp_dir.path())?;

    // 5. If dry_run, return validation summary
    if params.dry_run {
        return Ok(Json(plan.to_validation_result()));
    }

    // 6. Begin transaction
    let mut tx = data.db.get_pool().begin().await?;

    // 7. Upload assets via draft->chunk->asset pipeline (within tx)
    let asset_map = ingest_assets(&mut tx, &plan.asset_files).await?;
    // asset_map: HashMap<relative_path, asset_object_id>

    // 8. Materialize derived objects (within tx)
    //    a. Tasks from :::nushell blocks
    let task_map = materialize_tasks(&mut tx, &plan.derived_tasks).await?;
    //    b. Brief packs from :::brief-pack blocks
    let bp_map = materialize_brief_packs(&mut tx, &plan.derived_brief_packs).await?;
    //    c. Inference profiles from :::inference-profile blocks
    let ip_map = materialize_inference_profiles(&mut tx, &plan.derived_inference_profiles).await?;
    //    d. Playbook from :::stage/:::step hierarchy
    let playbook = materialize_playbook(&mut tx, &plan.derived_playbook, &task_map).await?;

    // 9. Create guide entity with chapters, blocks, tags
    let guide_args = plan.guide_args;
    let guide = insert_guide_within_tx(&mut tx, guide_args).await?;

    // 10. Create guide_assets associations
    insert_guide_assets(&mut tx, guide.id, &asset_map, &plan).await?;

    // 11. Create guide_cards entry
    let card = if let Some(card_args) = plan.card {
        let card_image_id = card_args.image.as_ref()
            .and_then(|path| asset_map.get(path));
        Some(insert_guide_card(&mut tx, guide.id, &card_args, card_image_id).await?)
    } else {
        None
    };

    // 12. Create guide_derived_locks for all derived objects
    insert_derived_locks(&mut tx, guide.id, &task_map, &bp_map, &ip_map, &playbook).await?;

    // 13. Commit
    tx.commit().await?;

    // 14. Return response
    Ok(Json(IngestGuideResponse { guide, card, assets, derived_objects }))
}
```

### 3.5 Files to Create/Modify

| Action | File |
|--------|------|
| Create | `crates/dapple-parser/src/transform/mod.rs` -- IngestPlan, definition types |
| Create | `crates/dapple-parser/src/transform/playbook.rs` -- stage/step extraction |
| Create | `crates/dapple-parser/src/transform/brief_pack.rs` -- brief pack extraction |
| Create | `crates/dapple-parser/src/transform/task.rs` -- nushell task extraction |
| Create | `crates/dapple-parser/src/transform/asset.rs` -- asset discovery and classification |
| Create | `crates/dapple-parser/src/transform/card.rs` -- card metadata extraction |
| Create | `crates/dapple-prompt-api/src/handler/guide_ingest.rs` -- ingest handler |
| Create | `crates/dapple-prompt-api/src/schema/guide_ingest.rs` -- request/response schemas |
| Modify | `crates/dapple-prompt-api/src/route/guides.rs` -- add ingest routes |
| Modify | `crates/dapple-prompt-api/Cargo.toml` -- add `tar`, `flate2` dependencies |

---

## 4. Workstream 3: Guide Assets

### Context

Guides contain non-markdown files (images, PDFs, etc.) that live alongside chapter files in the guide directory. The existing asset pipeline (draft -> chunk -> finalize) handles binary storage, but there is no link between guides and their assets.

### 4.1 Database Schema

```sql
-- Migration: 20260215000001_guide_assets.sql

-- Associative table linking guide versions to their assets.
-- Each row represents one asset used by a specific guide version.
-- The role column classifies the asset's purpose within the guide.
CREATE TABLE rev.guide_assets (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    guide_id BIGINT NOT NULL REFERENCES rev.guides (id),
    asset_id BIGINT NOT NULL REFERENCES rev.assets (id),
    role TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, asset_id)
);

CREATE INDEX idx_guide_assets_guide_id ON rev.guide_assets (guide_id);
CREATE INDEX idx_guide_assets_asset_id ON rev.guide_assets (asset_id);
CREATE INDEX idx_guide_assets_role ON rev.guide_assets (role);
```

**Role values (convention, not enforced by lookup table):**

| Role | Usage |
|------|-------|
| `card_image` | The image used in the guide's promotional card |
| `chapter_illustration` | An image embedded in a chapter |
| `attachment` | A downloadable file referenced in content |
| `cover` | PDF cover image |

The `relative_path` column stores the original path within the guide directory (e.g., `assets/images/card.jpg`) to support path-based resolution during future re-ingestion and to provide human-readable context.

### 4.2 Object Model

**DTO addition to Guide:**

```rust
// Added to the Guide DTO
pub struct GuideAssetRef {
    pub object_id: Uuid,          // guide_assets.object_id
    pub asset_object_id: Uuid,    // From assets table
    pub asset_path_object_id: Uuid,
    pub role: String,
    pub relative_path: String,
    pub mime_type: String,
    pub content_length: i64,
}
```

The `Guide` DTO gains a new field:
```rust
pub struct Guide {
    // ... existing fields ...
    pub assets: Vec<GuideAssetRef>,
}
```

### 4.3 Internal Models

```rust
// crates/dapple-data/src/model/guides.rs (additions)

pub struct InsertedGuideAssetModel {
    pub id: i64,
    pub object_id: Uuid,
    pub guide_id: i64,
    pub asset_id: i64,
    pub role: String,
    pub relative_path: String,
    pub created_at: OffsetDateTime,
}

pub struct InsertGuideAssetArgs {
    pub guide_ids: Vec<i64>,
    pub asset_ids: Vec<i64>,
    pub roles: Vec<String>,
    pub relative_paths: Vec<String>,
}
```

### 4.4 SQL Queries

**Insert:**
```sql
-- assets/sql/guides/insert_guide_assets.sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::BIGINT[],   -- guide_id[]
        $2::BIGINT[],   -- asset_id[]
        $3::TEXT[],      -- role[]
        $4::TEXT[]       -- relative_path[]
    ) WITH ORDINALITY AS o(guide_id, asset_id, role, relative_path, ord)
),
objs AS (
    INSERT INTO rev.guide_assets (guide_id, asset_id, role, relative_path)
    SELECT args.guide_id, args.asset_id, args.role, args.relative_path
    FROM args
    ORDER BY args.ord
    RETURNING id, object_id, guide_id, asset_id, role, relative_path, created_at
)
SELECT * FROM objs ORDER BY objs.id;
```

**Select (joined into FlatGuideModel):**

The existing `select_guides_by_object_ids.sql` and other SELECT queries need an additional LEFT JOIN to include guide assets. This adds `guide_asset_*` columns to the `FlatGuideModel`.

Alternatively (and preferably to avoid further widening the already-wide flat model), guide assets can be fetched in a separate query within the same transaction, since they are a relatively small associative set. The fold logic would attach them after the main guide fold.

### 4.5 Asset Ingestion During Tarball Processing

During tarball ingest, non-markdown files are processed through the existing draft pipeline:

1. Walk the extracted directory, filtering out `.md`, `.toml`, `SUMMARY.md`, and hidden files.
2. For each file, determine MIME type from extension.
3. Create a draft, insert chunks, finalize to asset.
4. SHA-256 dedup ensures identical files across guides share storage.
5. Build a `HashMap<relative_path, asset_id>` for downstream resolution.
6. After guide insertion, insert `guide_assets` rows linking the guide to each asset with the appropriate role.

**Role assignment logic:**
- If the path matches the card image path from `guide.toml [card].image`, assign `card_image`.
- If the file is referenced in a block's `attributes.image` or content markdown image syntax, assign `chapter_illustration`.
- Otherwise, assign `attachment`.

### 4.6 Files to Create/Modify

| Action | File |
|--------|------|
| Create | Migration `20260215000001_guide_assets.sql` |
| Create | `assets/sql/guides/insert_guide_assets.sql` |
| Create | `assets/sql/guides/select_guide_assets_by_guide_ids.sql` |
| Modify | `crates/dapple-data/src/model/guides.rs` -- add `GuideAssetRef`, insert models |
| Modify | `crates/dapple-data/src/query/guides.rs` -- add asset insert/select functions |
| Modify | `crates/dapple-prompt-api/src/schema/guides.rs` -- add assets to DTO |

---

## 5. Workstream 4: Promotional Cards

### Context

Guide display cards are currently embedded in `:::brief-pack` content as a "Display Card" section. This is fragile (requires content parsing), invisible to the API, and unsearchable. Cards should be first-class data.

### 5.1 `guide.toml` `[card]` Section

The `guide.toml` format gains a `[card]` section:

```toml
title = "Essay Helper"
description = "Ready to write an essay? We'll help you develop the fundamentals"
locale = "en"
author = "Dapple Team"
classification = "workflow"

[card]
subtitle = "Ready to write an essay? We'll help you develop the fundamentals"
image = "assets/images/card.jpg"
category = "researcher"
color = "#2D6A4F"
accent = "#95D5B2"
```

The parser extracts this into `ParsedCard` (see Workstream 1).

### 5.2 Database Schema

Cards are **per-version** because they ship with `guide.toml` in each version's tarball. The UI shows the latest version's card.

```sql
-- Migration: 20260215000002_guide_cards.sql

CREATE TABLE rev.guide_cards (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    guide_id BIGINT NOT NULL REFERENCES rev.guides (id) UNIQUE,
    subtitle TEXT,
    card_image_asset_id BIGINT REFERENCES rev.assets (id),
    category TEXT,
    color TEXT,
    accent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_guide_cards_guide_id ON rev.guide_cards (guide_id);
CREATE INDEX idx_guide_cards_category ON rev.guide_cards (category);
CREATE INDEX idx_guide_cards_card_image_asset_id ON rev.guide_cards (card_image_asset_id);
```

The `UNIQUE (guide_id)` constraint ensures one card per guide version. The `card_image_asset_id` FK references the asset created during ingest from the `[card].image` path.

### 5.3 Object Model

**DTO:**

```rust
/// Public DTO for guide card (API response).
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideCard {
    pub object_id: Uuid,
    pub subtitle: Option<String>,
    pub card_image_asset_object_id: Option<Uuid>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
    pub created_at: DateTime,
}
```

**Internal Models:**

```rust
pub struct InsertedGuideCardModel {
    pub id: i64,
    pub object_id: Uuid,
    pub guide_id: i64,
    pub subtitle: Option<String>,
    pub card_image_asset_id: Option<i64>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
    pub created_at: OffsetDateTime,
}

pub struct GuideCardModel {
    pub id: i64,
    pub object_id: Uuid,
    pub guide_id: i64,
    pub subtitle: Option<String>,
    pub card_image_asset_id: Option<i64>,
    pub card_image_asset_object_id: Option<Uuid>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
    pub created_at: OffsetDateTime,
}

pub struct InsertGuideCardArgs {
    pub guide_ids: Vec<i64>,
    pub subtitles: Vec<Option<String>>,
    pub card_image_asset_ids: Vec<Option<i64>>,
    pub categories: Vec<Option<String>>,
    pub colors: Vec<Option<String>>,
    pub accents: Vec<Option<String>>,
}
```

### 5.4 Guide DTO Extension

The `Guide` DTO gains an optional card:

```rust
pub struct Guide {
    // ... existing fields ...
    pub card: Option<GuideCard>,
    pub assets: Vec<GuideAssetRef>,
}
```

### 5.5 SQL Queries

**Insert:**
```sql
-- assets/sql/guides/insert_guide_cards.sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::BIGINT[],   -- guide_id[]
        $2::TEXT[],      -- subtitle[]
        $3::BIGINT[],   -- card_image_asset_id[]
        $4::TEXT[],      -- category[]
        $5::TEXT[],      -- color[]
        $6::TEXT[]       -- accent[]
    ) WITH ORDINALITY AS o(guide_id, subtitle, card_image_asset_id, category, color, accent, ord)
),
objs AS (
    INSERT INTO rev.guide_cards (guide_id, subtitle, card_image_asset_id, category, color, accent)
    SELECT args.guide_id, args.subtitle, args.card_image_asset_id, args.category, args.color, args.accent
    FROM args
    ORDER BY args.ord
    RETURNING id, object_id, guide_id, subtitle, card_image_asset_id, category, color, accent, created_at
)
SELECT * FROM objs ORDER BY objs.id;
```

**Select (for card carousel):**
```sql
-- assets/sql/guides/select_guide_cards.sql
-- Returns the latest version's card for each guide path.
-- Used by the UI to populate the card carousel.
WITH latest_guides AS (
    SELECT DISTINCT ON (op.id)
        g.id AS guide_id,
        g.object_id AS guide_object_id,
        op.object_id AS path_object_id,
        gpd.title,
        gpd.slug,
        vo.version_major,
        vo.version_minor,
        vo.version_patch
    FROM rev.guides g
    JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = vo.deprecation_level_id
    WHERE dl.id IS NULL  -- Not deprecated
    ORDER BY op.id, vo.version_major DESC, vo.version_minor DESC, vo.version_patch DESC
)
SELECT
    gc.id,
    gc.object_id,
    gc.guide_id,
    lg.guide_object_id,
    lg.path_object_id,
    lg.title,
    lg.slug,
    gc.subtitle,
    gc.card_image_asset_id,
    a_path.object_id AS "card_image_asset_object_id?",
    gc.category,
    gc.color,
    gc.accent,
    gc.created_at
FROM latest_guides lg
JOIN rev.guide_cards gc ON gc.guide_id = lg.guide_id
LEFT JOIN rev.assets a ON a.id = gc.card_image_asset_id
LEFT JOIN rev.versioned_objects a_vo ON a_vo.id = a.versioned_object_id
LEFT JOIN rev.object_paths a_path ON a_path.id = a_vo.path_id
ORDER BY gc.category, lg.title
LIMIT $1 OFFSET $2;
```

### 5.6 New API Endpoint for Card Carousel

```
GET /api/guides/cards?query=all
GET /api/guides/cards?query=by_category&category=researcher
```

This is a read-only endpoint. Cards are created as part of guide ingest, not independently.

```rust
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetGuideCardSetSchema {
    All { options: Option<FilterOptions> },
    ByCategory { category: String, options: Option<FilterOptions> },
}
```

### 5.7 Files to Create/Modify

| Action | File |
|--------|------|
| Create | Migration `20260215000002_guide_cards.sql` |
| Create | `assets/sql/guides/insert_guide_cards.sql` |
| Create | `assets/sql/guides/select_guide_cards.sql` |
| Create | `assets/sql/guides/select_guide_cards_by_category.sql` |
| Modify | `crates/dapple-data/src/model/guides.rs` -- add card models |
| Modify | `crates/dapple-data/src/query/guides.rs` -- add card query functions |
| Modify | `crates/dapple-prompt-api/src/schema/guides.rs` -- add card schemas |
| Modify | `crates/dapple-prompt-api/src/handler/guides.rs` -- add card index handler |
| Modify | `crates/dapple-prompt-api/src/route/guides.rs` -- add `/cards` route |

---

## 6. Workstream 5: PDF Rendering

### Context

The current PDF pipeline requires a Docker image with pandoc, texlive-xetex, and mdbook-pandoc. It runs at Docker build time, producing static PDFs. This is slow, inflexible (no audience filtering), and cannot generate PDFs on demand.

The replacement uses a pure-Rust PDF library to render guides server-side, filtering blocks by audience tags and applying branded templates.

### 6.1 Render Profiles

A render profile defines which blocks to include and how to style the output.

```sql
-- Migration: 20260215000003_render_profiles.sql

CREATE TABLE rev.render_profiles (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT,
    tag_filter JSONB NOT NULL DEFAULT '[]'::jsonb,
    style_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed profiles
INSERT INTO rev.render_profiles (key, description, tag_filter, style_config) VALUES
    ('consumer', 'Polished consumer-facing PDF', '["universal", "product"]',
     '{"cover_page": true, "branding": "consumer", "watermark": null}'),
    ('agent', 'Technical agent-facing PDF', '["universal", "agent"]',
     '{"cover_page": true, "branding": "technical", "watermark": null}'),
    ('internal', 'Internal PDF with all content', '["universal", "agent", "product", "coder", "auditor", "planner"]',
     '{"cover_page": true, "branding": "internal", "watermark": "INTERNAL"}');
```

The `tag_filter` is a JSON array of tag keys. A block is included in the rendered output if:
- The block has no tags (universal by convention), OR
- Any of the block's tags appear in the profile's `tag_filter` array

### 6.2 API Endpoint

```
POST /api/guides/{guide_object_id}/render
Content-Type: application/json

{
  "profile": "consumer"
}
```

Response: `200 OK` with `Content-Type: application/pdf` body, or the asset object_id of the stored PDF.

Alternative (async):
```
POST /api/guides/{guide_object_id}/render
  -> 202 Accepted { "render_job_id": "..." }

GET /api/guides/renders/{render_job_id}
  -> 200 OK { "status": "complete", "asset_object_id": "..." }
```

For the initial implementation, synchronous rendering is simpler. PDFs for typical guides (5-10 chapters) should render in under 2 seconds with a pure-Rust renderer.

### 6.3 Render Pipeline

```
1. Load guide from DB (full chapter/block/tag hierarchy)
2. Load render profile (tag_filter, style_config)
3. Filter blocks: keep blocks where (tags is empty) OR (tags intersect tag_filter)
4. Load card metadata (for cover page)
5. Load card image asset bytes (for cover)
6. Render PDF:
   a. Cover page (title, subtitle, card image, version)
   b. Table of contents
   c. For each chapter:
      - Chapter heading
      - For each included block:
        - Render markdown content to styled PDF elements
   d. Footer with page numbers
7. Return PDF bytes or store as asset
```

### 6.4 New Crate: `dapple-render`

```
crates/dapple-render/
  Cargo.toml
  src/
    lib.rs          -- public API: render_guide_pdf(guide, profile, card, assets)
    filter.rs       -- Block filtering by tag set
    layout.rs       -- Page layout, margins, header/footer
    markdown.rs     -- Markdown-to-PDF element conversion
    cover.rs        -- Cover page rendering
    toc.rs          -- Table of contents generation
    style.rs        -- Style profiles (fonts, colors, spacing)
    image.rs        -- Image embedding (from asset bytes)
```

Dependencies:
```toml
[dependencies]
# PDF generation -- evaluate options:
# Option A: printpdf (low-level, full control)
# Option B: genpdf (higher-level, markdown-friendly)
# Option C: oxidize-pdf (if available and mature)
genpdf = "0.3"        # Likely best fit for structured document rendering
image = "0.25"        # Image decoding for embedded assets
pulldown-cmark = "0.11"  # Markdown parsing (already used in parser)
```

The choice of PDF library should be evaluated during implementation. `genpdf` wraps `printpdf` and provides paragraph layout, font embedding, and basic styling -- a good fit for guide rendering.

### 6.5 Rendered PDF Storage

Rendered PDFs are stored as assets via the existing pipeline:
- `asset_kind`: `document`
- `mime_type`: `application/pdf`
- Linked back to the guide version via a new associative table or by convention in `guide_assets` with `role = "rendered_pdf:{profile_key}"`.

A cache-invalidation strategy is needed: when a new guide version is ingested, any rendered PDFs for the previous version become stale. The render endpoint should check if a cached PDF exists for the exact `(guide_object_id, profile_key)` combination before re-rendering.

```sql
-- Optional: Render cache table
CREATE TABLE rev.guide_renders (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    guide_id BIGINT NOT NULL REFERENCES rev.guides (id),
    render_profile_id SMALLINT NOT NULL REFERENCES rev.render_profiles (id),
    asset_id BIGINT NOT NULL REFERENCES rev.assets (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, render_profile_id)
);

CREATE INDEX idx_guide_renders_guide_id ON rev.guide_renders (guide_id);
```

### 6.6 Files to Create/Modify

| Action | File |
|--------|------|
| Create | New crate `crates/dapple-render/` |
| Create | Migration `20260215000003_render_profiles.sql` |
| Create | Migration `20260215000004_guide_renders.sql` (optional cache table) |
| Create | `crates/dapple-prompt-api/src/handler/guide_render.rs` |
| Create | `crates/dapple-prompt-api/src/schema/guide_render.rs` |
| Modify | `crates/dapple-prompt-api/src/route/guides.rs` -- add render route |
| Modify | `Cargo.toml` (workspace) -- add `dapple-render` member |

---

## 7. Cross-Cutting Concerns

### 7.1 Transaction Boundaries

The tarball ingest endpoint is the most critical transaction. It must:
- Create all assets (draft -> chunk -> finalize) within the transaction
- Create all derived objects (playbook, tasks, brief packs, inference profiles) within the transaction
- Create the guide with all chapters, blocks, and tags within the transaction
- Create guide_assets associations within the transaction
- Create guide_cards within the transaction
- Create guide_derived_locks within the transaction

If any step fails, the entire transaction rolls back. This means:
- Draft finalization must work within an existing transaction (verify the current `finalize_draft` function supports `&mut Transaction` instead of `&DbPool`)
- All derived object insert functions must accept `&mut Transaction`

**Implementation note:** The current `insert_guides` function in `query/guides.rs` already uses a transaction internally (`let mut tx = p.begin().await?`). The ingest handler should pass its own outer transaction down, or the query functions should be refactored to accept either a pool or a transaction (using SQLx's `Executor` trait).

### 7.2 Error Handling

Ingest errors fall into categories:

| Category | HTTP Status | Recovery |
|----------|-------------|----------|
| Tarball corrupt/unreadable | 400 | Client retries with corrected file |
| guide.toml missing/invalid | 422 | Client fixes guide source |
| SUMMARY.md missing | 422 | Client fixes guide source |
| Unknown directive type | 422 | Client fixes guide source or new migration needed |
| Unknown tag key | 422 | New migration needed to add tag |
| Asset upload failure | 500 | Server-side retry |
| Derived object conflict | 409 | Existing derived objects from a different guide |
| Database error | 500 | Server-side retry |

The dry-run mode should catch all 422-class errors before any persistence occurs.

### 7.3 Validation

Before any database writes, validate:

1. `guide.toml` parses correctly and has required `title` field
2. `SUMMARY.md` exists and lists at least one chapter
3. All chapter files referenced in `SUMMARY.md` exist
4. All `:::` fences are properly closed
5. All directive types match keys in `rev.guide_block_directive_types`
6. All tag keys match keys in `rev.guide_tags`
7. All `:::nushell { role: "directive", for: "..." }` blocks have matching step/stage names
8. No step/stage has both declarative flow attributes and a directive nushell block
9. Card image path (if specified) exists in the tarball
10. No circular chapter parent references

### 7.4 Symlink Handling in Tarballs

The dapple-guides repo uses symlinks for shared assets. When creating tarballs:
- Tarballs should be created with `--dereference` (follow symlinks) so the tarball contains actual file data
- The ingest endpoint should reject symlinks within the extracted tarball as a security measure
- Documentation should specify the `tar czh -f guide.tar.gz guide-dir/` command

### 7.5 Backward Compatibility

The existing `POST /api/guides/` endpoint remains unchanged. It continues to accept `CreateGuideSetSchema` JSON payloads for programmatic guide creation. The ingest endpoint is a higher-level alternative that handles parsing and asset management automatically.

The `Guide` DTO gains two new optional fields (`card` and `assets`). These are backward-compatible additions -- existing API consumers will simply see `null`/`[]` for guides created before these features.

---

## 8. Migration Ordering

All migrations use the `20260215` date prefix to sequence after the existing guide tables.

| Order | Migration File | Depends On |
|-------|---------------|------------|
| 1 | `20260215000001_guide_assets.sql` | `rev.guides`, `rev.assets` |
| 2 | `20260215000002_guide_cards.sql` | `rev.guides`, `rev.assets` |
| 3 | `20260215000003_render_profiles.sql` | (standalone) |
| 4 | `20260215000004_guide_renders.sql` | `rev.guides`, `rev.render_profiles`, `rev.assets` |

All four migrations are independent of each other (except #4 depends on #3). They can be applied in any order, but the numbering provides a logical sequence.

---

## 9. Testing Strategy

### 9.1 Parser Tests (Unit)

Located in `crates/dapple-parser/src/` alongside existing tests.

```rust
#[test]
fn test_parse_guide_toml_full() {
    let toml = r#"
title = "Test Guide"
description = "A test"
locale = "en"
author = "Test Author"
classification = "workflow"

[card]
subtitle = "Test subtitle"
image = "assets/images/card.jpg"
category = "researcher"
"#;
    let result = parse_guide_toml(toml).unwrap();
    assert_eq!(result.title, "Test Guide");
    assert!(result.card.is_some());
    assert_eq!(result.card.unwrap().category, Some("researcher".to_string()));
}

#[test]
fn test_parse_guide_toml_minimal() {
    let toml = r#"title = "Minimal Guide""#;
    let result = parse_guide_toml(toml).unwrap();
    assert_eq!(result.locale, "en");  // Default
    assert!(result.card.is_none());
}

#[test]
fn test_parse_guide_with_toml_metadata() {
    // Create temp dir with guide.toml + SUMMARY.md + chapter
    // Verify ParsedGuide.metadata is populated
}
```

### 9.2 Transform Tests (Unit)

Located in `crates/dapple-parser/src/transform/`.

```rust
#[test]
fn test_plan_guide_extracts_playbook() {
    // ParsedGuide with :::stage and :::step blocks
    // Verify IngestPlan.derived_playbook has correct stage/step hierarchy
}

#[test]
fn test_plan_guide_extracts_tasks() {
    // ParsedGuide with :::nushell blocks
    // Verify IngestPlan.derived_tasks has correct names and scripts
}

#[test]
fn test_plan_guide_links_nushell_directive_to_step() {
    // :::nushell { role: "directive", for: "Step Name" }
    // Verify the flow_control field is set on the matching StepDef
}
```

### 9.3 Integration Tests

**Ingest lifecycle trial (in `tests/user_lifecycle.rs`):**

```rust
fn trial_ingest_guide() -> Trial {
    // 1. Create a tarball from test guide directory
    // 2. POST /api/guides/ingest with the tarball
    // 3. Verify guide was created with correct metadata
    // 4. Verify chapters and blocks match expected counts
    // 5. Verify assets were created and linked
    // 6. Verify card was created with correct fields
    // 7. Register "init-guides-ingested" batch
}

fn trial_ingest_guide_version() -> Trial {
    // Wait for: init-guides-ingested
    // 1. Modify the test guide (change description)
    // 2. PATCH /api/guides/{path_id}/ingest
    // 3. Verify new version (0.0.2)
    // 4. Verify old version still exists
}

fn trial_ingest_dry_run() -> Trial {
    // 1. POST /api/guides/ingest?dry_run=true
    // 2. Verify validation result (no DB writes)
    // 3. Verify summary counts are correct
}

fn trial_guide_cards_carousel() -> Trial {
    // Wait for: init-guides-ingested
    // 1. GET /api/guides/cards?query=all
    // 2. Verify card data matches guide.toml [card] section
}
```

**Test consumer for ingest:**

The ingest endpoint uses `multipart/form-data`, which is different from the standard JSON endpoint pattern. The test consumer for ingest will need a custom implementation rather than using the generic `Endpoint` trait.

```rust
// tests/consumer/guide_ingest.rs

pub struct GuideIngestConsumer {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl GuideIngestConsumer {
    pub async fn ingest_tarball(
        &self,
        tarball: Vec<u8>,
        dry_run: bool,
    ) -> Result<IngestGuideResponse, ConsumerError> {
        let client = reqwest::Client::new();
        let form = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(tarball)
                .file_name("guide.tar.gz")
                .mime_str("application/gzip")
                .unwrap());

        let url = format!("{}api/guides/ingest?dry_run={}", self.base_url, dry_run);
        let resp = client.post(&url).multipart(form).send().await?;
        // ...
    }
}
```

### 9.4 Test Fixture Guide

Create a minimal test guide directory that can be tarballed for integration tests:

```
tests/fixtures/test-guide/
  guide.toml
  SUMMARY.md
  introduction.md       (with :::brief-pack block)
  getting-started.md    (with :::stage and :::step blocks)
  assets/
    images/
      card.jpg          (small test image)
```

This fixture is checked into the test directory and tarballed at test time.

### 9.5 PDF Render Tests

```rust
#[test]
fn test_filter_blocks_by_profile() {
    // Given blocks with various tags
    // When filtered with "consumer" profile (universal + product)
    // Then only matching blocks are included
}

#[test]
fn test_render_minimal_pdf() {
    // Given a guide with one chapter and two blocks
    // When rendered with consumer profile
    // Then PDF bytes are non-empty and valid
}
```

---

## 10. AGENTS.md Updates

Add the following section to `AGENTS.md`:

```markdown
## Guide Ingest Pipeline

When implementing features related to guide ingestion:

1. **Tarball is the primary ingest mechanism** -- `POST /api/guides/ingest` accepts a `.tar.gz`
   containing a complete guide directory with `guide.toml`, `SUMMARY.md`, chapters, and assets.
   The JSON `POST /api/guides/` endpoint remains available for programmatic creation but is
   not the primary path.

2. **guide.toml is authoritative** -- The parser reads `guide.toml` (not `book.toml`) for
   metadata. The `[card]` section defines promotional card data. The `title` field is required.

3. **Transform layer is pure** -- The `dapple-parser::transform` module converts `ParsedGuide`
   AST into `IngestPlan` without database access. It produces definition types (`PlaybookDef`,
   `TaskDef`, etc.) that the handler then materializes.

4. **Ingest is atomic** -- The entire ingest operation (assets, derived objects, guide, cards,
   associations) runs in a single database transaction. Any failure rolls back everything.

5. **Asset paths are resolved during ingest** -- Relative paths in directive attributes
   (e.g., `image: "assets/images/card.jpg"`) are resolved to asset `object_id` references by
   matching against the `HashMap<relative_path, asset_id>` built during asset upload.

6. **Cards are per-version** -- Each guide version can have its own card. The card carousel
   endpoint returns the latest non-deprecated version's card for each guide path.

7. **Render profiles filter by tags** -- PDF rendering uses named profiles that define a
   tag filter set. Blocks with no tags (universal) are always included.

See `docs/plans/guide-evolution.md` for full architecture details.
```

---

## 11. Open Questions and Risks

### Open Questions

1. **Flow control expansion timing** -- Should the parser expand declarative flow attributes
   (`retry_on`, `skip_when`, `require`) to JSONLogic at parse time, or should the transform
   layer handle this? The dapple-guides CLAUDE.md specifies parse-time expansion, but the
   parser currently has no expansion logic. The transform layer seems like the right place
   since it already needs to understand the semantic structure.

2. **Brief pack materialization depth** -- The `:::brief-pack` block contains semi-structured
   content (markdown with sections like "## Overview", "## Display Card"). How much of this
   should be parsed into structured BriefPack/BriefGroup/Brief entities vs. stored as raw
   content? Initial recommendation: store the raw content in the guide block, and defer
   structured materialization to a future workstream. The card data moves to `guide.toml [card]`.

3. **Cross-guide references** -- `:::ref` blocks reference objects by slug. During ingest,
   should we validate that the referenced slug exists? If the referenced guide has not been
   ingested yet, this would fail. Consider deferred validation or a separate linking pass.

4. **Tarball size limits** -- What is the maximum acceptable tarball size? Guides with many
   images could be several MB. A 50MB limit seems reasonable for initial implementation.
   Larger guides should use the draft/chunk pipeline for individual assets.

5. **PDF library selection** -- `genpdf`, `printpdf`, or `oxidize-pdf`? This needs prototyping
   to determine which library best handles: markdown rendering, font embedding, image
   placement, and page layout. The recommendation is to evaluate `genpdf` first as it
   provides the highest-level API.

### Risks

1. **Transaction size for large guides** -- A guide with 20 chapters, 100 blocks, and 50
   images would produce a large transaction. If asset finalization is slow (SHA-256 hashing
   of large files), the transaction could time out. Mitigation: asset upload could happen
   outside the main transaction, with the guide transaction only linking to existing assets.

2. **Parser backward compatibility** -- Changing `ParsedGuide` to add `metadata` and
   `asset_paths` fields is a breaking change for any code that constructs `ParsedGuide`
   directly. Since `dapple-parser` is an internal crate, this is acceptable but requires
   updating all call sites.

3. **Symlink security** -- Tarballs with symlinks pointing outside the guide directory could
   be used for path traversal attacks. The ingest handler must validate that all extracted
   paths are within the temp directory and reject symlinks.

4. **Duplicate guide titles** -- The `guide_path_details` table has `UNIQUE (title)`. If two
   different tarball uploads use the same title, the second will fail. The PATCH endpoint
   handles re-ingestion of the same guide, but a completely new guide with a duplicate title
   is an error. This is the intended behavior but should be clearly communicated in error
   messages.

5. **Pandoc/xelatex removal timing** -- The existing Docker-based PDF pipeline should not be
   removed until the Rust renderer is proven stable. Both can coexist during the transition
   period. The Dockerfile should be updated only after the render endpoint is in production.

---

## 12. Addendum: Fluent-Native Guide Structure

**Added:** 2026-02-14

This addendum revises the content model for guides. Guide prose and text content moves out of inline markdown and into Fluent (`.ftl`) message files. All locales become peers -- there is no "base" language. Chapter markdown files become purely structural (directives with names and attributes only), and all human-readable text lives in `.ftl` files keyed by locale.

### 12.1 Motivation

The original design stores localized text inline in chapter markdown files and carries a `locale` field on the guide entity to identify the authoring language. This creates several problems:

1. **Locale is not a property of the guide** -- A guide's structure (stages, steps, brief packs, inference profiles) is locale-independent. The same stage hierarchy applies whether the user reads English or Portuguese.
2. **Translation requires duplicating structure** -- To offer a guide in a second language, the entire chapter/block hierarchy would need to be re-created. This is fragile and wasteful.
3. **No reuse of the existing FluentMessage infrastructure** -- The system already has a mature FluentMessage entity with localized bodies, engine bodies, locale-chain resolution, and the AND gateway pattern in `dapple-fluent`. Guide content should participate in this resolution pipeline rather than inventing its own.
4. **Inference profile text is also localizable** -- System prompts, agent prose, and profile parameters benefit from the same locale/engine resolution that FluentMessages provide.

### 12.2 Core Principle: Guides Are Locale-Independent Structures

A guide defines a tree of directives -- stages, steps, nushell scripts, brief packs, inference profiles, and prose sections. Each directive has a `name` that serves as a stable identifier. The textual content for each directive lives in Fluent message files, not in the chapter markdown.

This means:

- **`guide.toml` loses its `locale` field.** The guide is not "in English" -- it is a structure that can be rendered in any locale for which `.ftl` files exist.
- **Chapter `.md` files contain only directive fences** with names and attributes. Block content is either empty or contains only a Fluent message ID reference for tooling purposes.
- **`.ftl` files per locale** provide all human-readable text. Each locale is a peer -- `en.ftl` and `pt-BR.ftl` have equal standing, and `dapple-fluent`'s locale chain resolution picks the best match at runtime.

### 12.3 Revised Directory Layout

```
public/
  consumer/
    guide.toml                # Structural metadata -- NO locale field
    SUMMARY.md                # Chapter ordering and hierarchy (shared across locales)
    chapters/
      introduction.md         # Pure structure: directives with names, NO inline prose
      choosing-your-topic.md
      building-your-outline.md
    locales/
      en.ftl                  # English text for all blocks in this guide
      pt.ftl                  # Portuguese text
      pt-BR.ftl               # Brazilian Portuguese overrides
    assets/
      images/
        card.jpg
```

The `chapters/` directory is optional -- chapter files can remain at guide root as before. The key addition is `locales/`, which contains one `.ftl` file per locale.

### 12.4 Revised `guide.toml`

```toml
title = "Essay Helper"
description = "Ready to write an essay? We'll help you develop the fundamentals"
# NO locale field -- guide structure is locale-independent
author = "Dapple Team"
classification = "workflow"

[card]
image = "assets/images/card.jpg"
color = "#2D6A4F"
accent = "#95D5B2"
# subtitle and category move to Fluent attributes (see 12.6)
```

**Removed:** `locale` field.

**Moved to Fluent:** Card `subtitle` and `category` become Fluent attributes on the guide's brief-pack message (see Section 12.6). The `image`, `color`, and `accent` fields remain in `guide.toml` because they are locale-independent visual properties.

**Rationale for keeping some card fields in `guide.toml`:** Colors and image paths are visual design choices that do not vary by locale. Subtitle text and category labels, however, are user-facing strings that benefit from localization.

### 12.5 Structural Chapter Files

Chapter files become pure directive structure. Content between fences is either empty or a comment noting the Fluent message key for author convenience.

**Before (inline content):**
```markdown
# Choosing Your Topic

This chapter guides you through topic selection, narrowing, and thesis framing.

:::stage { name: "topic-selection" }
Guide the user from initial brainstorming through to a focused thesis statement.
:::

:::step { name: "brainstorm-topics" }
Generate a list of potential essay topics based on the user's interests and constraints.
:::
```

**After (structural only):**
```markdown
# Choosing Your Topic

:::stage { name: "topic-selection" }
:::

:::step { name: "brainstorm-topics" }
:::

:::nushell { name: "brainstorm-script", tags: [coder] }
# Script content remains inline -- code is not localized
let topics = $in | get interests | each { |interest|
    { topic: $interest, score: (random float 0.0..1.0) }
}
$topics | sort-by score --reverse | first 5
:::

:::prose { name: "brainstorm-agent-guidance", tags: [agent] }
:::
```

**Key rules:**

1. Directive fences that produce localizable text (stage, step, prose, brief-pack, inference-profile) have their content removed. The `name` attribute becomes the Fluent message key.
2. Directive fences with code content (nushell scripts) retain inline content. Code is locale-independent.
3. Every localizable block MUST have a `name` attribute. Unnamed prose blocks are a parse error under the Fluent-native model.
4. The `ref` directive retains its slug in attributes -- it is structural, not textual.

### 12.6 Fluent Message Files

Each locale gets a `.ftl` file in `locales/`. The file contains Fluent messages keyed by the `name` attribute from directive fences.

**`locales/en.ftl`:**
```ftl
## Guide-level shared vocabulary
-guide-title = Essay Helper

## Brief pack (card subtitle and category are Fluent attributes)
essay-helper-pack =
    The Essay Helper guide walks users through the complete essay writing
    process, from topic selection through final revision. It produces a
    structured outline with source links that maps directly to the final
    essay structure.
    .card-subtitle = Ready to write an essay? We'll help you develop the fundamentals
    .card-category = researcher

## Stage: Topic Selection
topic-selection =
    Guide the user from initial brainstorming through to a focused
    thesis statement.

## Steps within Topic Selection
brainstorm-topics =
    Generate a list of potential essay topics based on the user's
    interests and constraints.

## Tagged prose (agent-only guidance for brainstorming)
brainstorm-agent-guidance =
    When guiding topic selection interactively, ask about assignment
    constraints first: word count, due date, required sources, and
    any excluded topics. Use these constraints to filter brainstormed
    topics before presenting them.

## Inference profile
essay-summarizer =
    Summarize the essay draft, highlighting strengths and areas
    for improvement.
    .system-prompt =
        You are an expert writing tutor reviewing student essays.
        Focus on structural clarity, argument strength, and evidence
        quality. Be encouraging but specific about improvements.
    .temperature = 0.3
    .max-tokens = 2048
```

**`locales/pt-BR.ftl`:**
```ftl
-guide-title = Assistente de Redacao

essay-helper-pack =
    O guia Assistente de Redacao orienta os usuarios pelo processo
    completo de escrita, desde a selecao do tema ate a revisao final.
    .card-subtitle = Pronto para escrever uma redacao? Vamos ajudar com os fundamentos
    .card-category = pesquisador

topic-selection =
    Orientar o usuario desde o brainstorming inicial ate uma
    declaracao de tese focada.

brainstorm-topics =
    Gerar uma lista de possiveis temas de redacao com base nos
    interesses e restricoes do usuario.

brainstorm-agent-guidance =
    Ao orientar a selecao de temas interativamente, pergunte
    primeiro sobre as restricoes da tarefa.
```

### 12.7 Fluent Attribute Conventions

Fluent attributes (the `.key = value` syntax) carry metadata alongside the message body:

| Attribute Pattern | Used By | Purpose |
|-------------------|---------|---------|
| `.card-subtitle` | `brief-pack` | Card subtitle text for the guide carousel |
| `.card-category` | `brief-pack` | Card category label for UI filtering |
| `.system-prompt` | `inference-profile` | System prompt for the LLM |
| `.temperature` | `inference-profile` | Temperature parameter (parsed as f32) |
| `.max-tokens` | `inference-profile` | Max token limit (parsed as u32) |
| `.description` | Any directive | Extended description beyond the body |

Fluent terms (`-guide-title = ...`) provide shared vocabulary within the guide. They can be referenced from other messages using `{ -guide-title }` syntax. Terms are not stored as FluentMessage entities -- they are resolved during `.ftl` parsing and inlined into message bodies before storage.

### 12.8 Block `name` as Fluent Message Identifier

The `name` attribute on a directive fence serves double duty:

1. **Structural identity** -- used by the transform layer to link blocks to derived objects (e.g., linking a `:::nushell { for: "brainstorm-topics" }` directive to its step).
2. **Fluent message key** -- used to look up the block's text content from the `.ftl` files.

**Naming conventions:**

| Directive | Name Convention | Example |
|-----------|----------------|---------|
| `brief-pack` | `{guide-slug}-pack` | `essay-helper-pack` |
| `stage` | `{stage-slug}` | `topic-selection` |
| `step` | `{step-slug}` | `brainstorm-topics` |
| `prose` (tagged) | `{context-slug}-{tag}-prose` or authored explicitly | `brainstorm-agent-guidance` |
| `inference-profile` | `{profile-slug}` | `essay-summarizer` |
| `nushell` | `{script-slug}` | `brainstorm-script` |
| `ref` | N/A (refs are structural, not textual) | -- |

Names must be unique within a guide. The parser validates this during ingest.

### 12.9 Inference Profiles Follow the Same Pattern

Inference profiles are defined structurally in chapter files and textually in `.ftl` files:

**Chapter file:**
```markdown
:::inference-profile { name: "essay-summarizer" }
:::
```

**`locales/en.ftl`:**
```ftl
essay-summarizer =
    Summarize the essay draft, highlighting strengths and areas
    for improvement.
    .system-prompt =
        You are an expert writing tutor reviewing student essays.
        Focus on structural clarity, argument strength, and evidence
        quality. Be encouraging but specific about improvements.
    .temperature = 0.3
    .max-tokens = 2048
```

The message body becomes the profile's user-facing description. Fluent attributes carry the operational parameters. At runtime, the conduit resolves the best body via locale chain and engine chain using the existing AND gateway resolution, then extracts attributes for the profile configuration.

### 12.10 Data Model Changes

#### 12.10.1 Remove `locale` from `rev.guides`

```sql
-- Migration: 20260216000001_guides_drop_locale.sql

-- Remove locale column from guides (guides are now locale-independent).
-- Existing data: locale values are preserved in a new authoring_locale column
-- on guide_path_details for editorial context only (not used at runtime).

ALTER TABLE rev.guide_path_details
    ADD COLUMN authoring_locale TEXT;

-- Backfill from existing guide versions (take the locale from the latest version)
UPDATE rev.guide_path_details gpd
SET authoring_locale = (
    SELECT g.locale
    FROM rev.guides g
    JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
    WHERE vo.path_id = gpd.path_id
    ORDER BY vo.version_major DESC, vo.version_minor DESC, vo.version_patch DESC
    LIMIT 1
);

ALTER TABLE rev.guides DROP COLUMN locale;
```

**Note:** This is the one UPDATE in the migration -- a data migration to preserve existing locale values before dropping the column. After migration, `authoring_locale` on `guide_path_details` is purely informational (for editorial tooling) and is not used by runtime resolution.

#### 12.10.2 Add `fluent_message_id` to `rev.guide_blocks`

```sql
-- Migration: 20260216000002_guide_blocks_fluent_message.sql

-- Link guide blocks to their Fluent message for localized content.
-- Nullable because some blocks (nushell scripts, refs) have inline content
-- and do not use Fluent messages.
ALTER TABLE rev.guide_blocks
    ADD COLUMN fluent_message_id BIGINT REFERENCES rev.fluent_messages (id);

CREATE INDEX guide_blocks_fluent_message_idx
    ON rev.guide_blocks (fluent_message_id)
    WHERE fluent_message_id IS NOT NULL;

-- Make content column nullable for Fluent-backed blocks.
-- Blocks with fluent_message_id get their content from the FluentMessage;
-- the content column is empty or contains a fallback.
ALTER TABLE rev.guide_blocks ALTER COLUMN content DROP NOT NULL;
```

#### 12.10.3 Summary of Column Changes

| Table | Column | Change |
|-------|--------|--------|
| `rev.guides` | `locale` | Dropped |
| `rev.guide_path_details` | `authoring_locale` | Added (TEXT, nullable, informational) |
| `rev.guide_blocks` | `fluent_message_id` | Added (BIGINT, nullable, FK to `rev.fluent_messages`) |
| `rev.guide_blocks` | `content` | Changed from `NOT NULL` to nullable |

### 12.11 Revised Object Models

#### 12.11.1 `GuideToml` (Parser)

```rust
/// Parsed representation of guide.toml (Fluent-native).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideToml {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    // NO locale field
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub classification: Option<String>,
    #[serde(default)]
    pub card: Option<GuideCardToml>,
}

/// The [card] section of guide.toml (Fluent-native).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideCardToml {
    // subtitle and category are now Fluent attributes, not TOML fields
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub accent: Option<String>,
}
```

#### 12.11.2 `GuideMetadata` (Parser)

```rust
/// Full metadata from guide.toml (Fluent-native).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideMetadata {
    pub title: String,
    pub description: Option<String>,
    // NO locale field
    pub author: Option<String>,
    pub classification: Option<String>,
    pub card: Option<ParsedCard>,
}
```

#### 12.11.3 `ParsedGuide` Extension

```rust
/// The complete parsed representation of a guide directory (Fluent-native).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedGuide {
    pub title: String,
    pub metadata: GuideMetadata,
    pub chapters: Vec<ParsedChapter>,
    pub asset_paths: Vec<String>,
    /// Fluent message definitions extracted from locales/*.ftl files.
    /// Keyed by locale tag (e.g., "en", "pt-BR").
    pub locales: HashMap<String, Vec<FluentMessageDef>>,
}

/// A single Fluent message definition from a .ftl file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FluentMessageDef {
    /// The message key (matches directive `name` attribute).
    pub key: String,
    /// The message body text.
    pub body: String,
    /// Fluent attributes (.key = value pairs).
    pub attributes: HashMap<String, String>,
}
```

#### 12.11.4 `CreateGuideSchema` (API)

```rust
/// Schema for creating a single guide (Fluent-native).
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateGuideSchema {
    pub title: String,
    pub description: Option<String>,
    // NO locale field
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub chapters: Vec<CreateGuideChapterSchema>,
}
```

#### 12.11.5 `Guide` DTO

```rust
/// Public DTO for guides (Fluent-native).
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Guide {
    pub object_id: Uuid,
    pub path_object_id: Uuid,
    pub title: String,
    pub slug: String,
    pub version: Version,
    pub deprecation_level: Option<String>,
    pub description: Option<String>,
    // NO locale field
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub chapters: Vec<GuideChapter>,
    pub card: Option<GuideCard>,
    pub assets: Vec<GuideAssetRef>,
    pub created_at: DateTime,
}
```

#### 12.11.6 `GuideBlock` DTO Extension

```rust
/// Public DTO for guide blocks (Fluent-native).
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideBlock {
    pub object_id: Uuid,
    pub directive_type: String,
    pub name: Option<String>,
    /// Raw content (for non-Fluent blocks like nushell scripts).
    /// Null/empty for Fluent-backed blocks.
    pub content: Option<String>,
    pub attributes: serde_json::Value,
    pub tags: Vec<String>,
    /// Reference to the FluentMessage providing localized content.
    /// Null for blocks with inline content (nushell, ref).
    pub fluent_message_object_id: Option<Uuid>,
    pub created_at: DateTime,
}
```

#### 12.11.7 Internal Model Changes

The `InsertGuideArgs` struct drops `locales`:

```rust
pub struct InsertGuideArgs {
    pub titles: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    // NO locales field
    pub authors: Vec<Option<String>>,
    pub classifications: Vec<Option<String>>,
    pub source_paths: Vec<Option<String>>,
}
```

The `InsertGuideBlockArgs` struct gains `fluent_message_ids`:

```rust
pub struct InsertGuideBlockArgs {
    pub chapter_ids: Vec<i64>,
    pub directive_type_ids: Vec<i16>,
    pub names: Vec<Option<String>>,
    pub contents: Vec<Option<String>>,  // Now Option<String>
    pub attributes: Vec<serde_json::Value>,
    pub fluent_message_ids: Vec<Option<i64>>,
}
```

The `FlatGuideModel`, `GuideModel`, `GuideBlockModel`, and `InsertedGuideModel` structs all lose `locale` and the block variants gain `fluent_message_id` / `fluent_message_object_id` as appropriate.

### 12.12 Revised Ingest Pipeline

The ingest pipeline gains two new phases: `.ftl` discovery/parsing and FluentMessage materialization.

```
1. Extract tarball
2. Parse guide.toml (no locale field)
3. Parse SUMMARY.md
4. Parse chapter .md files -> extract directive structure (names, types, attributes)
5. Discover locales/*.ftl files -> parse into FluentMessageDef per locale   [NEW]
6. Validate: every directive name has a matching key in at least one .ftl file
7. Plan ingest (transform ParsedGuide -> IngestPlan)
8. Begin transaction
9.  a. Create FluentMessage paths + versions for each unique block name     [NEW]
    b. Create FluentMessage localized bodies from each locale's .ftl file   [NEW]
    c. Upload assets via draft/chunk/asset pipeline
    d. Materialize derived objects (playbook, tasks, brief packs, inference profiles)
       -- All derived objects reference FluentMessages for their text content
    e. Create guide entity with chapters and blocks
       -- Blocks reference fluent_message_id instead of storing inline content
    f. Create guide_assets associations
    g. Create guide_cards (subtitle/category resolved from Fluent attributes)
10. Commit transaction
```

**Step 9a-9b detail:** For each unique `name` across all directive fences in the guide:

1. Create a FluentMessage path with `name = {directive-name}` and `description` from the `.ftl` body.
2. Create a FluentMessage version (0.0.1) linked to that path.
3. For each locale `.ftl` file that contains a message with that key, create a `fluent_message_body` row with the locale tag and body text.
4. If the `.ftl` file contains engine-body-style attributes (not yet defined), create engine body rows as well.

**Step 9g detail:** Card subtitle and category are extracted from Fluent attributes:

1. Look up the brief-pack's FluentMessage by its name.
2. Extract `.card-subtitle` attribute for the target card locale (or use locale chain fallback).
3. Extract `.card-category` attribute similarly.
4. Store the resolved values in `rev.guide_cards`.

### 12.13 Runtime Resolution

At runtime, when the conduit needs text content for a guide block (e.g., a stage description, a step instruction, agent prose):

1. The block carries a `fluent_message_id` reference.
2. The conduit calls the existing FluentMessage resolution pipeline with the user's locale chain and engine chain.
3. `dapple-fluent`'s AND gateway resolution picks the best body from the localized bodies.
4. The resolved text is injected into the Handlebars template context.

This means guide content participates in the exact same resolution logic as prompt messages, task descriptions, and all other FluentMessage consumers. No new resolution infrastructure is needed.

### 12.14 Impact on Existing Workstreams

#### Workstream 1 (Parser Modernization)

- **Remove** `locale` from `GuideToml`, `GuideMetadata`, and the `default_locale()` function.
- **Remove** `subtitle` and `category` from `GuideCardToml`.
- **Add** `.ftl` file discovery: scan `locales/` directory for `*.ftl` files.
- **Add** `.ftl` parsing: use the `fluent-syntax` crate (or a lightweight parser) to extract message keys, bodies, and attributes into `FluentMessageDef` structs.
- **Add** `locales: HashMap<String, Vec<FluentMessageDef>>` to `ParsedGuide`.
- **Enforce** that every non-code directive has a `name` attribute (parse error otherwise).
- **Validate** that every directive `name` appears as a key in at least one `.ftl` file.

**New file:** `crates/dapple-parser/src/ftl.rs` -- Fluent file discovery and parsing.

**New dependency:** `fluent-syntax = "0.11"` in `crates/dapple-parser/Cargo.toml`.

#### Workstream 2 (Tarball Ingest)

- **Transform layer** gains `FluentMessageDef` handling. `IngestPlan` grows:
  ```rust
  pub struct IngestPlan {
      // ... existing fields ...
      /// FluentMessage definitions per locale, keyed by message name.
      pub fluent_messages: HashMap<String, FluentMessageSet>,
  }

  pub struct FluentMessageSet {
      /// The message key (matches directive name).
      pub key: String,
      /// Description (from the first locale's body, or explicit).
      pub description: Option<String>,
      /// Localized bodies keyed by locale tag.
      pub bodies: HashMap<String, String>,
      /// Fluent attributes keyed by locale, then by attribute name.
      pub attributes: HashMap<String, HashMap<String, String>>,
  }
  ```

- **Ingest handler** creates FluentMessages before creating guide blocks:
  1. Batch-insert FluentMessage paths and versions for all unique block names.
  2. Batch-insert localized bodies for each locale.
  3. Pass `fluent_message_id` to the block insert args.

- **Card creation** resolves subtitle and category from Fluent attributes rather than from `guide.toml`.

- **`InsertGuideArgs`** drops `locales`.
- **`InsertGuideBlockArgs`** gains `fluent_message_ids` and makes `contents` optional.

#### Workstream 3 (Guide Assets)

No changes. Assets (images, PDFs, etc.) are locale-independent files in the guide directory. The `locales/` directory contains only `.ftl` files, which are not treated as assets.

#### Workstream 4 (Promotional Cards)

- **Card subtitle** moves from `guide.toml [card].subtitle` to the Fluent attribute `.card-subtitle` on the brief-pack message.
- **Card category** moves from `guide.toml [card].category` to the Fluent attribute `.card-category` on the brief-pack message.
- **Card image, color, accent** remain in `guide.toml [card]` (locale-independent visual properties).
- **`rev.guide_cards`** schema is unchanged -- the `subtitle` and `category` columns still exist. The change is in where the values come from during ingest: Fluent attributes rather than TOML fields.
- **Card carousel endpoint** is unchanged. The resolved subtitle and category are stored in the database at ingest time, so no runtime Fluent resolution is needed for card display.

#### Workstream 5 (PDF Rendering)

- **Render pipeline gains a `locale` parameter.** The render request becomes:
  ```json
  {
    "profile": "consumer",
    "locale": "en"
  }
  ```
- **Block content resolution** happens before rendering: for each Fluent-backed block, resolve the FluentMessage body for the requested locale using the locale chain.
- **Render cache key** expands to `(guide_object_id, profile_key, locale)`.
- **Fallback behavior:** if a block has no FluentMessage body for the requested locale, fall back through the locale chain. If no body exists at all, omit the block from the rendered output (or render a placeholder, depending on profile configuration).

### 12.15 Revised `IngestPlan` Definition Types

Several definition types from Section 3.3 gain FluentMessage references:

```rust
pub struct StageDef {
    pub name: String,
    // description removed -- lives in FluentMessage body
    pub flow_control: Option<serde_json::Value>,
    pub steps: Vec<StepDef>,
    pub source_block_index: BlockIndex,
}

pub struct StepDef {
    pub name: String,
    // description removed -- lives in FluentMessage body
    pub flow_control: Option<serde_json::Value>,
    pub task_ref: Option<String>,
    pub source_block_index: BlockIndex,
}

pub struct BriefPackDef {
    pub name: String,
    // content removed -- lives in FluentMessage body
    // card_subtitle and card_category extracted from Fluent attributes
    pub source_block_index: BlockIndex,
}

pub struct InferenceProfileDef {
    pub name: String,
    // description removed -- lives in FluentMessage body
    // system_prompt, temperature, max_tokens extracted from Fluent attributes
    pub source_block_index: BlockIndex,
}
```

The text content and operational parameters are no longer embedded in the definition types. Instead, the ingest handler resolves them from the `FluentMessageSet` in `IngestPlan.fluent_messages` when materializing derived objects.

### 12.16 Validation Rules (Revised)

The following validations are added or modified for the Fluent-native model:

1. **Every non-code directive MUST have a `name` attribute.** Parse error otherwise. ("Non-code" means all directives except `nushell` blocks that contain script content, though nushell blocks still need names for linking.)
2. **Every directive `name` MUST appear as a message key in at least one `.ftl` file.** This ensures no block is orphaned without text content.
3. **No duplicate `name` values within a guide.** Each name must be unique across all chapters.
4. **At least one `.ftl` file must exist in `locales/`.** A guide with no Fluent files is a validation error.
5. **Fluent syntax must be valid.** Malformed `.ftl` files produce parse errors with file and line information.
6. **Inference profile attributes must be parseable.** The `.temperature` attribute must parse as a float, `.max-tokens` as an integer. Invalid values are validation errors.
7. **`guide.toml` must not contain `locale` field.** If present, emit a deprecation warning (not an error) and ignore the value.
8. **`guide.toml [card]` must not contain `subtitle` or `category`.** If present, emit a deprecation warning and ignore the values in favor of Fluent attributes.

### 12.17 Migration Strategy for Existing Guides

Existing guides authored with inline content and a `locale` field need a migration path:

1. **Extract inline content to `.ftl` files.** A migration tool (CLI command in `dapple` crate or a script) reads existing chapter files, extracts block content by name, and writes `locales/{locale}.ftl` where `{locale}` comes from the old `guide.toml` locale field.
2. **Strip inline content from chapter files.** The same tool removes block content from chapter files, leaving only the directive fences with names and attributes.
3. **Remove `locale` from `guide.toml`.** The tool rewrites `guide.toml` without the `locale` field.
4. **Move card subtitle and category.** The tool extracts `subtitle` and `category` from `guide.toml [card]` and adds them as Fluent attributes on the brief-pack message in the `.ftl` file.

This migration is a content-authoring operation (run against the `dapple-guides` repo), not a database migration. The database migrations in Section 12.10 handle the schema changes. Re-ingesting the migrated guide via the ingest endpoint populates the new FluentMessage-backed structure.

### 12.18 Files to Create/Modify

| Action | File | Notes |
|--------|------|-------|
| Create | Migration `20260216000001_guides_drop_locale.sql` | Drop locale, add authoring_locale to path_details |
| Create | Migration `20260216000002_guide_blocks_fluent_message.sql` | Add fluent_message_id FK, make content nullable |
| Create | `crates/dapple-parser/src/ftl.rs` | Fluent file discovery and parsing |
| Modify | `crates/dapple-parser/Cargo.toml` | Add `fluent-syntax = "0.11"` |
| Modify | `crates/dapple-parser/src/guide_toml.rs` | Remove `locale`, remove card `subtitle`/`category` |
| Modify | `crates/dapple-parser/src/types.rs` | Update `GuideMetadata`, `ParsedGuide`, `ParsedCard` |
| Modify | `crates/dapple-parser/src/lib.rs` | Add `.ftl` discovery to parse pipeline |
| Modify | `crates/dapple-parser/src/transform/mod.rs` | Add `FluentMessageSet` to `IngestPlan` |
| Modify | `crates/dapple-prompt-api/src/schema/guides.rs` | Remove `locale` from `CreateGuideSchema` |
| Modify | `crates/dapple-prompt-api/src/schema/guide_ingest.rs` | Add locale to render request |
| Modify | `crates/dapple-prompt-api/src/handler/guide_ingest.rs` | Add FluentMessage creation phase |
| Modify | `crates/dapple-data/src/model/guides.rs` | Remove `locale` fields, add `fluent_message_id` |
| Modify | `crates/dapple-data/assets/sql/guides/insert_guides.sql` | Drop locale parameter |
| Modify | `crates/dapple-data/assets/sql/guides/insert_guide_blocks.sql` | Add fluent_message_id parameter |
| Modify | All guide SELECT queries | Remove locale, add fluent_message join |

### 12.19 Open Questions

1. **Fluent term resolution strategy** -- Should terms (`-guide-title = ...`) be resolved at parse time (inlined into message bodies before storage) or stored as metadata and resolved at runtime? Parse-time resolution is simpler and avoids needing a Fluent runtime in the conduit. Recommendation: parse-time inlining.

2. **Engine bodies from `.ftl` files** -- The current FluentMessage model supports engine bodies (per-model-release text). Should `.ftl` files support an engine body syntax, or should engine bodies only be created via the `PATCH /api/fluent-messages/` endpoint after ingest? Recommendation: defer engine body support in `.ftl` files to a future iteration. The initial implementation creates only localized bodies from `.ftl` files.

3. **Locale chain for card ingest** -- When ingesting a guide, what locale should be used to resolve the card subtitle and category from Fluent attributes? Options: (a) use the first locale alphabetically, (b) prefer `en` if available, (c) require the user to specify a primary locale in the ingest request. Recommendation: prefer `en` if available, otherwise use the first locale found.

4. **Backward compatibility of the JSON API** -- The `POST /api/guides/` JSON endpoint currently accepts `locale` and inline block `content`. Should this endpoint continue to work with the old format for backward compatibility, or should it require the Fluent-native format? Recommendation: accept both formats during a transition period. If `locale` is present, treat it as a legacy request and store content inline. If absent, require `fluent_message_object_ids` on blocks.

5. **`.ftl` file size limits** -- A guide with many blocks and verbose descriptions could produce large `.ftl` files. Is there a practical limit? Recommendation: no hard limit initially, but log a warning if a single `.ftl` file exceeds 100KB.

---

## 13. Addendum: Shared Content Library

**Added:** 2026-02-14

This addendum introduces a shared content library system. Guides compose from reusable, versioned objects that live in scoped shared directories. Utility stages, utility steps, common Fluent terms and messages, shared inference profiles, shared tasks, and shared assets are authored once in `_shared/` directories and referenced by path from any guide whose visibility scope permits access. At runtime, the latest non-deprecated version of a shared object is resolved automatically -- updating a shared object propagates to all referencing guides without re-ingestion.

### 13.1 Motivation

As the guide library grows, common patterns emerge: a "validate output" stage that appears in six guides, a "summarize and review" stage reused across workflows, brand terms like `-brand-name` that must be consistent everywhere, and inference profiles tuned for specific tasks (summarization, evaluation) that should not be copy-pasted. Without a sharing mechanism:

1. **Duplication** -- The same stage definition, Fluent messages, and inference profile configuration are duplicated across guides. A fix to the shared logic requires editing every guide that uses it.
2. **Drift** -- Duplicated content drifts over time. One guide's "validate output" stage diverges from another's because they were updated independently.
3. **Consistency** -- Brand terms, disclaimers, and boilerplate text must be identical across all guides. Manual synchronization is error-prone.
4. **Access control** -- Internal utility stages should be available to internal guides but invisible to public ones. Without scoped sharing, there is no mechanism for this.

### 13.2 Core Concept

Shared content lives in `_shared/` directories at the root of the `dapple-guides` repo, organized by visibility scope. Guides reference shared objects by path using the `ref` attribute on directives. At runtime, references resolve to the latest non-deprecated version of the shared object -- the reference is to the **path** (stable identity), not to a specific version.

### 13.3 Layered Visibility Scopes

Scopes determine which guides can see which shared content. The scope hierarchy is additive -- a guide sees its own scope plus all scopes below it.

| Shared Scope | Visible To | Ingest Endpoint |
|-------------|-----------|-----------------|
| `_shared/common/` | All guides (public, partner, internal) | `POST /api/shared/ingest?scope=common` |
| `_shared/internal/` | Internal guides only | `POST /api/shared/ingest?scope=internal` |
| `_shared/partners/{uuid}/` (future) | That partner's guides + internal guides | `POST /api/shared/ingest?scope=partner&partner_id={uuid}` |

**Visibility matrix:**

| Guide Location | Sees `common` | Sees `internal` | Sees `partners/{uuid}` |
|---------------|:---:|:---:|:---:|
| `public/*` | Yes | No | No |
| `internal/*` | Yes | Yes | No |
| `partners/{uuid}/*` | Yes | No | Own partner only |

### 13.4 Shared Directory Structure

Each scope directory mirrors the structure of a guide but contains reusable fragments instead of complete guide chapters.

```
_shared/
  common/
    shared.toml              # Manifest: name, description, scope
    SUMMARY.md               # List of shared content modules
    stages/
      validate-output.md     # :::stage + :::step definitions
      summarize-and-review.md
      retry-with-backoff.md
    profiles/
      summarizer.md          # :::inference-profile definition
      evaluator.md
    locales/
      en.ftl                 # Fluent messages for all shared content
      pt.ftl
    assets/
      logo.svg
  internal/
    shared.toml
    SUMMARY.md
    stages/
      admin-review.md
    locales/
      en.ftl
      pt.ftl
```

**Key conventions:**

- Each `.md` file under `stages/` contains one or more `:::stage` and `:::step` directive fences. The file name (without extension) becomes the shared path key (e.g., `common/validate-output`).
- Each `.md` file under `profiles/` contains one `:::inference-profile` directive fence. The file name becomes the shared path key (e.g., `common/summarizer`).
- `locales/*.ftl` provides Fluent messages for all directives defined in the scope. These follow the same conventions as guide-level `.ftl` files (Section 12.6).
- `assets/` contains files that shared content references (logos, icons, brand images).

### 13.5 shared.toml Format

```toml
name = "common"
description = "Shared utility stages, profiles, and terms"
scope = "common"   # "common", "internal", or "partner"
```

| Field | Required | Purpose |
|-------|----------|---------|
| `name` | Yes | Human-readable name for the shared library |
| `description` | No | Description of what this shared library provides |
| `scope` | Yes | Must match the directory's scope: `"common"`, `"internal"`, or `"partner"` |

The `scope` field is validated against the ingest endpoint's `scope` query parameter. A mismatch is a validation error.

### 13.6 shared.toml Parser Type

```rust
/// Parsed representation of shared.toml.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedToml {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub scope: String,
}

/// Parse shared.toml from file contents.
pub fn parse_shared_toml(content: &str) -> Result<SharedToml, ParseError> {
    toml::from_str(content).map_err(|e| ParseError::InvalidSharedToml {
        detail: e.to_string(),
    })
}
```

### 13.7 Guide Referencing Shared Content

Guides import shared objects via the `ref` attribute on directives. When a directive has a `ref` attribute, it means "link to the shared object at this path" rather than "create a new object inline."

**Chapter file in a guide:**
```markdown
# Workflow

# Custom stage defined in this guide
:::stage { name: "topic-selection" }
:::

# Shared utility stage imported by reference
:::stage { ref: "common/validate-output" }
:::

:::stage { ref: "common/summarize-and-review" }
:::

:::inference-profile { ref: "common/summarizer" }
:::
```

**Rules:**

1. A directive with `ref` MUST NOT also have a `name` attribute. The name comes from the shared definition.
2. A directive with `ref` MUST NOT have inline content. Content comes from the shared object's Fluent messages.
3. A directive with `ref` MAY have additional attributes (e.g., `tags`) that override or extend the shared definition for this guide's context.
4. The `ref` value is a slash-delimited path: `{scope}/{object-name}`. The scope prefix determines which shared library to search.
5. A directive without `ref` is a guide-local definition (the existing behavior).

### 13.8 Path-Level References for Runtime Resolution

Shared objects receive path-level identity in `rev.object_paths` with metadata that includes a scope marker. When a guide references `common/validate-output`, the ingest pipeline links the guide to the **path** of the shared object, not to a specific version. This has two consequences:

1. **Automatic version propagation** -- Updating `_shared/common/` and re-ingesting creates new versions of shared objects. All guides that reference them automatically resolve the latest version at runtime without re-ingestion.
2. **Stable identity** -- The `object_path.object_id` for a shared object never changes. Guides hold a durable reference that survives version updates.

**Resolution flow at runtime:**

```
Guide block with ref "common/validate-output"
  -> guide_shared_refs table: guide_id -> object_path_id
  -> rev.object_paths: find path with ref_key = "common/validate-output"
  -> rev.versioned_objects: latest non-deprecated version for that path
  -> Entity-specific table (e.g., rev.stages): full object data
```

### 13.9 Access Control at Ingest Time

Scope visibility is enforced at guide ingest time, not at runtime. This means:

1. When a guide at `public/consumer/` references `common/validate-output`, the ingest pipeline permits it (public guides can see `common`).
2. When a guide at `public/consumer/` references `internal/admin-review`, the ingest pipeline **rejects it** with a validation error.
3. When a guide at `internal/ops-guide/` references `internal/admin-review`, the ingest pipeline permits it (internal guides can see `common` + `internal`).

**Validation rule:** The guide's `source_path` determines its location tier. The location tier determines which scopes are visible:

| Guide Location Tier | Visible Scopes |
|---------------------|---------------|
| `public/*` | `common` |
| `internal/*` | `common`, `internal` |
| `partners/{uuid}/*` | `common`, `partners/{uuid}` |

Rejection produces a 422 validation error with a message like: `Guide at "public/consumer" cannot reference shared scope "internal" (ref: "internal/admin-review")`.

### 13.10 Shared Content Ingest Pipeline

Shared content is ingested separately from guides via a dedicated endpoint.

**Endpoint:**
```
POST /api/shared/ingest
Content-Type: multipart/form-data

Fields:
  file: <shared.tar.gz>          (required)
  scope: "common"|"internal"     (required query param)
  partner_id: UUID               (required when scope=partner, future)
  dry_run: true|false            (optional, default false)
```

**Pipeline steps:**

```
1. Extract tarball
2. Parse shared.toml -- validate scope matches query param
3. Parse SUMMARY.md -- list of content modules
4. Parse .md files in stages/, profiles/ -- extract directive definitions
5. Discover locales/*.ftl files -- parse Fluent messages
6. Discover assets/ files
7. Validate:
   a. Every directive name has a matching key in at least one .ftl file
   b. shared.toml scope matches the endpoint scope query param
   c. No duplicate names within the shared library
8. If dry_run, return validation summary
9. Begin transaction
   a. Create/version shared objects (paths + versioned_objects)
      -- For each directive definition, find or create an object_path
         with scope metadata and ref_key
      -- If path exists, create new version (patch increment)
      -- If path is new, create path + version 0.0.1
   b. Create FluentMessage paths + versions for each shared directive name
   c. Create FluentMessage localized bodies from each locale's .ftl file
   d. Upload assets via draft/chunk/asset pipeline
   e. Create entity-specific rows (stages, steps, tasks, inference profiles)
10. Commit transaction
11. Return list of created/updated shared objects with their path_object_ids
```

**Response:**
```json
{
  "scope": "common",
  "objects": [
    {
      "ref_key": "common/validate-output",
      "path_object_id": "...",
      "version": "0.0.2",
      "entity_type": "stage",
      "action": "updated"
    },
    {
      "ref_key": "common/summarizer",
      "path_object_id": "...",
      "version": "0.0.1",
      "entity_type": "inference_profile",
      "action": "created"
    }
  ],
  "fluent_messages_created": 12,
  "assets_created": 1
}
```

### 13.11 Request/Response Schemas

```rust
// crates/dapple-prompt-api/src/schema/shared_ingest.rs

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Query parameters for the shared content ingest endpoint.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct SharedIngestQueryParams {
    /// The visibility scope for this shared library.
    pub scope: String,
    /// Partner UUID (required when scope = "partner").
    pub partner_id: Option<Uuid>,
    /// When true, validate without persisting.
    #[serde(default)]
    pub dry_run: bool,
}

/// A single shared object in the ingest response.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct SharedObjectResult {
    pub ref_key: String,
    pub path_object_id: Uuid,
    pub version: String,
    pub entity_type: String,
    pub action: SharedObjectAction,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SharedObjectAction {
    Created,
    Updated,
}

/// Response from the shared content ingest endpoint.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct SharedIngestResponse {
    pub scope: String,
    pub objects: Vec<SharedObjectResult>,
    pub fluent_messages_created: usize,
    pub assets_created: usize,
}

/// Dry-run validation result for shared content.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct SharedIngestValidationResult {
    pub valid: bool,
    pub errors: Vec<IngestError>,
    pub warnings: Vec<String>,
    pub summary: SharedIngestSummary,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct SharedIngestSummary {
    pub scope: String,
    pub stages: usize,
    pub steps: usize,
    pub tasks: usize,
    pub inference_profiles: usize,
    pub fluent_messages: usize,
    pub assets: usize,
}
```

### 13.12 Database Schema Changes

#### 13.12.1 Add `scope` Column to `rev.object_paths`

```sql
-- Migration: 20260217000001_object_paths_scope.sql

-- Add scope column to object_paths for shared content visibility control.
-- NULL means guide-private (not shared). Non-null values indicate the
-- shared scope that owns this path.
--
-- Values: 'common', 'internal', 'partner:{uuid}'
-- The scope column is used at guide ingest time to validate that a guide's
-- location tier permits access to the referenced shared object.

ALTER TABLE rev.object_paths
    ADD COLUMN scope TEXT;

-- Partial index for scope lookups -- only shared objects have non-null scope.
CREATE INDEX object_paths_scope_idx
    ON rev.object_paths (scope)
    WHERE scope IS NOT NULL;
```

#### 13.12.2 Add `ref_key` Column to `rev.object_paths`

```sql
-- Migration: 20260217000002_object_paths_ref_key.sql

-- Add ref_key column to object_paths for shared content path-based lookup.
-- The ref_key is the slash-delimited path used in guide :::directive { ref: "..." }
-- attributes (e.g., "common/validate-output", "internal/admin-review").
-- NULL for non-shared (guide-private) objects.

ALTER TABLE rev.object_paths
    ADD COLUMN ref_key TEXT;

-- Unique constraint on ref_key -- each shared path must have a unique ref_key.
-- Partial unique because guide-private objects have NULL ref_key.
CREATE UNIQUE INDEX object_paths_ref_key_unique_idx
    ON rev.object_paths (ref_key)
    WHERE ref_key IS NOT NULL;
```

#### 13.12.3 Shared Scopes Lookup Table

```sql
-- Migration: 20260217000003_shared_scopes.sql

-- Lookup table for valid shared content scopes.
-- Used to validate the scope query parameter on the ingest endpoint
-- and the scope column on object_paths.

CREATE TABLE rev.shared_scopes (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed scopes.
INSERT INTO rev.shared_scopes (key, description) VALUES
    ('common', 'Shared content visible to all guides (public, partner, internal)'),
    ('internal', 'Shared content visible only to internal guides');

-- Partner scopes will be inserted dynamically when partners are onboarded (future).
-- Pattern: 'partner:{uuid}' -- validated by convention, not by FK.
```

#### 13.12.4 Guide-to-Shared-Object References

```sql
-- Migration: 20260217000004_guide_shared_refs.sql

-- Associative table linking guide versions to shared object paths.
-- Each row records one :::directive { ref: "..." } usage within a guide.
-- The ref_key column stores the string used in the ref attribute for
-- provenance and debugging. The object_path_id is the resolved path.
--
-- This table is an Associative Dependent of the Guide principal.
-- It is created atomically during guide ingest and has no separate endpoint.

CREATE TABLE rev.guide_shared_refs (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    guide_id BIGINT NOT NULL REFERENCES rev.guides (id),
    object_path_id BIGINT NOT NULL REFERENCES rev.object_paths (id),
    ref_key TEXT NOT NULL,
    directive_type TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, ref_key)
);

CREATE INDEX idx_guide_shared_refs_guide_id
    ON rev.guide_shared_refs (guide_id);
CREATE INDEX idx_guide_shared_refs_object_path_id
    ON rev.guide_shared_refs (object_path_id);
```

**Column details:**

| Column | Purpose |
|--------|---------|
| `guide_id` | FK to the guide version that contains the reference |
| `object_path_id` | FK to the shared object's stable path identity |
| `ref_key` | The string from the `ref` attribute (e.g., `"common/validate-output"`) |
| `directive_type` | The directive type of the reference (e.g., `"stage"`, `"inference_profile"`) |

The `UNIQUE (guide_id, ref_key)` constraint prevents a guide from referencing the same shared object twice.

#### 13.12.5 Summary of Schema Changes

| Table | Column/Constraint | Change |
|-------|-------------------|--------|
| `rev.object_paths` | `scope` | Added (TEXT, nullable) |
| `rev.object_paths` | `ref_key` | Added (TEXT, nullable, unique where non-null) |
| `rev.shared_scopes` | (new table) | Lookup table for valid scope values |
| `rev.guide_shared_refs` | (new table) | Associative table linking guides to shared object paths |

### 13.13 Object Models

#### 13.13.1 Parsed Shared Library

```rust
/// The complete parsed representation of a shared content directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedSharedLibrary {
    /// Manifest metadata from shared.toml.
    pub manifest: SharedToml,
    /// Stage definitions from stages/*.md files.
    pub stages: Vec<SharedStageDef>,
    /// Inference profile definitions from profiles/*.md files.
    pub profiles: Vec<SharedInferenceProfileDef>,
    /// Task definitions (from :::nushell blocks within stage files).
    pub tasks: Vec<SharedTaskDef>,
    /// Fluent message definitions per locale.
    pub locales: HashMap<String, Vec<FluentMessageDef>>,
    /// Asset file entries from assets/.
    pub asset_paths: Vec<String>,
}

/// A shared stage definition parsed from a .md file in stages/.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedStageDef {
    /// The shared path key (e.g., "validate-output" from the filename).
    pub key: String,
    /// Stage definition extracted from :::stage fence.
    pub stage: StageDef,
}

/// A shared inference profile definition parsed from a .md file in profiles/.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedInferenceProfileDef {
    /// The shared path key (e.g., "summarizer" from the filename).
    pub key: String,
    /// Profile definition extracted from :::inference-profile fence.
    pub profile: InferenceProfileDef,
}

/// A shared task definition extracted from a :::nushell block.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedTaskDef {
    /// The shared path key.
    pub key: String,
    /// Task definition.
    pub task: TaskDef,
}
```

#### 13.13.2 Shared Reference in IngestPlan

The `IngestPlan` (Section 3.3) gains a field for shared references encountered during guide parsing:

```rust
pub struct IngestPlan {
    // ... existing fields from Section 3.3 ...
    // ... fluent_messages from Section 12.14 ...

    /// Shared content references found in :::directive { ref: "..." } blocks.
    /// Each entry records the ref_key and directive type for validation
    /// and for creating guide_shared_refs rows during ingest.
    pub shared_refs: Vec<SharedRefDef>,
}

/// A reference to a shared object found during guide parsing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedRefDef {
    /// The ref path as written in the directive (e.g., "common/validate-output").
    pub ref_key: String,
    /// The directive type (e.g., "stage", "inference_profile").
    pub directive_type: String,
    /// Location within the parsed guide for provenance.
    pub source_block_index: BlockIndex,
}
```

#### 13.13.3 ParsedBlock Extension

The parser's `ParsedBlock` type gains a field to distinguish "create new object" from "link to shared object":

```rust
/// A single parsed block from a chapter file.
pub struct ParsedBlock {
    // ... existing fields ...

    /// If present, this block is a reference to a shared object
    /// rather than a new inline definition.
    pub ref_path: Option<String>,
}
```

When `ref_path` is `Some("common/validate-output")`, the transform layer produces a `SharedRefDef` instead of a `StageDef`.

#### 13.13.4 Internal Models for guide_shared_refs

```rust
// crates/dapple-data/src/model/guides.rs (additions)

/// Row from rev.guide_shared_refs, used after INSERT.
pub(crate) struct InsertedGuideSharedRefModel {
    pub id: i64,
    pub object_id: Uuid,
    pub guide_id: i64,
    pub object_path_id: i64,
    pub ref_key: String,
    pub directive_type: String,
    pub created_at: OffsetDateTime,
}

/// Column-wise insert arguments for guide_shared_refs.
pub(crate) struct InsertGuideSharedRefArgs {
    pub guide_ids: Vec<i64>,
    pub object_path_ids: Vec<i64>,
    pub ref_keys: Vec<String>,
    pub directive_types: Vec<String>,
}

/// Public DTO for a shared reference within a guide response.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideSharedRef {
    pub object_id: Uuid,
    pub path_object_id: Uuid,
    pub ref_key: String,
    pub directive_type: String,
}
```

The `Guide` DTO gains a new field:

```rust
pub struct Guide {
    // ... existing fields from Section 12.11.5 ...
    pub shared_refs: Vec<GuideSharedRef>,
}
```

### 13.14 SQL Queries

#### 13.14.1 Resolve Shared Object Paths by ref_key

Used during guide ingest to look up shared object paths and validate scope visibility:

```sql
-- assets/sql/shared/select_shared_paths_by_ref_keys.sql
WITH args AS (
    SELECT *
    FROM UNNEST($1::TEXT[]) WITH ORDINALITY AS o(ref_key, ord)
)
SELECT
    op.id,
    op.object_id,
    op.entity_type_id,
    op.scope,
    op.ref_key,
    et.key AS entity_type_key,
    op.created_at
FROM args
JOIN rev.object_paths op ON op.ref_key = args.ref_key
JOIN rev.entity_types et ON et.id = op.entity_type_id
WHERE op.ref_key IS NOT NULL
ORDER BY args.ord;
```

#### 13.14.2 Insert Guide Shared Refs

```sql
-- assets/sql/guides/insert_guide_shared_refs.sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::BIGINT[],   -- guide_ids
        $2::BIGINT[],   -- object_path_ids
        $3::TEXT[],      -- ref_keys
        $4::TEXT[]       -- directive_types
    ) WITH ORDINALITY AS o(guide_id, object_path_id, ref_key, directive_type, ord)
),
objs AS (
    INSERT INTO rev.guide_shared_refs (guide_id, object_path_id, ref_key, directive_type)
    SELECT args.guide_id, args.object_path_id, args.ref_key, args.directive_type
    FROM args
    ORDER BY args.ord
    RETURNING id, object_id, guide_id, object_path_id, ref_key, directive_type, created_at
)
SELECT * FROM objs ORDER BY objs.object_id;
```

#### 13.14.3 Insert Shared Object Paths

Used during shared content ingest to create or find object paths for shared objects:

```sql
-- assets/sql/shared/insert_shared_object_paths.sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],      -- ref_keys
        $2::TEXT[],      -- scopes
        $3::SMALLINT[]   -- entity_type_ids
    ) WITH ORDINALITY AS o(ref_key, scope, entity_type_id, ord)
),
existing AS (
    SELECT op.id, op.object_id, op.ref_key, args.ord
    FROM args
    JOIN rev.object_paths op ON op.ref_key = args.ref_key
),
new_paths AS (
    INSERT INTO rev.object_paths (object_id, entity_type_id, scope, ref_key)
    SELECT
        uuidv7(),
        args.entity_type_id,
        args.scope,
        args.ref_key
    FROM args
    LEFT JOIN existing e ON e.ref_key = args.ref_key
    WHERE e.id IS NULL
    ORDER BY args.ord
    RETURNING id, object_id, ref_key
)
SELECT
    COALESCE(e.id, np.id) AS id,
    COALESCE(e.object_id, np.object_id) AS object_id,
    args.ref_key,
    CASE WHEN e.id IS NOT NULL THEN 'existing' ELSE 'created' END AS status
FROM args
LEFT JOIN existing e ON e.ref_key = args.ref_key
LEFT JOIN new_paths np ON np.ref_key = args.ref_key
ORDER BY args.ord;
```

#### 13.14.4 Resolve Latest Shared Version

Used at runtime to resolve a shared object path to its latest non-deprecated version:

```sql
-- assets/sql/shared/select_latest_shared_versions.sql
WITH args AS (
    SELECT *
    FROM UNNEST($1::BIGINT[]) WITH ORDINALITY AS o(path_id, ord)
)
SELECT
    vo.id AS versioned_object_id,
    vo.object_id AS versioned_object_object_id,
    vo.path_id,
    op.object_id AS path_object_id,
    op.ref_key,
    op.scope,
    vo.version_major,
    vo.version_minor,
    vo.version_patch
FROM args
JOIN rev.object_paths op ON op.id = args.path_id
JOIN LATERAL (
    SELECT v.*
    FROM rev.versioned_objects v
    WHERE v.path_id = args.path_id
      AND v.deprecation_level_id IS NULL
    ORDER BY v.version_major DESC, v.version_minor DESC, v.version_patch DESC
    LIMIT 1
) vo ON TRUE
ORDER BY args.ord;
```

### 13.15 What Gets Shared (Primary Use Cases)

| Content Type | Directive | Shared Directory | Example |
|-------------|-----------|-----------------|---------|
| Utility stages | `:::stage` | `stages/` | `common/validate-output` |
| Utility steps | `:::step` (within shared stages) | `stages/` | Steps within `validate-output.md` |
| Tasks | `:::nushell` | `stages/` (inline) | Nushell scripts within shared stages |
| Inference profiles | `:::inference-profile` | `profiles/` | `common/summarizer` |
| Fluent terms | `-term-name = ...` in `.ftl` | `locales/` | `-brand-name = Dapple` |
| Fluent messages | `message-key = ...` in `.ftl` | `locales/` | Common disclaimers, boilerplate |
| Assets | Binary files | `assets/` | `logo.svg`, brand images, icons |

### 13.16 Shared Fluent Terms

Shared Fluent terms receive special treatment. Terms defined in `_shared/common/locales/en.ftl` (e.g., `-brand-name = Dapple`) are available to ALL guide `.ftl` files regardless of the guide's location tier -- because `common` is visible to every guide.

**How it works:**

1. During shared content ingest, terms (identifiers prefixed with `-`) are extracted from `locales/*.ftl` files and stored as FluentMessage entities with a naming convention: the term name becomes the message key (e.g., `-brand-name`).
2. During guide ingest, when `.ftl` files are parsed, any term references (`{ -brand-name }`) are resolved by looking up the shared FluentMessage. If the referenced term comes from a shared scope the guide can see, the reference is valid.
3. **Resolution strategy:** Shared terms are resolved at guide ingest time (parse-time inlining). The resolved value is substituted into guide message bodies before storage. This means guide FluentMessages are self-contained after ingest -- no runtime term resolution is needed. This aligns with the recommendation in Section 12.19, question 1.

**Example:**

`_shared/common/locales/en.ftl`:
```ftl
-brand-name = Dapple
-support-email = support@dapple.dev
-disclaimer = This content is provided by { -brand-name } for informational purposes.
```

`public/consumer/locales/en.ftl`:
```ftl
essay-helper-pack =
    The { -brand-name } Essay Helper guide walks users through
    the complete essay writing process.
```

After ingest-time resolution, the stored FluentMessage body for `essay-helper-pack` becomes:
```
The Dapple Essay Helper guide walks users through
the complete essay writing process.
```

### 13.17 Revised Ingest Pipeline (Guide Ingest with Shared Refs)

The guide ingest pipeline (Section 12.12) gains additional steps for shared reference handling. Modified steps are marked with `[SHARED]`.

```
1. Extract tarball
2. Parse guide.toml
3. Parse SUMMARY.md
4. Parse chapter .md files
   -> For each directive with ref attribute, record SharedRefDef     [SHARED]
   -> For each directive without ref, record as guide-local definition
5. Discover locales/*.ftl files -> parse into FluentMessageDef per locale
6. Validate:
   a. Every guide-local directive name has a matching key in at least one .ftl file
   b. Every ref path has a valid scope prefix                         [SHARED]
   c. Every ref path's scope is visible to this guide's location tier [SHARED]
7. Plan ingest (transform ParsedGuide -> IngestPlan)
   -> IngestPlan.shared_refs populated from SharedRefDef list         [SHARED]
8. Begin transaction
9. Resolve shared references:                                         [SHARED]
   a. Look up object_path_ids for all ref_keys via
      select_shared_paths_by_ref_keys.sql
   b. Validate all ref_keys resolved (missing = error, rollback)
   c. Validate scope visibility (wrong scope = error, rollback)
   d. Resolve shared Fluent terms for inlining into guide .ftl bodies [SHARED]
10. Create FluentMessage paths + versions for guide-local block names
11. Create FluentMessage localized bodies (with shared terms inlined)  [SHARED]
12. Upload assets
13. Materialize guide-local derived objects (playbook, tasks, etc.)
    -> Playbook includes shared stages by reference (path_id link)    [SHARED]
14. Create guide entity with chapters and blocks
    -> Blocks with ref_path store the shared object's fluent_message_id [SHARED]
15. Create guide_assets associations
16. Create guide_cards
17. Create guide_shared_refs rows                                      [SHARED]
18. Commit transaction
```

### 13.18 Impact on Existing Workstreams

#### Workstream 1 (Parser Modernization)

- **Recognize `ref` attribute** on directives. When present, the block is a shared reference.
- **`ParsedBlock` gains `ref_path: Option<String>`** field. When `Some`, the block content is ignored and the ref path is recorded.
- **Validate `ref` and `name` are mutually exclusive.** A directive with both is a parse error.
- **Add `parse_shared_library()` entry point** to the parser for parsing `_shared/` directories. This parallels `parse_guide()` but produces `ParsedSharedLibrary` instead of `ParsedGuide`.
- **New `ParseError` variants:**
  ```rust
  #[error("Directive has both 'ref' and 'name' attributes at {location}")]
  RefAndNameConflict { location: String },

  #[error("shared.toml not found in shared directory: {0}")]
  MissingSharedToml(String),

  #[error("Invalid shared.toml: {detail}")]
  InvalidSharedToml { detail: String },
  ```

#### Workstream 2 (Tarball Ingest)

- **`IngestPlan` gains `shared_refs: Vec<SharedRefDef>`** (Section 13.13.2).
- **Guide ingest handler** resolves `ref_key` values to `object_path_id` via CTE lookup (Section 13.14.1). Scope visibility validation occurs within the same transaction.
- **Shared term inlining** happens before FluentMessage body creation: shared `.ftl` terms are fetched, and guide `.ftl` bodies are preprocessed to replace `{ -term-name }` references with resolved values.
- **New handler:** `shared_ingest_post_handler` for the `POST /api/shared/ingest` endpoint (Section 13.10).
- **Playbook materialization** must handle stages that are shared references. A shared stage is not re-created -- instead, the playbook's stage list includes the shared stage by linking to its `object_path_id`.

#### Section 12 (Fluent-Native)

- **Shared `.ftl` files provide terms and messages** that guide-specific `.ftl` files can reference. The term resolution strategy (parse-time inlining, as recommended in Section 12.19) is formalized.
- **Term naming convention:** Shared terms use the standard Fluent term syntax (`-term-name`). Guide-local terms are rare -- prefer shared terms for any vocabulary that appears in more than one guide.

### 13.19 Migration Ordering

| Order | Migration File | Depends On |
|-------|---------------|------------|
| 1 | `20260217000001_object_paths_scope.sql` | `rev.object_paths` |
| 2 | `20260217000002_object_paths_ref_key.sql` | `rev.object_paths` |
| 3 | `20260217000003_shared_scopes.sql` | (standalone) |
| 4 | `20260217000004_guide_shared_refs.sql` | `rev.guides`, `rev.object_paths` |

Migrations 1 and 2 are `ALTER TABLE` additions and can be applied independently. Migration 3 is a standalone lookup table. Migration 4 depends on `rev.guides` existing (from the `20260214` series) and on `rev.object_paths` having the `scope` and `ref_key` columns.

### 13.20 Testing Strategy

#### 13.20.1 Parser Tests (Unit)

```rust
#[test]
fn test_parse_block_with_ref_attribute() {
    let md = r#":::stage { ref: "common/validate-output" }
:::"#;
    let blocks = parse_blocks(md);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].ref_path, Some("common/validate-output".to_string()));
    assert!(blocks[0].name.is_none());
}

#[test]
fn test_parse_block_ref_and_name_conflict() {
    let md = r#":::stage { ref: "common/validate-output", name: "my-stage" }
:::"#;
    let result = parse_blocks_result(md);
    assert!(result.is_err());
    // Expect RefAndNameConflict error
}

#[test]
fn test_parse_shared_library() {
    // Create temp dir with shared.toml, SUMMARY.md, stages/, profiles/, locales/
    // Verify ParsedSharedLibrary has correct stages and profiles
}

#[test]
fn test_parse_shared_toml() {
    let toml = r#"
name = "common"
description = "Shared utility stages"
scope = "common"
"#;
    let result = parse_shared_toml(toml).unwrap();
    assert_eq!(result.scope, "common");
}
```

#### 13.20.2 Integration Tests

```rust
fn trial_ingest_shared_common() -> Trial {
    // 1. Create tarball from test _shared/common/ directory
    // 2. POST /api/shared/ingest?scope=common
    // 3. Verify shared objects were created with correct ref_keys
    // 4. Verify FluentMessages were created for shared content
    // 5. Register "init-shared-common" batch
}

fn trial_ingest_guide_with_shared_refs() -> Trial {
    // Wait for: init-shared-common
    // 1. Create a guide tarball with :::stage { ref: "common/validate-output" }
    // 2. POST /api/guides/ingest
    // 3. Verify guide was created
    // 4. Verify guide_shared_refs rows exist
    // 5. Verify shared stage is linked by path, not duplicated
    // 6. Register "init-guides-with-shared" batch
}

fn trial_ingest_guide_scope_violation() -> Trial {
    // Wait for: init-shared-common
    // 1. Create a guide at public/* with :::stage { ref: "internal/admin-review" }
    // 2. POST /api/guides/ingest
    // 3. Verify 422 response with scope visibility error
}

fn trial_update_shared_propagates() -> Trial {
    // Wait for: init-guides-with-shared
    // 1. Re-ingest _shared/common/ with updated content
    // 2. Verify shared objects are now version 0.0.2
    // 3. Fetch guide that references shared content
    // 4. Verify guide still links to same path_object_id
    // 5. Verify runtime resolution returns updated content
}
```

#### 13.20.3 Test Fixture

```
tests/fixtures/shared-common/
  shared.toml
  SUMMARY.md
  stages/
    validate-output.md    # :::stage + :::step definitions
  profiles/
    summarizer.md         # :::inference-profile definition
  locales/
    en.ftl                # English messages for shared content

tests/fixtures/guide-with-refs/
  guide.toml
  SUMMARY.md
  workflow.md             # Contains :::stage { ref: "common/validate-output" }
  locales/
    en.ftl
```

### 13.21 Files to Create/Modify

| Action | File | Notes |
|--------|------|-------|
| Create | Migration `20260217000001_object_paths_scope.sql` | Add scope column to object_paths |
| Create | Migration `20260217000002_object_paths_ref_key.sql` | Add ref_key column to object_paths |
| Create | Migration `20260217000003_shared_scopes.sql` | Shared scopes lookup table with seed data |
| Create | Migration `20260217000004_guide_shared_refs.sql` | Guide-to-shared-object associative table |
| Create | `assets/sql/shared/select_shared_paths_by_ref_keys.sql` | Resolve ref_keys to object_path_ids |
| Create | `assets/sql/shared/insert_shared_object_paths.sql` | Create or find shared object paths |
| Create | `assets/sql/shared/select_latest_shared_versions.sql` | Resolve latest non-deprecated version |
| Create | `assets/sql/guides/insert_guide_shared_refs.sql` | Insert guide-to-shared-object links |
| Create | `crates/dapple-parser/src/shared.rs` | `parse_shared_library()` entry point, SharedToml parsing |
| Create | `crates/dapple-parser/src/transform/shared.rs` | Transform ParsedSharedLibrary to shared IngestPlan |
| Create | `crates/dapple-prompt-api/src/handler/shared_ingest.rs` | Shared content ingest handler |
| Create | `crates/dapple-prompt-api/src/schema/shared_ingest.rs` | Request/response schemas |
| Create | `crates/dapple-data/src/query/shared.rs` | Query functions for shared object paths |
| Create | `tests/fixtures/shared-common/` | Test fixture for shared content |
| Create | `tests/fixtures/guide-with-refs/` | Test fixture for guide with shared refs |
| Modify | `crates/dapple-parser/src/types.rs` | Add `ref_path` to `ParsedBlock` |
| Modify | `crates/dapple-parser/src/error.rs` | Add `RefAndNameConflict`, `MissingSharedToml`, `InvalidSharedToml` |
| Modify | `crates/dapple-parser/src/lib.rs` | Add `parse_shared_library()` entry point |
| Modify | `crates/dapple-parser/src/transform/mod.rs` | Add `SharedRefDef` and `shared_refs` to `IngestPlan` |
| Modify | `crates/dapple-prompt-api/src/handler/guide_ingest.rs` | Add shared ref resolution and validation |
| Modify | `crates/dapple-prompt-api/src/route/guides.rs` | Add `/shared/ingest` route |
| Modify | `crates/dapple-data/src/model/guides.rs` | Add `GuideSharedRef`, `InsertGuideSharedRefArgs` |
| Modify | `crates/dapple-data/src/query/guides.rs` | Add shared ref insert/select functions |
| Modify | `crates/dapple-prompt-api/src/schema/guides.rs` | Add `shared_refs` to Guide DTO |

### 13.22 Open Questions and Risks

#### Open Questions

1. **Shared stage composition in playbooks** -- When a guide's playbook includes a shared stage via `ref`, how is the stage linked? Options: (a) the playbook's stage list contains a `path_id` reference to the shared stage's object path, and the runtime resolves the latest version; (b) the playbook copies the shared stage's structure at ingest time, creating a snapshot. Option (a) preserves the "automatic propagation" benefit. Option (b) is simpler but loses propagation. Recommendation: option (a) -- path-level linking.

2. **Shared content versioning granularity** -- When `_shared/common/` is re-ingested, should all objects in the library receive a new version, or only the objects whose content changed? Content-aware versioning (only version changed objects) is more efficient but adds complexity (diff detection). Recommendation: version all objects in the library on each ingest for simplicity. Content-hash dedup can be added later.

3. **Cross-scope shared references** -- Can a shared library in `_shared/internal/` reference objects from `_shared/common/`? This would be useful for building on common utilities. Recommendation: yes, following the same scope visibility rules. Internal scope can see common. The ingest pipeline validates this.

4. **Partner scope lifecycle** -- When a partner is onboarded, who creates the `_shared/partners/{uuid}/` directory and inserts the `shared_scopes` row? Recommendation: defer partner scope to a future workstream. The `rev.shared_scopes` table and `scope` column support the `partner:{uuid}` pattern, but no partner-specific logic is implemented in this phase.

5. **Shared content deprecation** -- When a shared object is deprecated, should guides that reference it receive a warning on their next GET response? Recommendation: yes -- add a `deprecated_refs: Vec<String>` field to the Guide DTO that lists any shared ref_keys whose latest version is deprecated.

#### Risks

1. **Orphaned references** -- If a shared object is removed (all versions deprecated or path deleted), guides that reference it will fail to resolve at runtime. Mitigation: deprecation levels already exist in the versioning infrastructure. Shared objects should be deprecated, not deleted. The runtime resolution query (Section 13.14.4) returns no results for fully-deprecated paths, and the handler should provide a graceful fallback or warning.

2. **Circular dependencies between shared libraries** -- If `_shared/internal/` references `_shared/common/` and vice versa, ingest ordering becomes critical. Mitigation: the scope hierarchy is strictly layered (`common` is the base; `internal` builds on `common`; `partner` builds on `common`). References from `common` to `internal` are not permitted -- the ingest pipeline validates this.

3. **Ingest ordering** -- Shared content must be ingested before guides that reference it. If a guide references `common/validate-output` but `_shared/common/` has not been ingested, the guide ingest fails. Mitigation: clear documentation and error messages. The dry-run mode should catch missing shared references before any database writes.

4. **Term collision** -- If a guide defines a local Fluent term `-brand-name` and a shared library also defines `-brand-name`, which takes precedence? Recommendation: shared terms take precedence. The guide ingest pipeline should emit a warning if a guide-local term shadows a shared term.

5. **Migration impact on existing object_paths** -- Adding `scope` and `ref_key` columns to `rev.object_paths` adds nullable columns. Existing rows (all guide-private) will have `NULL` for both columns. No data migration is needed, but queries that filter on scope must handle `NULL` correctly (using `WHERE scope IS NOT NULL` or similar).

---

## 14. Addendum: Dapple SDK Crate & Code Generation

**Added:** 2026-02-14

This addendum defines the transformation of the `dapple` crate (currently a placeholder in the dapple-prompt workspace) into the canonical Rust SDK for interacting with dapple-prompt-api. The SDK absorbs the code generation tooling and typed client infrastructure that currently lives in the dapple-suite repository, making it the single source of truth for API types, HTTP client logic, and high-level operation builders.

### 14.1 Motivation

The dapple-suite repository at `/home/kenny/src/dapple/dapple-suite/` maintains its own code generation pipeline and typed client:

- `tools/generate-models/` -- A standalone binary that fetches the dapple-prompt-api OpenAPI spec and produces ~6,000 lines of strongly-typed Rust DTOs into `packages/api/src/models_generated.rs` via the `typify` crate.
- `packages/api/` -- A crate providing `DappleClient` (typed HTTP client with batch semantics), hand-written complement types (`models.rs` for batch wrappers, tagged unions, status enums), and domain-specific endpoint methods (`endpoints/*.rs`).

All dapple-suite applications (web, desktop, mobile, TUI) consume this crate. The arrangement works for a single consuming repository, but with the guide evolution -- tarball ingest, shared content references, Fluent message creation with locale/engine bodies, scoped visibility -- operations are becoming complex enough that non-dapple-suite consumers should not have to recreate this logic. Third-party integrations, CLI tools, and automation scripts all need the same typed access.

**Problems with the current arrangement:**

1. **Duplicated source of truth** -- The OpenAPI spec is generated from dapple-prompt-api, but the typed client and code gen live in a separate repository. Schema changes require coordinated updates across repos.
2. **No SDK for external consumers** -- Any Rust project that wants to interact with dapple-prompt-api must either vendor the `packages/api/` crate from dapple-suite or build its own client from raw HTTP and JSON.
3. **Complex workflows are inaccessible** -- Guide ingest (tarball construction + multipart upload), shared content ingest (scoped tarball upload), and Fluent message creation (locale/engine body composition) require deep knowledge of API conventions. Builders should encapsulate this.
4. **The `dapple` crate is empty** -- The workspace already contains `crates/dapple/` with a placeholder `lib.rs`. It is the natural home for an SDK.

### 14.2 Core Decision

The `dapple` crate in the dapple-prompt workspace becomes the official Rust SDK for interacting with dapple-prompt-api. It provides:

1. **Generated models from the OpenAPI spec** via `typify`, using the same proven approach as dapple-suite's `tools/generate-models/`.
2. **Hand-written complement types** for concepts the spec cannot fully express (batch wrappers, internally-tagged unions, status enums).
3. **A typed HTTP client** with batch semantics, authentication, and error handling.
4. **Endpoint methods** organized by domain (guides, playbooks, journeys, fluent messages, etc.).
5. **High-level operation builders** for complex multi-step workflows (guide ingest, shared content ingest, Fluent message composition, playbook assembly).

The `dapple` crate does NOT depend on `dapple-data` or `dapple-prompt-api` at runtime -- it is a pure client-side SDK that talks to the API over HTTP. The only build-time dependency is on the OpenAPI spec (a JSON file) for code generation.

### 14.3 Module Organization

```
crates/dapple/
  Cargo.toml
  src/
    lib.rs                      # Re-exports, crate-level docs
    models/
      mod.rs                    # Re-export generated + hand-written models
      generated.rs              # Auto-generated from OpenAPI spec (via typify)
      complement.rs             # Hand-written: batch wrappers, tagged unions, status enums
    client/
      mod.rs                    # DappleClient struct, auth, config
      batch.rs                  # BatchRequest/BatchResponse semantics
      error.rs                  # ApiError, ApiResult
    endpoints/
      mod.rs                    # Re-exports all endpoint modules
      guides.rs                 # Guide CRUD + ingest operations
      shared.rs                 # Shared content ingest
      journeys.rs               # Journey operations
      playbooks.rs              # Playbook operations
      fluent.rs                 # Fluent message operations
      assets.rs                 # Asset upload/download (draft/chunk/finalize)
      briefs.rs                 # Brief, BriefGroup, BriefPack operations
      inference.rs              # Inference profile operations
      render.rs                 # PDF render operations
      dispatches.rs             # Dispatch operations
      bindings.rs               # Intent binding operations
      tokens.rs                 # API token management
    builders/
      mod.rs                    # Re-exports all builders
      fluent_message.rs         # FluentMessageBuilder
      guide_ingest.rs           # GuideIngestBuilder (tarball construction)
      shared_ingest.rs          # SharedContentIngestBuilder
      playbook.rs               # PlaybookBuilder with shared refs
  bin/
    generate.rs                 # Binary: cargo run -p dapple --bin generate
```

### 14.4 Key Type Sketches

#### 14.4.1 DappleClient

The client migrates from `dapple-suite/packages/api/src/client.rs` with the same public interface:

```rust
// crates/dapple/src/client/mod.rs

/// Client for interacting with the dapple-prompt-api.
#[derive(Debug, Clone)]
pub struct DappleClient {
    base_url: String,
    auth: AuthState,
    client: reqwest::Client,
}

impl DappleClient {
    /// Create a new client with the given base URL.
    pub fn new(base_url: impl Into<String>) -> Self { ... }

    /// Set a bearer token for authentication.
    pub fn with_token(self, token: impl Into<String>) -> Self { ... }

    /// Set development claims for local testing.
    pub fn with_dev_claims(self, claims: DevClaims) -> Self { ... }

    /// Set a JWT obtained from authentication.
    pub fn with_jwt(self, jwt: impl Into<String>, expires_at: DateTime<Utc>, signer: Option<String>) -> Self { ... }

    /// Exchange a signed auth token for a JWT and store it in the client.
    pub async fn authenticate(&mut self, token: &SignedAuthToken, user_id: impl Into<String>) -> ApiResult<()> { ... }

    /// Perform a GET request with batch semantics.
    pub async fn get_batch<T: DeserializeOwned>(&self, endpoint: &str) -> ApiResult<BatchResponse<T>> { ... }

    /// Perform a GET request with query parameters.
    pub async fn get_batch_with_query<T: DeserializeOwned>(
        &self, endpoint: &str, query: &str, params: &str, options: Option<QueryOptions>,
    ) -> ApiResult<BatchResponse<T>> { ... }

    /// Perform a POST request with batch semantics.
    pub async fn post_batch<T: Serialize, R: DeserializeOwned>(
        &self, endpoint: &str, data: BatchRequest<T>,
    ) -> ApiResult<BatchResponse<R>> { ... }

    /// Perform a PATCH request with batch semantics.
    pub async fn patch_batch<T: Serialize, R: DeserializeOwned>(
        &self, endpoint: &str, data: BatchRequest<T>,
    ) -> ApiResult<BatchResponse<R>> { ... }

    /// Perform a multipart POST (for tarball ingest endpoints).
    pub async fn post_multipart<R: DeserializeOwned>(
        &self, endpoint: &str, form: reqwest::multipart::Form,
    ) -> ApiResult<R> { ... }
}
```

The `post_multipart` method is new -- it supports the tarball ingest endpoints introduced in Sections 3 and 13.

#### 14.4.2 Complement Types

Migrated from `dapple-suite/packages/api/src/models.rs`:

```rust
// crates/dapple/src/models/complement.rs

/// Batch request wrapper -- all POST/PATCH requests use this format.
#[derive(Debug, Clone, Serialize)]
pub struct BatchRequest<T> {
    pub objs: Vec<T>,
}

/// Batch response wrapper for list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResponse<T> {
    pub status: String,
    pub results: usize,
    pub objs: Vec<T>,
}

/// Task execution status (the spec uses strings; we keep enums for safety).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Queued, Running, Ok, Failed, Canceled,
}

/// Brief unit subtype -- internally tagged by "kind".
/// typify generates #[serde(untagged)] for these, so hand-written
/// versions with proper internal tagging are required.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BriefUnitSubtype {
    Markdown { ... },
    Notice { ... },
    Input { ... },
    Media { ... },
    None,
}

// ... QueryOptions, ErrorResponse, AuthErrorCode, PromptEvent, etc.
// (Full list migrated from dapple-suite/packages/api/src/models.rs)
```

#### 14.4.3 Endpoint Methods

Each endpoint module implements typed methods on `DappleClient`:

```rust
// crates/dapple/src/endpoints/guides.rs

use crate::client::DappleClient;
use crate::models::{BatchRequest, BatchResponse, ApiResult};
use crate::models::generated::{Guide, CreateGuide};

impl DappleClient {
    /// Create one or more guides.
    pub async fn create_guides(&self, guides: Vec<CreateGuide>) -> ApiResult<BatchResponse<Guide>> {
        self.post_batch("guides/", BatchRequest::new(guides)).await
    }

    /// Get guides by object IDs.
    pub async fn get_guides_by_ids(&self, ids: &[&str]) -> ApiResult<BatchResponse<Guide>> {
        let ids_param = ids.join(",");
        self.get_batch_with_query("guides/", "by_object_ids", &format!("ids={}", ids_param), None).await
    }

    /// Ingest a guide from a tarball.
    pub async fn ingest_guide(&self, tarball: Vec<u8>, dry_run: bool) -> ApiResult<IngestGuideResponse> {
        let form = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(tarball)
                .file_name("guide.tar.gz")
                .mime_str("application/gzip")
                .unwrap());
        let endpoint = format!("guides/ingest?dry_run={}", dry_run);
        self.post_multipart(&endpoint, form).await
    }
}
```

```rust
// crates/dapple/src/endpoints/fluent.rs

impl DappleClient {
    /// Create Fluent messages with localized bodies.
    pub async fn create_fluent_messages(
        &self, messages: Vec<CreateFluentMessage>,
    ) -> ApiResult<BatchResponse<FluentMessage>> {
        self.post_batch("fluent-messages/", BatchRequest::new(messages)).await
    }

    /// Add localized bodies to an existing Fluent message.
    pub async fn add_fluent_message_bodies(
        &self, message_object_id: &str, bodies: Vec<CreateFluentMessageBody>,
    ) -> ApiResult<BatchResponse<FluentMessageBody>> {
        let endpoint = format!("fluent-messages/{}/bodies/", message_object_id);
        self.post_batch(&endpoint, BatchRequest::new(bodies)).await
    }
}
```

#### 14.4.4 FluentMessageBuilder

Simplifies the multi-step process of creating a Fluent message with locale bodies and optional engine bodies:

```rust
// crates/dapple/src/builders/fluent_message.rs

/// Builder for composing FluentMessage creation with localized and engine bodies.
pub struct FluentMessageBuilder {
    name: String,
    description: Option<String>,
    locale_bodies: Vec<(String, String)>,          // (locale, body_text)
    engine_bodies: Vec<(String, String, String, serde_json::Value)>, // (maker, model, release, body)
}

impl FluentMessageBuilder {
    /// Start building a Fluent message with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            locale_bodies: Vec::new(),
            engine_bodies: Vec::new(),
        }
    }

    /// Set the message description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a localized body for the given locale.
    pub fn locale(mut self, locale: impl Into<String>, body: impl Into<String>) -> Self {
        self.locale_bodies.push((locale.into(), body.into()));
        self
    }

    /// Add an engine-specific body for a model release.
    pub fn engine_body(
        mut self,
        maker: impl Into<String>,
        model: impl Into<String>,
        release: impl Into<String>,
        body: serde_json::Value,
    ) -> Self {
        self.engine_bodies.push((maker.into(), model.into(), release.into(), body));
        self
    }

    /// Build and send the Fluent message creation request.
    pub async fn send(self, client: &DappleClient) -> ApiResult<FluentMessage> {
        // 1. Create the FluentMessage path + version
        let create_schema = CreateFluentMessage {
            name: self.name,
            description: self.description,
        };
        let response = client.create_fluent_messages(vec![create_schema]).await?;
        let message = response.objs.into_iter().next()
            .ok_or_else(|| ApiError::Api { message: "No message returned".into() })?;

        // 2. Add localized bodies
        if !self.locale_bodies.is_empty() {
            let bodies: Vec<CreateFluentMessageBody> = self.locale_bodies.into_iter()
                .map(|(locale, body)| CreateFluentMessageBody { locale, body })
                .collect();
            client.add_fluent_message_bodies(&message.object_id, bodies).await?;
        }

        // 3. Add engine bodies (if any)
        if !self.engine_bodies.is_empty() {
            let engine_bodies: Vec<CreateFluentMessageEngineBody> = self.engine_bodies.into_iter()
                .map(|(maker, model, release, body)| CreateFluentMessageEngineBody {
                    maker_key: maker,
                    model_key: model,
                    model_release_key: release,
                    body,
                })
                .collect();
            client.add_fluent_message_engine_bodies(&message.object_id, engine_bodies).await?;
        }

        Ok(message)
    }
}
```

**Usage:**

```rust
dapple::builders::FluentMessageBuilder::new("brainstorm-topics")
    .description("Generate a list of potential essay topics")
    .locale("en", "Generate a list of potential essay topics based on the user's interests.")
    .locale("pt", "Gere uma lista de topicos potenciais com base nos interesses do usuario.")
    .locale("pt-BR", "Gere uma lista de topicos potenciais para a redacao.")
    .engine_body("anthropic", "claude-3-5", "sonnet-20241022", custom_body)
    .send(&client)
    .await?;
```

#### 14.4.5 GuideIngestBuilder

Wraps tarball construction and multipart upload into a single fluent API:

```rust
// crates/dapple/src/builders/guide_ingest.rs

use std::path::Path;

/// Builder for guide ingest operations.
///
/// Constructs a .tar.gz archive from a guide directory and uploads it
/// via `POST /api/guides/ingest`.
pub struct GuideIngestBuilder {
    source_dir: PathBuf,
    source_path: Option<String>,
    dry_run: bool,
}

impl GuideIngestBuilder {
    /// Start building an ingest operation from a guide directory.
    pub fn new(source_dir: impl AsRef<Path>) -> Self {
        Self {
            source_dir: source_dir.as_ref().to_path_buf(),
            source_path: None,
            dry_run: false,
        }
    }

    /// Set the logical source path stored in the guide entity
    /// (e.g., "public/consumer").
    pub fn source_path(mut self, path: impl Into<String>) -> Self {
        self.source_path = Some(path.into());
        self
    }

    /// Enable dry-run mode (validate without persisting).
    pub fn dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Build the tarball and send the ingest request.
    pub async fn send(self, client: &DappleClient) -> ApiResult<IngestGuideResponse> {
        // 1. Create tarball from source directory
        let tarball = self.create_tarball()?;

        // 2. Build multipart form
        let mut form = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(tarball)
                .file_name("guide.tar.gz")
                .mime_str("application/gzip")
                .unwrap());

        // 3. Add source_path field if specified
        if let Some(ref sp) = self.source_path {
            form = form.text("source_path", sp.clone());
        }

        // 4. Send
        let endpoint = format!("guides/ingest?dry_run={}", self.dry_run);
        client.post_multipart(&endpoint, form).await
    }

    /// Create a .tar.gz archive from the source directory.
    fn create_tarball(&self) -> Result<Vec<u8>, ApiError> {
        let buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(buf, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);

        // Follow symlinks (--dereference) per Section 7.4
        archive.follow_symlinks(true);
        archive.append_dir_all(".", &self.source_dir)
            .map_err(|e| ApiError::Api { message: format!("Failed to create tarball: {}", e) })?;

        let encoder = archive.into_inner()
            .map_err(|e| ApiError::Api { message: format!("Failed to finalize tarball: {}", e) })?;
        let bytes = encoder.finish()
            .map_err(|e| ApiError::Api { message: format!("Failed to compress tarball: {}", e) })?;

        Ok(bytes)
    }
}
```

**Usage:**

```rust
dapple::builders::GuideIngestBuilder::new("./public/consumer/")
    .source_path("public/consumer")
    .dry_run(true)
    .send(&client)
    .await?;
```

#### 14.4.6 SharedContentIngestBuilder

Wraps scoped shared content tarball upload:

```rust
// crates/dapple/src/builders/shared_ingest.rs

/// Builder for shared content ingest operations.
pub struct SharedContentIngestBuilder {
    source_dir: PathBuf,
    scope: String,
    partner_id: Option<Uuid>,
    dry_run: bool,
}

impl SharedContentIngestBuilder {
    /// Start building a shared content ingest from a directory.
    pub fn new(source_dir: impl AsRef<Path>, scope: impl Into<String>) -> Self {
        Self {
            source_dir: source_dir.as_ref().to_path_buf(),
            scope: scope.into(),
            partner_id: None,
            dry_run: false,
        }
    }

    /// Set the partner ID (required when scope is "partner").
    pub fn partner_id(mut self, id: Uuid) -> Self {
        self.partner_id = Some(id);
        self
    }

    /// Enable dry-run mode.
    pub fn dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Build the tarball and send the ingest request.
    pub async fn send(self, client: &DappleClient) -> ApiResult<SharedIngestResponse> {
        let tarball = self.create_tarball()?;
        let form = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(tarball)
                .file_name("shared.tar.gz")
                .mime_str("application/gzip")
                .unwrap());

        let mut endpoint = format!("shared/ingest?scope={}&dry_run={}", self.scope, self.dry_run);
        if let Some(pid) = self.partner_id {
            endpoint.push_str(&format!("&partner_id={}", pid));
        }

        client.post_multipart(&endpoint, form).await
    }

    fn create_tarball(&self) -> Result<Vec<u8>, ApiError> {
        // Same tarball construction as GuideIngestBuilder
        // (extract to a shared utility in builders/mod.rs)
        let buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(buf, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        archive.follow_symlinks(true);
        archive.append_dir_all(".", &self.source_dir)
            .map_err(|e| ApiError::Api { message: format!("Failed to create tarball: {}", e) })?;
        let encoder = archive.into_inner()
            .map_err(|e| ApiError::Api { message: format!("Failed to finalize tarball: {}", e) })?;
        encoder.finish()
            .map_err(|e| ApiError::Api { message: format!("Failed to compress tarball: {}", e) })
    }
}
```

**Usage:**

```rust
dapple::builders::SharedContentIngestBuilder::new("_shared/common/", "common")
    .dry_run(false)
    .send(&client)
    .await?;
```

#### 14.4.7 PlaybookBuilder

Composes playbooks with both inline stages and shared references:

```rust
// crates/dapple/src/builders/playbook.rs

/// Builder for composing playbooks with inline and shared stages.
pub struct PlaybookBuilder {
    name: String,
    description: Option<String>,
    stages: Vec<PlaybookStageEntry>,
    category_object_ids: Vec<String>,
}

enum PlaybookStageEntry {
    /// An inline stage with steps defined here.
    Inline {
        name: String,
        steps: Vec<PlaybookStepEntry>,
    },
    /// A shared stage referenced by ref_key.
    Shared {
        ref_key: String,
    },
}

struct PlaybookStepEntry {
    name: String,
    task_object_ids: Vec<String>,
}

impl PlaybookBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            stages: Vec::new(),
            category_object_ids: Vec::new(),
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add an inline stage with a closure that defines its steps.
    pub fn stage(mut self, name: impl Into<String>, builder: impl FnOnce(StageBuilder) -> StageBuilder) -> Self {
        let sb = builder(StageBuilder::new(name));
        self.stages.push(PlaybookStageEntry::Inline {
            name: sb.name,
            steps: sb.steps,
        });
        self
    }

    /// Add a shared stage by reference key (e.g., "common/validate-output").
    pub fn shared_stage(mut self, ref_key: impl Into<String>) -> Self {
        self.stages.push(PlaybookStageEntry::Shared { ref_key: ref_key.into() });
        self
    }

    /// Build the CreatePlaybook request and send it.
    pub async fn send(self, client: &DappleClient) -> ApiResult<Playbook> { ... }
}

/// Sub-builder for defining steps within an inline stage.
pub struct StageBuilder {
    name: String,
    steps: Vec<PlaybookStepEntry>,
}

impl StageBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), steps: Vec::new() }
    }

    pub fn step(mut self, name: impl Into<String>) -> Self {
        self.steps.push(PlaybookStepEntry {
            name: name.into(),
            task_object_ids: Vec::new(),
        });
        self
    }
}
```

**Usage:**

```rust
dapple::builders::PlaybookBuilder::new("essay-helper")
    .description("Walk users through the essay writing process")
    .stage("Topic Selection", |s| s
        .step("Brainstorm Topics")
        .step("Narrow Focus"))
    .stage("Outlining", |s| s
        .step("Build Outline")
        .step("Source Research"))
    .shared_stage("common/validate-output")
    .shared_stage("common/summarize-and-review")
    .send(&client)
    .await?;
```

### 14.5 The Generate Binary

Code generation is implemented as a binary target in the `dapple` crate. It fetches the OpenAPI spec from a running API instance or a local file and produces `src/models/generated.rs`.

```rust
// crates/dapple/bin/generate.rs

//! Code generator for dapple SDK models from the OpenAPI spec.
//!
//! Usage:
//!   cargo run -p dapple --bin generate -- --url https://localhost:8080
//!   cargo run -p dapple --bin generate -- --file ./api.json
//!   cargo run -p dapple --bin generate -- --url https://localhost:8080 --output src/models/generated.rs

use std::path::PathBuf;

struct Args {
    /// URL of a running dapple-prompt-api instance.
    url: Option<String>,
    /// Path to a local OpenAPI JSON file.
    file: Option<PathBuf>,
    /// Output path for the generated Rust file.
    output: PathBuf,
}

fn main() {
    // 1. Parse args (url or file, output path)
    // 2. Fetch or read the OpenAPI spec JSON
    // 3. Parse as schemars JSON Schema
    // 4. Feed to typify::TypeSpace to generate Rust types
    // 5. Format with prettyplease
    // 6. Write to output path
}
```

**CLI interface:**

```bash
# Generate from running server (fetches /api/api.json)
cargo run -p dapple --bin generate -- --url https://localhost:8080

# Generate from local spec file
cargo run -p dapple --bin generate -- --file ./api.json

# Output to custom path
cargo run -p dapple --bin generate -- --url https://localhost:8080 --output src/models/generated.rs
```

**Why a binary instead of build.rs:** A `build.rs` approach would require a running API server (or a checked-in spec file) at every `cargo build`. The binary target is run explicitly when the API schema changes, and the generated file is committed to version control. This is the same approach used by dapple-suite's `tools/generate-models/` and avoids surprises in CI/CD.

**Why not a proc-macro:** Proc-macros run at compile time and would impose the same "need the spec at build time" constraint. They also make IDE support harder and debugging more opaque. A committed generated file is transparent and auditable.

### 14.6 Dependencies

```toml
# crates/dapple/Cargo.toml

[package]
name = "dapple"
version = "0.1.0"
edition = "2024"
description = "Rust SDK for the dapple-prompt-api"

[dependencies]
# HTTP client
reqwest = { version = "0.12", features = ["json", "multipart", "native-tls"] }
tokio = { version = "1", features = ["rt", "macros"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Type support
uuid = { version = "1", features = ["v7", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

# Tarball construction (for ingest builders)
tar = "0.4"
flate2 = "1.0"

# Error handling
thiserror = "2"

# Build-time only (for the generate binary)
[dev-dependencies]
# None needed for the library itself

[[bin]]
name = "generate"
path = "bin/generate.rs"
required-features = ["codegen"]

[features]
default = []
codegen = ["dep:typify", "dep:prettyplease", "dep:syn", "dep:schemars"]

[dependencies.typify]
version = "0.6"
default-features = false
optional = true

[dependencies.prettyplease]
version = "0.2"
optional = true

[dependencies.syn]
version = "2"
optional = true

[dependencies.schemars]
version = "0.8"
optional = true
```

The `codegen` feature gate keeps `typify`, `prettyplease`, `syn`, and `schemars` out of the default dependency tree. Applications that depend on `dapple` as a library never pull in code generation dependencies. The `generate` binary requires `--features codegen` to build.

### 14.7 Migration Path from dapple-suite

The transition from dapple-suite's `packages/api/` to the `dapple` crate follows a phased approach to avoid breakage:

**Phase 1: Establish the SDK (this plan)**

- Transform `crates/dapple/` from placeholder to SDK with models, client, endpoints, and builders.
- The `generate` binary produces `src/models/generated.rs` from the same OpenAPI spec that dapple-suite uses.
- `src/models/complement.rs` absorbs the hand-written types from `dapple-suite/packages/api/src/models.rs`.
- `src/client/` absorbs the `DappleClient` from `dapple-suite/packages/api/src/client.rs`.
- `src/endpoints/` absorbs endpoint methods from `dapple-suite/packages/api/src/endpoints/`.
- The builders module is new -- no equivalent exists in dapple-suite.

**Phase 2: Dual-source period**

- dapple-suite adds `dapple` as a path or git dependency.
- `packages/api/` re-exports from `dapple` where types overlap, maintaining backward compatibility.
- New endpoint methods and builders are written only in the `dapple` crate.
- dapple-suite applications gradually migrate imports from `packages/api` to `dapple`.

**Phase 3: Deprecate dapple-suite API package**

- All dapple-suite applications import directly from the `dapple` crate.
- `packages/api/` is reduced to thin re-exports or removed.
- `tools/generate-models/` is deprecated in favor of `cargo run -p dapple --bin generate --features codegen`.
- The `auth` and `storage` modules remain in dapple-suite if they contain UI-specific concerns (keychain access, session management) that do not belong in a server-side SDK.

**What stays in dapple-suite:**

- `auth.rs` -- Token signing, keychain integration, and platform-specific credential storage. These are client-application concerns, not SDK concerns.
- `storage.rs` -- `TokenStore` trait and platform implementations (macOS Keychain, DPAPI, Secret Service). SDK consumers may use their own storage.
- Server functions (`server_fns`) -- Dioxus-specific, tied to the fullstack framework.

**What moves to the dapple crate:**

| dapple-suite File | Destination in dapple crate |
|---|---|
| `packages/api/src/models_generated.rs` | `src/models/generated.rs` |
| `packages/api/src/models.rs` | `src/models/complement.rs` |
| `packages/api/src/client.rs` | `src/client/mod.rs` + `src/client/batch.rs` + `src/client/error.rs` |
| `packages/api/src/endpoints/*.rs` | `src/endpoints/*.rs` |
| `tools/generate-models/src/main.rs` | `bin/generate.rs` |

### 14.8 Relationship to Guide Evolution Workstreams

The SDK directly supports the complex operations introduced in Sections 2, 3, 12, and 13:

| Workstream | SDK Component | What It Provides |
|---|---|---|
| Tarball Ingest (Section 3) | `GuideIngestBuilder` | Tarball construction from directory, multipart upload, dry-run support |
| Fluent-Native Content (Section 12) | `FluentMessageBuilder` | Locale body composition, engine body attachment, message creation in one call |
| Shared Content Library (Section 13) | `SharedContentIngestBuilder` | Scoped tarball upload, scope validation, partner ID support |
| Shared Content Library (Section 13) | `PlaybookBuilder` | Playbook composition with both inline stages and `shared_stage()` references |
| PDF Rendering (Section 6) | `endpoints::render` | Typed render request with profile and locale parameters |
| Guide Assets (Section 4) | `endpoints::assets` | Draft/chunk/finalize upload pipeline |

Without the SDK, every consumer of these operations must manually construct tarballs, compose multipart requests, manage the Fluent message creation sequence, and understand scope visibility rules. The builders encapsulate these conventions so consumers interact at the semantic level ("ingest this guide directory") rather than the protocol level ("create a gzip archive, POST as multipart/form-data to this endpoint with these query parameters").

### 14.9 Testing Strategy

#### 14.9.1 Unit Tests (in the dapple crate)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_request_single() {
        let req = BatchRequest::single("hello".to_string());
        assert_eq!(req.objs.len(), 1);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["objs"][0], "hello");
    }

    #[test]
    fn test_task_status_roundtrip() {
        for status in [TaskStatus::Queued, TaskStatus::Running, TaskStatus::Ok, TaskStatus::Failed, TaskStatus::Canceled] {
            let json = serde_json::to_string(&status).unwrap();
            let back: TaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }

    #[test]
    fn test_client_url_construction() {
        let client = DappleClient::new("https://api.example.com");
        assert_eq!(client.url("guides/"), "https://api.example.com/api/guides/");
    }

    #[test]
    fn test_client_url_trims_trailing_slash() {
        let client = DappleClient::new("https://api.example.com/");
        assert_eq!(client.url("guides/"), "https://api.example.com/api/guides/");
    }
}
```

#### 14.9.2 Builder Tests

```rust
#[test]
fn test_guide_ingest_builder_creates_tarball() {
    // Create a temp directory with guide.toml and SUMMARY.md
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("guide.toml"), r#"title = "Test""#).unwrap();
    std::fs::write(dir.path().join("SUMMARY.md"), "# Summary\n- [Intro](intro.md)").unwrap();
    std::fs::write(dir.path().join("intro.md"), "# Intro\n\nContent.").unwrap();

    let builder = GuideIngestBuilder::new(dir.path())
        .source_path("public/test")
        .dry_run(true);

    // Verify tarball can be created (send() requires a live server)
    let tarball = builder.create_tarball().unwrap();
    assert!(!tarball.is_empty());

    // Decompress and verify contents
    let decoder = flate2::read::GzDecoder::new(&tarball[..]);
    let mut archive = tar::Archive::new(decoder);
    let entries: Vec<String> = archive.entries().unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(entries.iter().any(|e| e.contains("guide.toml")));
    assert!(entries.iter().any(|e| e.contains("SUMMARY.md")));
}
```

#### 14.9.3 Integration Tests

Integration tests for the SDK run against a live dapple-prompt-api instance. They are gated behind a feature flag or environment variable:

```rust
// crates/dapple/tests/integration.rs

#[cfg(feature = "integration")]
#[tokio::test]
async fn test_sdk_guide_ingest_dry_run() {
    let client = DappleClient::new(
        std::env::var("DAPPLE_API_URL").unwrap_or("http://localhost:8080".into())
    );

    let result = GuideIngestBuilder::new("tests/fixtures/test-guide/")
        .source_path("public/test")
        .dry_run(true)
        .send(&client)
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.valid);
}
```

These tests reuse the same fixture directories defined in Section 9.4 (test guide) and Section 13.20.3 (shared content).

### 14.10 Files to Create/Modify

| Action | File | Notes |
|--------|------|-------|
| Rewrite | `crates/dapple/Cargo.toml` | Replace placeholder with SDK dependencies and feature flags |
| Rewrite | `crates/dapple/src/lib.rs` | Module declarations and re-exports |
| Create | `crates/dapple/src/models/mod.rs` | Re-export generated + complement models |
| Create | `crates/dapple/src/models/generated.rs` | Auto-generated from OpenAPI spec |
| Create | `crates/dapple/src/models/complement.rs` | Migrated from dapple-suite `models.rs` |
| Create | `crates/dapple/src/client/mod.rs` | DappleClient struct, auth, config |
| Create | `crates/dapple/src/client/batch.rs` | BatchRequest/BatchResponse types |
| Create | `crates/dapple/src/client/error.rs` | ApiError, ApiResult |
| Create | `crates/dapple/src/endpoints/mod.rs` | Re-export all endpoint modules |
| Create | `crates/dapple/src/endpoints/guides.rs` | Guide CRUD + ingest |
| Create | `crates/dapple/src/endpoints/shared.rs` | Shared content ingest |
| Create | `crates/dapple/src/endpoints/journeys.rs` | Journey operations |
| Create | `crates/dapple/src/endpoints/playbooks.rs` | Playbook operations |
| Create | `crates/dapple/src/endpoints/fluent.rs` | Fluent message operations |
| Create | `crates/dapple/src/endpoints/assets.rs` | Asset upload/download |
| Create | `crates/dapple/src/endpoints/briefs.rs` | Brief, BriefGroup, BriefPack operations |
| Create | `crates/dapple/src/endpoints/inference.rs` | Inference profile operations |
| Create | `crates/dapple/src/endpoints/render.rs` | PDF render operations |
| Create | `crates/dapple/src/endpoints/dispatches.rs` | Dispatch operations |
| Create | `crates/dapple/src/endpoints/bindings.rs` | Intent binding operations |
| Create | `crates/dapple/src/endpoints/tokens.rs` | API token management |
| Create | `crates/dapple/src/builders/mod.rs` | Re-export all builders |
| Create | `crates/dapple/src/builders/fluent_message.rs` | FluentMessageBuilder |
| Create | `crates/dapple/src/builders/guide_ingest.rs` | GuideIngestBuilder |
| Create | `crates/dapple/src/builders/shared_ingest.rs` | SharedContentIngestBuilder |
| Create | `crates/dapple/src/builders/playbook.rs` | PlaybookBuilder |
| Create | `crates/dapple/bin/generate.rs` | Code generation binary |
| Modify | `Cargo.toml` (workspace root) | Ensure `dapple` member is listed (already present) |

### 14.11 Open Questions and Risks

#### Open Questions

1. **Auth module scope** -- The dapple-suite `auth.rs` contains `SignedAuthToken` (HMAC-based token signing), `DevClaims` (development mode claims), and `AuthState` (token lifecycle management). `SignedAuthToken` involves `ed25519-dalek` for cryptographic signing, which is a significant dependency. Should the SDK include the signing logic, or should it only accept pre-signed tokens and JWTs? Recommendation: the SDK accepts pre-obtained tokens via `with_token()` and `with_jwt()`. The signing logic stays in dapple-suite or is offered behind an optional feature flag (`features = ["signing"]`).

2. **Async runtime requirement** -- The SDK uses `reqwest` which requires `tokio`. Should a synchronous (blocking) client be offered for CLI tools and scripts that do not need async? Recommendation: offer `reqwest`'s blocking feature behind a `blocking` feature flag. The primary API is async.

3. **Generated code drift** -- If the generated file is committed to version control, it can drift from the live API. How should this be detected? Recommendation: CI runs the generate binary against the latest API spec and fails if the output differs from the committed file. This is the same approach used by `cargo sqlx prepare` for offline mode.

4. **Version coupling** -- Should the SDK crate version track the API version? The API uses semver (target 0.3.0 per this plan). Recommendation: the SDK tracks its own semver, with a `COMPATIBLE_API_VERSION` constant that documents which API version it was generated against. The client should send an `X-Dapple-SDK-Version` header for server-side compatibility checking.

5. **Feature flag granularity** -- Should endpoint modules be individually feature-gated (e.g., `features = ["guides", "playbooks"]`) to reduce compile times for consumers that only use a subset? Recommendation: no, at least initially. The overhead of generated types is minimal, and feature-flag complexity is not justified until the crate grows significantly.

#### Risks

1. **Dependency weight** -- Adding `reqwest`, `tokio`, `tar`, and `flate2` to the `dapple` crate increases compile time and binary size. Mitigation: `tar` and `flate2` are only needed by the builders. Consider gating them behind a `builders` feature flag if compile time becomes a concern.

2. **Breaking changes during migration** -- dapple-suite applications depend on the existing `packages/api/` types. Moving to the `dapple` crate changes import paths and may alter type definitions. Mitigation: Phase 2 (dual-source period) maintains backward compatibility via re-exports. Type aliases bridge any naming differences.

3. **OpenAPI spec completeness** -- Some types in `dapple-suite/packages/api/src/models.rs` exist because the OpenAPI spec does not fully express them (internally-tagged unions, batch wrappers). If the spec evolves to cover these, the complement types become redundant. Mitigation: the complement module is explicit about why each type exists. As the spec improves, types move from complement to generated.

4. **Tarball construction correctness** -- The `GuideIngestBuilder` constructs tarballs client-side. If the tarball format does not match what the API expects (path prefix conventions, symlink handling, file ordering), ingest will fail silently or with confusing errors. Mitigation: thorough builder tests that round-trip create/extract and integration tests against the real ingest endpoint.

5. **Multi-repo coordination** -- During Phase 2, both dapple-prompt and dapple-suite must be coordinated. A schema change requires: (a) update dapple-prompt-api schema, (b) regenerate models in the `dapple` crate, (c) update dapple-suite's dependency. Mitigation: this is strictly better than the current situation (where dapple-suite must independently regenerate). The `dapple` crate being in the same repo as the API eliminates step (a)-(b) coordination.
