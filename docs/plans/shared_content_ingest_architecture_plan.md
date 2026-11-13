# Shared Content Ingest Architecture Plan

**Status:** Draft
**Created:** 2026-02-15
**Last Updated:** 2026-02-15
**Target Version:** 0.1.0
**Dependencies:** Guide ingest pipeline (implemented), shared content migrations (20260217000001-4, applied)

---

## 1. Executive Summary

The shared content ingest endpoint (`POST /api/shared-content/ingest?scope=<common|internal>`) enables authors to upload reusable content libraries that multiple guides can reference. Shared content lives in `_shared/common/` and `_shared/internal/` directories and is ingested separately from guides. Once ingested, guides reference shared content via `:::ref` blocks (e.g., `:::stage { ref: "common/validate-output" }`) and the platform resolves these at materialization time.

This endpoint is currently a 501 stub. The database schema for shared content is already in place: `rev.object_paths` has `scope` and `ref_key` columns, `rev.shared_scopes` is a lookup table, and `rev.guide_shared_refs` links guide versions to shared paths. The parser already supports `shared.toml` parsing and the `ref_path` attribute on blocks. What remains is:

1. A parser-level `parse_shared_content()` function that handles the shared content directory structure (using `shared.toml` instead of `guide.toml`)
2. A transform-level `plan_shared_content()` function producing a `SharedContentPlan`
3. The handler implementation that extracts, parses, validates, and materializes shared content into `rev.object_paths` with the appropriate `scope` and `ref_key`
4. Materialization of the derived objects (tasks, inference profiles, Fluent messages) that the shared content defines

The design closely follows the existing guide ingest pipeline (`extract_tarball_field()` -> `decompress_and_plan()` -> `materialize_guide()`) but is simpler because shared content does not produce a full guide entity with chapters. Instead, it creates an `object_path` with `scope` and `ref_key`, then materializes the individual derived objects that the shared content defines.

## 2. Context and Background

### Current State

**Database infrastructure (complete):**
- `rev.object_paths` has `scope TEXT` and `ref_key TEXT` columns (migrations 20260217000001-2)
- `rev.shared_scopes` lookup table seeded with `common` and `internal` (migration 20260217000003)
- `rev.guide_shared_refs` associative table linking guides to shared paths (migration 20260217000004)
- `select_shared_paths_by_ref_keys.sql` query and `query::shared::select_shared_paths_by_ref_keys()` function exist

**Parser infrastructure (partial):**
- `dapple_parser::shared::parse_shared_toml()` parses `shared.toml` metadata
- `dapple_parser::shared::SharedToml` struct with `name`, `description`, `scope`
- `ParsedBlock.ref_path` field is set when a block uses `ref` instead of `name`
- `SharedRefDef` type exists in the transform layer
- `ParseError::MissingSharedToml` and `ParseError::InvalidSharedToml` error variants exist
- No `parse_shared_content()` or `plan_shared_content()` functions exist yet

**API infrastructure (stub):**
- Route registered at `/api/shared-content/ingest`
- `shared_content_ingest_handler()` returns 501
- `SharedIngestQueryParams`, `SharedIngestResponse`, `SharedIngestValidationResult`, `SharedIngestSummary`, `SharedIngestErrorResponse` schemas defined

### Problems Being Addressed

1. Guides that reference shared content via `:::stage { ref: "common/validate-output" }` cannot resolve the reference because the shared object paths do not exist in the database yet.
2. Common reusable objects (validation stages, standard prompts, shared glossaries) must be duplicated across every guide rather than being defined once and referenced.
3. Scope visibility enforcement (internal guides see both scopes, public guides see only `common`) cannot be implemented without the ingested shared paths.

### Relevant Patterns

The guide ingest handler at `crates/dapple-prompt-api/src/handler/guide_ingest.rs` is the reference implementation. The shared content ingest handler follows the same pipeline structure but with key differences:

| Aspect | Guide Ingest | Shared Content Ingest |
|--------|-------------|----------------------|
| Config file | `guide.toml` | `shared.toml` |
| Root entity | Creates a guide (versioned) with chapters and blocks | Creates an `object_path` with `scope` and `ref_key` |
| Chapters/blocks | Full chapter hierarchy stored in `guide_chapters`/`guide_blocks` | Chapters parsed for derived object extraction only; not stored as guide entities |
| Derived objects | Tasks, inference profiles, playbook, brief packs | Tasks, inference profiles, Fluent messages (no playbook, no brief packs) |
| Versioning | Guide versioned via `versioned_objects` | Object path is the anchor; derived objects are individually versioned |
| Assets | Uploaded via draft/chunk/finalize pipeline | Same pipeline |
| Scope | Not applicable | `common` or `internal` |

## 3. Shared Content Directory Structure

A shared content tarball contains a directory with this structure:

```
_shared/common/validate-output/
  shared.toml            # Required: name, description, scope
  SUMMARY.md             # Required: references chapter .md files
  validation-stage.md    # Chapter files with directive blocks
  locales/
    en.ftl               # Fluent messages
    pt-BR.ftl            # Additional locales
  assets/
    diagram.png          # Optional asset files
```

### shared.toml

```toml
name = "validate-output"
description = "Standard output validation stage with retry logic"
scope = "common"
```

The `scope` field in `shared.toml` must match the `scope` query parameter. If they disagree, the handler returns a 422 error. The `name` is used to construct the `ref_key` as `{scope}/{name}` (e.g., `common/validate-output`).

### SUMMARY.md

Standard guide SUMMARY.md format. The shared content parser reuses `parse_guide()` but substitutes `shared.toml` for `guide.toml` metadata.

## 4. Database Schema Changes (DDL)

**No new migrations are needed.** All required database infrastructure is already in place:

- `rev.object_paths.scope` -- Set to `"common"` or `"internal"` for shared paths
- `rev.object_paths.ref_key` -- Set to `"{scope}/{name}"` for shared paths (unique index exists)
- `rev.shared_scopes` -- Lookup table with `common` and `internal` rows
- `rev.guide_shared_refs` -- Associative table for guide -> shared path references

The shared content ingest creates rows in existing tables:

1. **`rev.object_paths`** -- One row per shared content path, with `scope` and `ref_key` set
2. **`rev.versioned_objects`** -- One row per shared content version
3. **Derived object tables** -- Tasks in `rev.tasks`, Fluent messages in `rev.fluent_messages`/`rev.fluent_message_paths`, inference profiles in `rev.inference_profiles`, assets in `rev.assets`

### Scope Validation Query (new SQL file needed)

```sql
-- assets/sql/shared/validate_scope.sql
SELECT EXISTS (
    SELECT 1 FROM rev.shared_scopes WHERE key = $1
) AS "valid!";
```

### Insert Shared Object Path (new SQL file needed)

```sql
-- assets/sql/shared/insert_shared_object_path.sql
WITH entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = $1  -- entity_type_key
)
INSERT INTO rev.object_paths (entity_type_id, scope, ref_key)
SELECT
    (SELECT id FROM entity_type),
    $2,  -- scope
    $3   -- ref_key
RETURNING id, object_id, entity_type_id, scope, ref_key, created_at;
```

### Upsert Shared Object Path (for re-ingest)

When shared content is re-ingested, the `object_path` with the same `ref_key` already exists. The handler must detect this and create a new `versioned_object` under the existing path (PATCH semantics), or return the existing path for fresh materialization.

```sql
-- assets/sql/shared/select_or_insert_shared_object_path.sql
-- First try to find existing path by ref_key
WITH existing AS (
    SELECT id, object_id, entity_type_id, scope, ref_key, created_at
    FROM rev.object_paths
    WHERE ref_key = $3  -- ref_key
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = $1  -- entity_type_key
),
new_path AS (
    INSERT INTO rev.object_paths (entity_type_id, scope, ref_key)
    SELECT
        (SELECT id FROM entity_type),
        $2,  -- scope
        $3   -- ref_key
    WHERE NOT EXISTS (SELECT 1 FROM existing)
    RETURNING id, object_id, entity_type_id, scope, ref_key, created_at
)
SELECT id, object_id, entity_type_id, scope, ref_key AS "ref_key!", created_at
FROM existing
UNION ALL
SELECT id, object_id, entity_type_id, scope, ref_key AS "ref_key!", created_at
FROM new_path;
```

## 5. Object Model Design

### SharedContentPlan (new transform type)

This is the output of the transform layer for shared content. It is similar to `IngestPlan` but lacks guide-specific fields.

```rust
/// The ingest plan for shared content, produced by plan_shared_content().
pub struct SharedContentPlan {
    /// Name from shared.toml (e.g., "validate-output").
    pub name: String,
    /// Description from shared.toml.
    pub description: Option<String>,
    /// Scope from shared.toml ("common" or "internal").
    pub scope: String,
    /// The constructed ref_key: "{scope}/{name}".
    pub ref_key: String,

    /// Chapters parsed for block extraction (not stored as guide chapters).
    pub chapters: Vec<ChapterDef>,

    /// Asset files discovered in the shared content directory.
    pub asset_files: Vec<AssetFileEntry>,

    /// Task definitions extracted from :::nushell blocks.
    pub derived_tasks: Vec<TaskDef>,
    /// Inference profile definitions extracted from :::inference_profile blocks.
    pub derived_inference_profiles: Vec<InferenceProfileDef>,

    /// FluentMessage definitions per locale, keyed by message name.
    pub fluent_messages: HashMap<String, FluentMessageSet>,

    /// Validation warnings (non-fatal issues).
    pub warnings: Vec<String>,
}
```

Note: `SharedContentPlan` intentionally omits:
- `card` -- shared content has no promotional card
- `derived_playbook` -- shared content does not define standalone playbooks; individual stages/steps within shared content become derived objects that guides reference
- `derived_brief_packs` -- not applicable to shared content
- `derived_refs` -- shared content does not itself reference other shared content (no nesting)
- `shared_refs` -- same reason
- `source_path` -- not applicable (shared content uses `ref_key` for identification)

### Entity Type

Shared content paths use the existing entity type system. The appropriate entity type depends on what the shared content defines. However, since a shared content package can contain multiple directive types (stages, tasks, inference profiles), the `object_path` should use a dedicated entity type:

**New entity type needed:** `shared_content`

This requires a small seed migration (see Section 4 addendum below).

### Migration Addendum: Entity Type Seed

```sql
-- New migration: YYYYMMDD_shared_content_entity_type.sql
INSERT INTO rev.entity_types (key, description) VALUES
    ('shared_content', 'Shared content library path (common or internal scope)');
```

## 6. API Contract

### Endpoint

```
POST /api/shared-content/ingest?scope=<common|internal>&dry_run=<true|false>
Content-Type: multipart/form-data

field: file (application/gzip, .tar.gz tarball)
```

### Query Parameters (existing schema)

```rust
pub struct SharedIngestQueryParams {
    /// Required: "common" or "internal"
    pub scope: String,
    /// Optional: validate without persisting (default: false)
    #[serde(default)]
    pub dry_run: bool,
}
```

### Success Response (existing schema, 201 Created)

```json
{
    "status": "success",
    "ref_key": "common/validate-output",
    "path_object_id": "01234567-89ab-cdef-0123-456789abcdef",
    "scope": "common"
}
```

### Dry-Run Response (existing schema, 200 OK)

```json
{
    "valid": true,
    "errors": [],
    "warnings": [":::brief_pack blocks are ignored in shared content"],
    "summary": {
        "name": "validate-output",
        "scope": "common",
        "chapters": 1,
        "blocks": 3,
        "assets": 1,
        "fluent_messages": 2
    }
}
```

### Error Responses (existing schema)

**400 Bad Request** -- No file field, tarball too large, invalid multipart
```json
{ "status": "fail", "message": "No 'file' field found in multipart request" }
```

**422 Unprocessable Entity** -- Parse failure, scope mismatch, invalid shared.toml
```json
{ "status": "fail", "message": "Scope mismatch: shared.toml says 'internal' but query param says 'common'" }
```

**409 Conflict** -- Re-ingest of existing ref_key when versioning is not desired (future consideration)

**500 Internal Server Error** -- Database failure during materialization
```json
{ "status": "error", "message": "Materialization failed: ..." }
```

### Scope Validation Rules

1. Query parameter `scope` is required and must be `"common"` or `"internal"`
2. If `shared.toml` contains a `scope` field, it must match the query parameter
3. Invalid scope values return 422

## 7. SQL Query Strategy

### Shared Object Path Insert

The shared content ingest creates an `object_path` with `scope` and `ref_key`. This is a single row insert (not batch), since shared content is ingested one package at a time.

The `select_or_insert_shared_object_path.sql` query (Section 4) handles the idempotent upsert pattern. On re-ingest:
- If `ref_key` already exists: return the existing path (handler creates a new version of derived objects)
- If `ref_key` does not exist: create new path with `scope` and `ref_key`

### Derived Object Materialization

Derived objects (tasks, inference profiles, Fluent messages) use the same insert patterns as guide ingest. The key difference is that after creating these objects, the handler does not create guide chapters/blocks. Instead, the derived objects stand alone and are referenced by guides via `rev.guide_shared_refs`.

**Task insertion:** Reuse `insert_tasks()` from `dapple_data::query::tasks`
**Inference profile insertion:** Reuse `insert_inference_profile_models_in_tx()` from `dapple_data::query::inference_profiles`
**Fluent message insertion:** Reuse `insert_fluent_messages()` from `dapple_data::query::fluent_messages`
**Asset upload:** Reuse the `upload_assets()` helper from `guide_ingest.rs`

### Association Pattern

After materialization, the derived object `object_id`s should be associated with the shared content `object_path`. This can be accomplished via an associative table. However, since the existing schema does not have a `shared_content_objects` table, and the `ref_key` on `object_paths` already uniquely identifies the shared content, the association is implicit: derived objects are created under the shared content's namespace and can be looked up by re-parsing the shared content or by convention-based naming.

**Recommended approach:** Store the mapping of derived objects in a new `rev.shared_content_objects` table, or store the derived object references as JSONB metadata on the `object_path`. Given the append-only philosophy, a lightweight associative table is preferred:

```sql
-- New migration: YYYYMMDD_shared_content_objects.sql
CREATE TABLE rev.shared_content_objects (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    shared_path_id BIGINT NOT NULL REFERENCES rev.object_paths (id),
    target_path_id BIGINT NOT NULL REFERENCES rev.object_paths (id),
    object_kind TEXT NOT NULL,  -- 'task', 'inference_profile', 'fluent_message'
    object_name TEXT NOT NULL,  -- the name attribute from the directive
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_shared_content_objects_shared_path
    ON rev.shared_content_objects (shared_path_id);
CREATE INDEX idx_shared_content_objects_target_path
    ON rev.shared_content_objects (target_path_id);
```

This table answers: "What objects does the shared content at ref_key=X provide?" and "Which shared content package does object Y belong to?"

## 8. Implementation Guidance

### Phase 1: Parser Layer (`dapple-parser`)

**New function: `parse_shared_content(shared_dir: &Path) -> Result<ParsedGuide, ParseError>`**

Location: `crates/dapple-parser/src/lib.rs` (alongside `parse_guide`)

This function is nearly identical to `parse_guide()` but:
1. Reads `shared.toml` instead of `guide.toml` / `book.toml`
2. Maps `SharedToml` fields to `GuideMetadata` (name -> title, description -> description, no card)
3. Reuses `resolve_summary_path()`, `parse_chapter_files()`, `discover_assets()`, `ftl::discover_and_parse_ftl_files()`
4. Returns a `ParsedGuide` with the shared content's chapters and blocks

The rationale for returning `ParsedGuide` instead of a new type: the downstream `plan_guide()` already handles all the block extraction logic, and we want to reuse it. The shared content plan is a subset of the guide plan.

**New function: `plan_shared_content(parsed: &ParsedGuide, shared_toml: &SharedToml, guide_dir: &Path) -> Result<SharedContentPlan, ParseError>`**

Location: `crates/dapple-parser/src/transform/mod.rs`

This function:
1. Calls internal helpers from `plan_guide()` to extract derived objects
2. Constructs `ref_key` from `{scope}/{name}`
3. Filters out playbook and brief_pack derived objects (warns if present)
4. Returns `SharedContentPlan`

Alternatively, the handler can call `plan_guide()` and then extract the relevant subset into a `SharedContentPlan`. This avoids code duplication in the parser layer.

**Recommended approach:** Call `plan_guide()` directly and construct `SharedContentPlan` in the handler by pulling relevant fields from the `IngestPlan`. This keeps the parser layer unchanged.

### Phase 2: Handler Layer (`dapple-prompt-api`)

**File: `crates/dapple-prompt-api/src/handler/shared_ingest.rs`**

Replace the stub with a handler that follows this flow:

```
1. extract_tarball_field(multipart)           -- reuse from guide_ingest.rs
2. validate_scope(scope)                      -- check against shared_scopes
3. decompress_and_plan_shared(bytes, scope)   -- extract, parse shared.toml, parse guide, plan
4. [if dry_run] return validation summary
5. materialize_shared_content(data, plan)     -- create object_path, derived objects
```

**Key implementation details:**

`decompress_and_plan_shared()`:
- Extract tarball (reuse security checks from `decompress_and_plan()`)
- Find shared content root: look for `shared.toml` instead of `guide.toml`
- Read and parse `shared.toml` via `dapple_parser::shared::parse_shared_toml()`
- Validate scope matches query parameter
- Temporarily create a synthetic `guide.toml` or modify the `parse_guide()` call to accept `shared.toml` metadata
- Call `parse_guide()` on the shared content directory (after creating a minimal `guide.toml` equivalent, or by adding a `parse_shared_content()` to the parser)
- Call `plan_guide()` to get an `IngestPlan`
- Convert to `SharedContentPlan`

**Alternative simpler approach for finding the root:**
Rather than requiring `guide.toml`, add a `find_shared_root()` function parallel to `find_guide_root()` that looks for `shared.toml`.

`materialize_shared_content()`:
1. Create or find the `object_path` for this `ref_key` (upsert pattern)
2. Upload assets via `upload_assets()` (reuse from `guide_ingest.rs`)
3. Materialize Fluent messages (reuse `materialize_fluent_messages()` from `guide_ingest.rs`)
4. Materialize tasks (reuse `materialize_tasks()` from `guide_ingest.rs`)
5. Materialize inference profiles (reuse `materialize_inference_profiles()` from `guide_ingest.rs`)
6. Create `shared_content_objects` associations (new)
7. Return `SharedIngestResponse`

### Phase 3: Data Layer (`dapple-data`)

**New SQL files:**

| File | Purpose |
|------|---------|
| `assets/sql/shared/validate_scope.sql` | Check scope against `shared_scopes` lookup |
| `assets/sql/shared/select_or_insert_shared_object_path.sql` | Upsert shared content `object_path` |
| `assets/sql/shared/insert_shared_content_objects.sql` | Insert associations between shared path and derived objects |

**New query functions in `crates/dapple-data/src/query/shared.rs`:**

```rust
/// Validate that a scope key exists in shared_scopes.
pub async fn validate_scope(pool: &DbPool, scope: &str) -> Result<bool, DataError>;

/// Find or create an object_path for shared content.
pub async fn select_or_insert_shared_object_path(
    pool: &DbPool,
    entity_type_key: &str,
    scope: &str,
    ref_key: &str,
) -> Result<SharedPathRow, DataError>;

/// Insert shared_content_objects associations.
pub async fn insert_shared_content_objects(
    pool: &DbPool,
    args: InsertSharedContentObjectsArgs,
) -> Result<Vec<InsertedSharedContentObjectModel>, DataError>;
```

**New model types (in `crates/dapple-data/src/query/shared.rs` or new model file):**

```rust
pub struct InsertSharedContentObjectsArgs {
    pub shared_path_ids: Vec<i64>,
    pub target_path_ids: Vec<i64>,
    pub object_kinds: Vec<String>,
    pub object_names: Vec<String>,
}

pub struct InsertedSharedContentObjectModel {
    pub id: i64,
    pub object_id: Uuid,
    pub shared_path_id: i64,
    pub target_path_id: i64,
    pub object_kind: String,
    pub object_name: String,
    pub created_at: OffsetDateTime,
}
```

### Module Organization Summary

| Layer | File | Action |
|-------|------|--------|
| Parser | `crates/dapple-parser/src/lib.rs` | Add `parse_shared_content()` |
| Parser | `crates/dapple-parser/src/transform/mod.rs` | Add `SharedContentPlan` struct |
| Data | `crates/dapple-data/migrations/YYYYMMDD_shared_content_entity_type.sql` | Seed `shared_content` entity type |
| Data | `crates/dapple-data/migrations/YYYYMMDD_shared_content_objects.sql` | Create `rev.shared_content_objects` table |
| Data | `crates/dapple-data/assets/sql/shared/validate_scope.sql` | New SQL |
| Data | `crates/dapple-data/assets/sql/shared/select_or_insert_shared_object_path.sql` | New SQL |
| Data | `crates/dapple-data/assets/sql/shared/insert_shared_content_objects.sql` | New SQL |
| Data | `crates/dapple-data/src/query/shared.rs` | Add new query functions |
| API | `crates/dapple-prompt-api/src/handler/shared_ingest.rs` | Replace stub with full implementation |
| API | `crates/dapple-prompt-api/src/schema/shared_ingest.rs` | Already complete (no changes needed) |

### Key Implementation Considerations

**Reuse over duplication:** The `extract_tarball_field()` function from `guide_ingest.rs` should be extracted to a shared utility module (e.g., `handler/ingest_util.rs`) rather than duplicated. Similarly, `upload_assets()`, `materialize_fluent_messages()`, `materialize_tasks()`, and `materialize_inference_profiles()` should be made `pub(crate)` and importable from `guide_ingest.rs` or extracted to a shared module.

**Transaction boundaries:** All materialization steps (object_path creation, derived objects, associations) should execute within a single database transaction to ensure atomicity. If any step fails, the entire ingest rolls back.

**Idempotent re-ingest:** Re-ingesting the same `ref_key` should be supported. The handler checks if an `object_path` with the given `ref_key` exists. If it does, it creates new versions of the derived objects (append-only). The `shared_content_objects` entries for the new version are appended, not updated.

**Scope validation at two levels:**
1. The `scope` query parameter must match a valid `rev.shared_scopes` entry
2. The `scope` in `shared.toml` must match the query parameter (defense in depth)

**Error handling:** Follow the same pattern as `guide_ingest.rs`: return `Result<SharedIngestResponse, (StatusCode, String)>` from the materialization function, mapped to JSON error responses in the handler.

### Deferred to Implementation Agent

- Exact Rust function signatures (follow existing patterns in `guide_ingest.rs`)
- Whether to extract shared utilities to `handler/ingest_util.rs` or keep helpers in `guide_ingest.rs` as `pub(crate)`
- Specific error messages and validation logic
- The exact `find_shared_root()` implementation (mirror `find_guide_root()`)
- Test data generation details
- Whether `parse_shared_content()` should synthesize a `GuideMetadata` from `SharedToml` or introduce a new parse path

## 9. Testing Strategy

### Unit Tests (Parser)

**In `crates/dapple-parser/src/lib.rs` or a new test module:**

```rust
#[test]
fn test_parse_shared_content_basic() {
    // Create temp dir with shared.toml, SUMMARY.md, chapter.md
    // Verify ParsedGuide is returned with correct title, chapters, blocks
}

#[test]
fn test_parse_shared_content_missing_shared_toml() {
    // Verify ParseError::MissingSharedToml returned
}

#[test]
fn test_parse_shared_content_with_locales() {
    // Create locales/en.ftl, verify locales populated
}
```

### Integration Tests

**In `crates/dapple-prompt-api/tests/`:**

```rust
fn trial_shared_content_ingest_dry_run() -> Trial {
    // POST tarball with dry_run=true
    // Verify 200 OK with validation summary
    // Verify blocks/chapters/assets counts
}

fn trial_shared_content_ingest_common() -> Trial {
    // POST tarball with scope=common
    // Verify 201 Created
    // Verify ref_key = "common/{name}"
    // Verify object_path exists with correct scope
}

fn trial_shared_content_ingest_internal() -> Trial {
    // POST tarball with scope=internal
    // Verify 201 Created
    // Verify ref_key = "internal/{name}"
}

fn trial_shared_content_ingest_scope_mismatch() -> Trial {
    // shared.toml says scope=internal, query param says common
    // Verify 422 Unprocessable Entity
}

fn trial_shared_content_ingest_invalid_scope() -> Trial {
    // scope=invalid in query params
    // Verify 422 Unprocessable Entity
}

fn trial_shared_content_reingest() -> Trial {
    // Ingest same ref_key twice
    // Verify second ingest succeeds (new version of derived objects)
    // Verify same path_object_id returned
}

fn trial_guide_with_shared_ref() -> Trial {
    // Wait for: shared-content-common-init batch
    // Ingest a guide that has :::stage { ref: "common/validate-output" }
    // Verify guide_shared_refs row created
    // Verify shared_ref resolves to the correct object_path
}
```

### Test Tarball Construction

The test consumer should construct tarballs programmatically:

```rust
fn build_shared_content_tarball(name: &str, scope: &str) -> Vec<u8> {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().join(name);
    std::fs::create_dir_all(&root).unwrap();

    // shared.toml
    std::fs::write(
        root.join("shared.toml"),
        format!("name = \"{name}\"\nscope = \"{scope}\"\n"),
    ).unwrap();

    // SUMMARY.md
    std::fs::write(root.join("SUMMARY.md"), "- [Main](./main.md)\n").unwrap();

    // Chapter with a nushell task
    std::fs::write(
        root.join("main.md"),
        ":::nushell { name: \"shared-validate\" }\necho 'ok'\n:::\n",
    ).unwrap();

    // Compress to tar.gz
    let mut buf = Vec::new();
    {
        let enc = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_dir_all(name, &root).unwrap();
        tar.finish().unwrap();
    }
    buf
}
```

### Verification Points

- POST with `dry_run=true` returns correct block/chapter/asset/fluent_message counts
- POST with `dry_run=false` creates `object_path` with correct `scope` and `ref_key`
- Derived tasks are created and queryable
- Derived Fluent messages are created with correct locale bodies
- Derived inference profiles are created
- Assets are uploaded via the draft/chunk/finalize pipeline
- Re-ingest of same `ref_key` creates new derived object versions (append-only)
- Scope mismatch between `shared.toml` and query parameter returns 422
- Invalid scope value returns 422
- Guide ingest with `:::stage { ref: "common/validate-output" }` creates `guide_shared_refs` row

## 10. AGENTS.md Updates

The following guidance should be added to `crates/dapple-prompt-api/AGENTS.md` under a new "Shared Content" section:

```markdown
## Shared Content Ingest

When implementing features related to shared content:

1. **Shared content uses `shared.toml` not `guide.toml`** -- The parser detects shared content
   directories by the presence of `shared.toml`
2. **The ref_key is `{scope}/{name}`** -- Constructed from the query parameter scope and the
   `name` field in `shared.toml`
3. **Scope must be validated twice** -- Check the query parameter against `shared_scopes` lookup
   AND verify it matches `shared.toml`
4. **Re-ingest is idempotent at the path level** -- The same `ref_key` maps to the same
   `object_path`; new derived objects are created as new versions
5. **No guide entity is created** -- Shared content creates an `object_path` with derived objects,
   not a full guide with chapters/blocks
6. **Reuse guide ingest helpers** -- `upload_assets()`, `materialize_fluent_messages()`,
   `materialize_tasks()`, `materialize_inference_profiles()` are shared with `guide_ingest.rs`

See `docs/plans/shared_content_ingest_architecture_plan.md` for full details.
```

## 11. Open Questions and Risks

### Open Questions

1. **Playbook stages in shared content:** Should shared content be able to define complete playbook stages that guides reference? The current plan only materializes tasks, inference profiles, and Fluent messages. If a guide does `:::stage { ref: "common/validate-output" }`, what exactly gets resolved -- the stage definition with its child steps, or just a named reference? This needs clarification from the product side. The current plan treats it as a reference linkage (the shared content defines the objects, the guide's `:::stage { ref }` block creates a `guide_shared_refs` entry pointing to the shared path).

2. **Shared content versioning:** When shared content is re-ingested, should derived objects be re-created or should the handler check for diffs? The append-only philosophy suggests always creating new versions. This is the recommended approach.

3. **Shared content deprecation:** Should shared content paths support deprecation? The `object_paths` table supports deprecation via `versioned_objects`. This is a future consideration and is not needed for the initial implementation.

4. **Brief packs in shared content:** The plan currently warns and ignores `:::brief_pack` blocks in shared content. Should these be supported in the future? BriefPack materialization is already deferred in the guide ingest pipeline, so this is consistent.

5. **Scope upgrade path:** Can shared content initially ingested as `internal` be promoted to `common`? This would require changing the `scope` on the `object_path`, which is an UPDATE operation (prohibited by convention). The recommended approach is to create a new `object_path` with scope `common` and the same content, then deprecate the internal one.

### Risks

1. **Parser coupling:** Adding `parse_shared_content()` to the parser creates a second parse entry point. If the guide parsing logic changes (new directive types, new metadata fields), both paths must be updated. Mitigation: `parse_shared_content()` delegates to the same internal functions as `parse_guide()`.

2. **Transactional atomicity:** The materialization step creates objects across multiple tables. A partial failure could leave orphaned `object_path` rows. Mitigation: wrap all materialization in a single database transaction.

3. **ref_key uniqueness:** The unique index on `object_paths.ref_key` prevents duplicate shared content. If two developers simultaneously try to ingest the same `ref_key`, one will get a unique constraint violation. Mitigation: the upsert query handles this gracefully by returning the existing row.

4. **Guide ingest dependency:** When a guide is ingested with `:::stage { ref: "common/validate-output" }`, the shared content must already be ingested. If the shared content does not exist, the guide ingest should warn but not fail (the `ref` block is stored as-is and can be resolved later when the shared content is ingested). The current implementation in `extract_derived_from_block()` already handles this -- blocks with `ref_path` are recorded as `SharedRefDef` entries and do not produce derived objects.
