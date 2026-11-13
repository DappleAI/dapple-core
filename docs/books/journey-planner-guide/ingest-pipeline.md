# Guide Ingest Pipeline

> **Audience:** :wrench: Prompt Engineers

This chapter explains what happens when you ingest a guide tarball into the Dapple Prompt API. The ingest pipeline is the bridge between authored guide content (directories of markdown, TOML, Fluent files, and assets) and the database entities that power the platform.

> For SDK and HTTP ingest methods, see the [API Consumer Guide - Guide Ingest API](../api-consumer-guide/ingest-api.md).

---

## Overview

The ingest pipeline converts a `.tar.gz` archive of guide source files into a full graph of database entities within a single atomic transaction. The pipeline has four phases:

1. **Extract** -- Decompress the tarball with security validation
2. **Parse** -- Read `guide.toml`, `SUMMARY.md`, chapter files, `.ftl` locales, and assets into a `ParsedGuide` AST
3. **Plan** -- Transform the AST into an `IngestPlan` containing all derived object definitions
4. **Materialize** -- Write everything to the database atomically (or return a dry-run summary)

```
.tar.gz upload
    |
    v
extract_tarball_field()        [50MB max, multipart/form-data]
    |
    v
decompress_and_plan()
    |-- tar extraction          [security: no symlinks, no path traversal]
    |-- find_guide_root()       [locate guide.toml in tarball]
    |-- parse_guide()           [dapple-parser -> ParsedGuide]
    |-- plan_guide()            [dapple-parser::transform -> IngestPlan]
    |
    v
dry_run?  ----yes----> validation summary (HTTP 200)
    |
    no
    |
    v
materialize_guide()            [POST: new guide path + v1]
materialize_guide_version()    [PATCH: new version under existing path]
    |-- upload_assets()
    |-- materialize_fluent_messages()
    |-- insert_guides() / patch_guides()
    |-- create_guide_asset_associations()
    |-- create_guide_card()
    |-- materialize_derived_objects()
         |-- materialize_tasks()
         |-- materialize_inference_profiles()
         |-- materialize_prompts()
         |-- materialize_task_prompts()
         |-- materialize_inference_profile_configs()
         |-- materialize_playbook()
    |
    v
IngestGuideResponse (HTTP 201)
```

---

## Phase 1: Extract

The ingest endpoint accepts a `multipart/form-data` request with a single `file` field containing a `.tar.gz` archive.

**Endpoint:**
- `POST /api/guides/ingest` -- Create a new guide (new path + first version)
- `PATCH /api/guides/{path_object_id}/ingest` -- Create a new version of an existing guide

**Constraints:**
- Maximum tarball size: **50 MB** (52,428,800 bytes). Enforced before decompression.
- The `file` field name must be exactly `file`. If missing, the endpoint returns HTTP 400.

**Query parameters:**
- `dry_run=true` -- Parse and validate without materializing. Returns a validation summary.
- `source_path=<path>` -- Override the guide's `source_path` metadata field.

---

## Phase 2: Parse

After extraction, the tarball is decompressed into a temporary directory and processed with security checks.

### Security Validation

The pipeline rejects tarballs that contain:

- **Symlinks** -- Both symbolic links and hard links are rejected. If your source directory contains symlinks, create the tarball with `tar czh` (the `--dereference` flag) to follow them at archive time.
- **Path traversal** -- Any entry containing `..` path components is rejected. All paths must resolve within the extraction directory.

### Guide Root Discovery

The pipeline searches for `guide.toml` (or `book.toml` for mdBook compatibility) using this strategy:

1. Check the extraction root directory
2. Check immediate subdirectories for `guide.toml`
3. If there is exactly one subdirectory, check its children
4. If the single subdirectory contains `SUMMARY.md`, use it as the root

This handles common tarball layouts where the archive contains either flat files or a single top-level directory wrapper.

### Parsing

The `dapple_parser::parse_guide()` function reads the guide root and produces a `ParsedGuide` containing:

- **Metadata** from `guide.toml`: title, description, author, classification, version, card config, deprecation
- **Chapters** from `SUMMARY.md`: ordered list with nesting depth and parent relationships
- **Blocks** from chapter `.md` files: fenced directive blocks (`:::stage`, `:::step`, `:::nushell`, `:::brief_pack`, `:::inference_profile`, `:::ref`, `:::block`) and prose blocks
- **Locales** from `locales/*.ftl` files: Fluent message definitions per locale tag
- **Asset paths**: non-markdown files discovered in the guide directory

### Transform

The `dapple_parser::transform::plan_guide()` function walks the parsed AST and produces an `IngestPlan`. This is a pure function with no database access. It extracts:

- **ChapterDefs** with BlockDefs (preserving hierarchy)
- **PlaybookDef** from `:::stage`/`:::step` block hierarchy
- **TaskDefs** from `:::nushell` blocks
- **BriefPackDefs** from `:::brief_pack` blocks
- **InferenceProfileDefs** from `:::inference_profile` blocks
- **RefDefs** from `:::ref` blocks
- **SharedRefDefs** from blocks with `ref` attributes (shared content references)
- **FluentMessageSets** grouped by message key across all locales
- **CardDef** from the `[card]` section of `guide.toml`
- **AssetFileEntries** with file bytes and inferred MIME types
- **Warnings** for non-fatal issues (missing attributes, unnamed blocks)

---

## IngestPlan Structure

The `IngestPlan` is the complete blueprint for materialization:

```
IngestPlan
 |-- title: String
 |-- description: Option<String>
 |-- author: Option<String>
 |-- classification: Option<String>
 |-- version: Option<String>               # semver from guide.toml
 |-- source_path: Option<String>           # set by query param override
 |
 |-- chapters: Vec<ChapterDef>
 |    |-- title, source_file, parent_index
 |    |-- blocks: Vec<BlockDef>
 |         |-- directive_type              # "prose", "stage", "step", "nushell", etc.
 |         |-- name: Option<String>        # matches Fluent message key
 |         |-- content: Option<String>     # None for Fluent-backed, Some for inline
 |         |-- attributes: serde_json::Value
 |         |-- tags: Vec<String>           # audience tags
 |
 |-- card: Option<CardDef>
 |    |-- image: Option<String>            # relative path to card image
 |    |-- color: Option<String>
 |    |-- accent: Option<String>
 |
 |-- asset_files: Vec<AssetFileEntry>
 |    |-- relative_path, mime_type, data (bytes)
 |
 |-- derived_playbook: Option<PlaybookDef>
 |    |-- name, version
 |    |-- stages: Vec<StageDef>
 |         |-- name, version, flow_control
 |         |-- steps: Vec<StepDef>
 |              |-- name, version, flow_control, task_refs: Vec<String>
 |
 |-- derived_tasks: Vec<TaskDef>
 |    |-- name, version, script, role, target
 |
 |-- derived_brief_packs: Vec<BriefPackDef>
 |    |-- name
 |
 |-- derived_inference_profiles: Vec<InferenceProfileDef>
 |    |-- name
 |
 |-- derived_refs: Vec<RefDef>
 |    |-- slug, ref_type
 |
 |-- shared_refs: Vec<SharedRefDef>
 |    |-- ref_key, directive_type
 |
 |-- fluent_messages: HashMap<String, FluentMessageSet>
 |    |-- key, description
 |    |-- bodies: HashMap<locale_tag, body_text>
 |    |-- attributes: HashMap<locale_tag, HashMap<attr_key, attr_value>>
 |
 |-- deprecation_level: Option<String>     # "deprecated", "retired", "quarantined"
 |-- deprecation_reason: Option<String>
 |-- warnings: Vec<String>
```

### Inline vs Fluent-Backed Blocks

The transform layer determines whether a block retains inline content or defers to Fluent messages:

| Directive Type | Content Model | Rationale |
|----------------|--------------|-----------|
| `nushell` | Inline (`content = Some(...)`) | Code is not localized |
| `ref` | Inline (`content = Some(...)`) | Structural cross-reference, not localized |
| `prose` | Fluent-backed (`content = None`) | Narrative text varies by locale |
| `stage` | Fluent-backed (`content = None`) | Stage descriptions vary by locale |
| `step` | Fluent-backed (`content = None`) | Step descriptions vary by locale |
| `brief_pack` | Fluent-backed (`content = None`) | Pack content and card metadata vary by locale |
| `inference_profile` | Fluent-backed (`content = None`) | System prompts and descriptions vary by locale |
| `block` | Fluent-backed (`content = None`) | Generic localizable content |

If a Fluent-backed block has non-empty markdown content in the chapter file, that content is retained as fallback text in the `content` field.

---

## Phase 3: Dry Run

When `dry_run=true`, the pipeline returns a validation summary without writing to the database. This is useful for CI checks and pre-flight validation.

The response (HTTP 200) includes:

```json
{
  "valid": true,
  "errors": [],
  "warnings": [":::nushell block at chapter 2, block 3 has no name attribute"],
  "summary": {
    "title": "Build a Resume",
    "chapters": 4,
    "blocks": 12,
    "assets": 2,
    "derived_playbook_stages": 2,
    "derived_brief_packs": 1,
    "derived_tasks": 3,
    "derived_inference_profiles": 1,
    "deprecation_level": null,
    "deprecation_reason": null
  }
}
```

The `warnings` array contains non-fatal issues discovered during parsing and planning: unnamed blocks, missing attributes, Fluent key mismatches, and similar problems that do not prevent ingest but may indicate authoring errors.

---

## Phase 4: Materialize

Materialization writes all entities to the database. The operations differ between POST (new guide) and PATCH (new version), but both follow the same sequence.

### Step 1: Upload Assets

Asset files discovered in the guide directory are uploaded through the draft/chunk/finalize pipeline:

1. **Create drafts** -- One draft per asset file, classified by MIME type (`image`, `audio`, `video`, `document`, `binary`)
2. **Insert chunks** -- Each file is split into 256 KB chunks and inserted into `rev.draft_chunks`
3. **Finalize** -- Each draft is finalized, creating an immutable `rev.assets` entry with `rev.asset_chunks`

The result is a map from relative path to asset object ID, used for downstream association.

### Step 2: Materialize Fluent Messages

For each `FluentMessageSet` in the plan:

1. **Dedup check** -- Query existing Fluent messages by name. If a message with the same name already exists, reuse its internal ID.
2. **Insert new** -- Create new `FluentMessage` paths and versions with localized bodies for messages that do not yet exist.

The result is a map from message key to internal Fluent message ID, used to link blocks to their localized content.

### Step 3: Insert or Patch Guide

**POST (new guide):** `insert_guides()` creates:
- A guide path (lineage)
- A versioned guide object (v1, or the version from `guide.toml`)
- All chapters with parent relationships
- All blocks within each chapter
- Block tag associations

**PATCH (new version):** `patch_guides()` creates a new versioned guide object under the existing path, incrementing the version. The path (and its slug) remains stable across versions.

Both operations are atomic -- a single CTE-based SQL query creates the entire guide hierarchy.

### Step 4: Associate Assets

Guide-to-asset associations are created in `rev.guide_assets` with role classification:
- `card_image` -- If the asset's relative path matches the card image path from `guide.toml`
- `attachment` -- All other assets

### Step 5: Create Guide Card

If the `[card]` section exists in `guide.toml`, a card record is created with:
- Card image asset linkage (if specified)
- Color and accent values (locale-independent visual properties)
- Subtitle and category are Fluent attributes on the brief-pack message, not card fields

### Step 6: Materialize Derived Objects

Derived objects are independent entities extracted from the guide's directive blocks. Each type uses slug-plus-version deduplication to avoid creating duplicates across multiple ingests.

**Tasks** (from `:::nushell` blocks):
- Slug is computed from the task name
- If a task at the same slug and version already exists, its object ID is reused
- New tasks are created with `function = "nushell"` and the script stored in `function_inputs`

**Inference Profiles** (from `:::inference_profile` blocks):
- Name-based deduplication (matching the tasks pattern)
- If a profile with the same name already exists, its object ID is reused
- Returns both object IDs and name-to-internal-ID pairs for downstream config wiring

**Prompts** (one per `:::inference_profile` block):
- Each inference profile block produces a corresponding Prompt entity
- The prompt is linked to its Fluent message via `rev.prompt_fluent_messages`
- Name-based deduplication: if a prompt with the same name exists, it is reused

**Task-Prompt Links** (`rev.task_prompts`):
- A cross-product of all materialized tasks and all materialized prompts
- Every task is linked to every prompt, enabling any task to use any profile's prompt

**Inference Profile Configs** (`rev.inference_profile_configs`):
- Extracts `.lineage`, `.tier`, and `.release` attributes from the FluentMessageSet for each profile
- Resolves the `model_release_id` by querying `rev.model_releases` with the extracted lineage/tier/release tuple
- Creates config entries linking each profile to its prompt and the resolved model release

**Playbook** (from `:::stage`/`:::step` hierarchy):
- Deduplication at every level: playbook, stage, step
- Steps are linked to their tasks by name — from nested `:::nushell` children (auto-scoped to the step name) and/or the step's `tasks: [...]` / `task:` attribute. A step's `task_refs` is the resulting ordered list of task names. Multiple tasks per step is normal.
- Stages are linked to steps via the hierarchy extracted during parsing
- If a playbook at the same slug already exists (any version), it is reused

**Brief Packs** (from `:::brief_pack` blocks):
- Brief pack materialization is currently deferred. The `BriefPackDef` is extracted during planning, but the full Brief/BriefGroup/BriefPack hierarchy requires Fluent attribute mapping that is not yet fully wired. A warning is emitted during ingest.

### Step 7: Embedding Generation (asynchronous)

After the materialization transaction commits, the API spawns a background task that asks the conduit (`POST /embed` on the `dapple-prompt-conduit` service) to generate per-block embeddings for embeddable directive types (`prose`, `stage`, `step`, `block`, `ref`). The result is written to `rev.guide_content_embeddings` along with tag and locale junctions, replacing any prior embeddings for the guide atomically.

This phase is **fire-and-forget**: the HTTP response to your ingest call returns before embedding writes complete. Failures are logged as warnings and never fail the ingest. Consequences for callers:

- A brand-new guide is briefly invisible to `GET /api/guides/search` until embeddings finish.
- If `DAPPLE_PROMPT_CONDUIT_URL` is unset on the API, or the conduit's embedding service is not configured, embeddings are silently skipped.
- The 768-dimension vectors come from ModernBERT (`lightonai/modernbert-embed-large`) running via ONNX inside the conduit.

### Content-addressable deduplication

Across all phases, materialization is **content-addressable**: a SHA-1 hash is computed over each entity's canonical content. On PATCH, if the new hash matches the latest version's hash, the operation short-circuits and returns the existing version — no duplicate is written. The hash propagates Merkle-style: a Step's hash folds its ordered Task hashes (every task in the step); a Stage's hash includes its sorted Step hashes; a Playbook's hash includes its sorted Stage hashes. Change one task and every ancestor hash updates.

---

## POST vs PATCH

| Aspect | POST `/api/guides/ingest` | PATCH `/api/guides/{path_id}/ingest` |
|--------|--------------------------|--------------------------------------|
| Creates | New guide path + first version | New version under existing path |
| Path | New path with new slug | Existing path, slug unchanged |
| Version | From `guide.toml` or `0.0.1` | From `guide.toml` or auto-incremented patch |
| Derived objects | Created or reused (slug dedup) | Created or reused (slug dedup) |
| Response | HTTP 201 with guide + derived refs | HTTP 201 with guide + derived refs |

---

## Response Format

A successful ingest returns an `IngestGuideResponse`:

```json
{
  "status": "success",
  "guide": {
    "object_id": "01234567-...",
    "path_object_id": "89abcdef-...",
    "title": "Build a Resume",
    "slug": "build-resume",
    "version_major": 1,
    "version_minor": 0,
    "version_patch": 0,
    "chapters": [ ... ],
    "created_at": "2026-02-18T..."
  },
  "derived_objects": {
    "playbook_object_id": "fedcba98-...",
    "task_object_ids": ["11111111-...", "22222222-..."],
    "inference_profile_object_ids": ["33333333-..."],
    "brief_pack_object_ids": [],
    "prompt_object_ids": ["44444444-..."]
  }
}
```

---

## Constraints Summary

| Constraint | Value | Enforcement Point |
|------------|-------|-------------------|
| Max tarball size | 50 MB | `extract_tarball_field()` |
| Symlinks | Rejected | Tar entry iteration |
| Path traversal (`..`) | Rejected | Tar entry iteration |
| Transaction scope | Atomic | Single database transaction per ingest |
| Chunk size | 256 KB | Asset upload pipeline |
| Version format | Semver | `guide.toml` parsing (validated by `semver` crate) |
| Deprecation levels | `deprecated`, `retired`, `quarantined` | `guide.toml` parsing |

---

## Creating a Tarball

Package a guide directory into a gzipped tarball for ingest:

```bash
tar czh -f my-guide.tar.gz my-guide/
```

The `h` flag follows symlinks, which is important if your guide references shared assets via symlinks.

### What to Include

Your tarball should contain the complete guide directory:

```
my-guide/
├── guide.toml          # Required
├── SUMMARY.md          # Required
├── locales/            # Fluent .ftl files
│   ├── en.ftl
│   └── pt-BR.ftl
├── assets/             # Images, documents, binary files
│   └── images/
│       └── card.jpg
├── intro.md            # Chapter files
└── advanced/
    └── configuration.md
```

### Constraints

| Constraint | Value |
|------------|-------|
| Maximum tarball size | **50 MB** |
| Symlinks inside tarball | Rejected (use `h` flag to follow them at tar time) |
| Path traversal (`..`) | Rejected |
| Required file | `guide.toml` must exist in the tarball root or a single nested directory |

> For SDK and HTTP ingest methods, see the [API Consumer Guide - Guide Ingest API](../api-consumer-guide/ingest-api.md).

---

## Troubleshooting

### Common Structural Errors

**"No guide.toml found in the tarball"** -- The pipeline could not locate `guide.toml` in the archive. Ensure the tarball contains either `guide.toml` at the root or inside a single top-level directory.

**"Tarball contains symlinks"** -- Use `tar czh` (with `--dereference`) when creating the archive to follow symlinks instead of storing them.

**"Tarball exceeds maximum size"** -- The archive is over 50 MB. Remove unnecessary assets or split into multiple guides.

**"Failed to parse guide"** -- A parsing error in `guide.toml`, `SUMMARY.md`, or a chapter markdown file. Check the error message for the specific file and line.

**"Guide patch failed"** -- The `path_object_id` in the PATCH URL does not match an existing guide path. Verify the UUID.

**Missing Fluent keys** -- If a Fluent-backed directive block has a `name` attribute but no corresponding message key in any `.ftl` file, the pipeline emits a warning. The block is still created, but its localized content will be empty.

**Unnamed directive blocks** -- Blocks without a `name` attribute cannot be linked to Fluent messages. The pipeline emits a warning. Consider adding a `name` attribute to all Fluent-backed blocks.

### Validation Strategy

Use dry-run ingests in your CI pipeline to catch errors before deployment:

```bash
curl -X POST https://api.example.com/api/guides/ingest?dry_run=true \
  -H "Authorization: Bearer dpl_..." \
  -F "file=@my-guide.tar.gz"
```

Check the `warnings` array for non-fatal issues and the `summary` object for expected entity counts.
