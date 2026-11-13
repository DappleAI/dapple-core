# PATCH Routes Implementation Plan

**Status:** Ready for Implementation
**Created:** 2026-01-17
**Updated:** 2026-01-18
**Author:** Claude
**Prerequisites:** Versioned Objects Refactoring Plan (completed)
**Blocks:** None

---

## Summary

This plan covers implementing PATCH routes for all versioned entities. PATCH creates a new version of an object by inserting into `versioned_objects` and the entity-specific table. Path-level data (name, slug) remains immutable.

---

## Architecture Overview

With the unified versioning system:

```
┌──────────────────────────────────────┐
│         PATCH /api/briefs            │
│  { object_id, title, description }   │
└──────────────────┬───────────────────┘
                   │
                   ▼
┌──────────────────────────────────────┐
│       rev.versioned_objects          │
│  INSERT new row with version+1       │
│  (same path_id, incremented patch)   │
└──────────────────┬───────────────────┘
                   │
                   ▼
┌──────────────────────────────────────┐
│           rev.briefs                 │
│  INSERT new row with new fields      │
│  (references new versioned_object)   │
└──────────────────────────────────────┘
```

**Key Points:**
- PATCH creates NEW rows (append-only)
- `versioned_objects.path_id` stays the same (same lineage)
- `versioned_objects.version_patch` increments
- `versioned_objects.object_id` is NEW (new version identity)
- Path-level data (name, slug in `*_path_details`) is IMMUTABLE

---

## What Can Be PATCHed

| Entity | Mutable Fields (PATCH allowed) | Immutable Path Fields |
|--------|-------------------------------|----------------------|
| Briefs | title, description | name |
| Brief Groups | title, brief_object_ids | name |
| Brief Packs | title, brief_group_object_ids | name |
| Assets | name, description, content_type, data | asset_kind_id |
| Value Slots | value | name, value_kind_id |
| Fluent Messages | content, engine_bodies | name, description |
| Prompts | description, fluent_message_object_ids | name |
| Tasks | description, function, function_input, prompt_object_ids | name, slug |
| Steps | description, task_object_ids | name, slug |
| Stages | description | name, slug |
| Categories | description | name, slug |
| Hats | description, category_object_ids | name, slug |
| Playbooks | description, structure (TBD) | name, slug |

---

## Entities with Existing PATCH (Update for New Architecture)

These 6 entities have PATCH implementations that need updating:

| Entity | Current Implementation | Changes Needed |
|--------|----------------------|----------------|
| Briefs | `patch_briefs.sql` | Update to use versioned_objects |
| Assets | `patch_assets.sql` | Update to use versioned_objects |
| Value Slots | `patch_value_slots.sql` | Update to use versioned_objects |
| Fluent Messages | `patch_fluent_messages.sql` | Update to use versioned_objects |
| Brief Groups | `patch_brief_groups.sql` | Update to use versioned_objects |
| Brief Packs | `patch_brief_packs.sql` | Update to use versioned_objects |

---

## Entities Needing New PATCH Implementation

These 7 entities need PATCH routes created:

| Entity | Complexity | Notes |
|--------|------------|-------|
| Stages | Simple | Description only |
| Categories | Simple | Description only |
| Prompts | Medium | description + fluent_message associations |
| Hats | Medium | description + category associations |
| Tasks | Medium | description + function fields + prompt associations |
| Steps | Complex | description + task associations with ordinals |
| Playbooks | Complex | description + full structure (TBD) |

---

## SQL Pattern (New Architecture)

### PATCH Query Template

```sql
-- assets/sql/briefs/patch_briefs.sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],      -- object_ids (version to base on)
        $2::TEXT[],      -- titles
        $3::TEXT[]       -- descriptions
    ) WITH ORDINALITY AS o(object_id, title, description, ord)
),
-- Look up existing version and path
existing AS (
    SELECT
        vo.id as old_versioned_object_id,
        vo.path_id,
        vo.entity_type_id,
        vo.version_major,
        vo.version_minor,
        vo.version_patch,
        b.title as old_title,
        b.description as old_description,
        args.title as new_title,
        args.description as new_description,
        args.ord
    FROM args
    JOIN rev.versioned_objects vo ON vo.object_id = args.object_id
    JOIN rev.briefs b ON b.versioned_object_id = vo.id
),
-- Get latest version for each path (may differ from args if multiple versions exist)
latest_versions AS (
    SELECT
        e.path_id,
        MAX(vo2.version_major) as latest_major,
        MAX(vo2.version_minor) FILTER (WHERE vo2.version_major = MAX(vo2.version_major)) as latest_minor,
        MAX(vo2.version_patch) FILTER (
            WHERE vo2.version_major = MAX(vo2.version_major)
            AND vo2.version_minor = MAX(vo2.version_minor) FILTER (WHERE vo2.version_major = MAX(vo2.version_major))
        ) as latest_patch
    FROM existing e
    JOIN rev.versioned_objects vo2 ON vo2.path_id = e.path_id
    GROUP BY e.path_id
),
-- Insert new versioned_object (increments patch)
new_versions AS (
    INSERT INTO rev.versioned_objects (
        path_id,
        entity_type_id,
        version_major,
        version_minor,
        version_patch
    )
    SELECT
        e.path_id,
        e.entity_type_id,
        lv.latest_major,
        lv.latest_minor,
        lv.latest_patch + 1
    FROM existing e
    JOIN latest_versions lv ON lv.path_id = e.path_id
    RETURNING id, object_id, path_id, version_major, version_minor, version_patch, created_at
),
-- Insert new brief with updated fields
new_briefs AS (
    INSERT INTO rev.briefs (versioned_object_id, title, description)
    SELECT
        nv.id,
        COALESCE(e.new_title, e.old_title),
        COALESCE(e.new_description, e.old_description)
    FROM new_versions nv
    JOIN existing e ON e.path_id = nv.path_id
    RETURNING id, versioned_object_id, title, description, created_at
)
SELECT
    nb.id,
    nv.object_id,
    op.object_id as path_object_id,
    bpd.name,
    nb.title,
    nb.description,
    nv.version_major,
    nv.version_minor,
    nv.version_patch,
    nb.created_at,
    -- Deprecation (new version starts active)
    'active' as deprecation_level,
    'active' as effective_deprecation_level,
    e.ord
FROM new_briefs nb
JOIN new_versions nv ON nv.id = nb.versioned_object_id
JOIN rev.object_paths op ON op.id = nv.path_id
JOIN rev.brief_path_details bpd ON bpd.path_id = op.id
JOIN existing e ON e.path_id = nv.path_id
ORDER BY e.ord;
```

### PATCH with Associative Tables (Brief Groups Example)

```sql
-- assets/sql/brief_groups/patch_brief_groups.sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],       -- object_ids
        $2::TEXT[],       -- titles
        $3::UUID[][]      -- brief_object_ids per group (2D array)
    ) WITH ORDINALITY AS o(object_id, title, brief_object_ids, ord)
),
existing AS (
    SELECT
        vo.id as old_versioned_object_id,
        vo.path_id,
        vo.entity_type_id,
        bg.title as old_title,
        args.title as new_title,
        args.brief_object_ids,
        args.ord
    FROM args
    JOIN rev.versioned_objects vo ON vo.object_id = args.object_id
    JOIN rev.brief_groups bg ON bg.versioned_object_id = vo.id
),
latest_versions AS (
    SELECT
        e.path_id,
        MAX(vo2.version_patch) as latest_patch
        -- Simplified: assuming all same major.minor
    FROM existing e
    JOIN rev.versioned_objects vo2 ON vo2.path_id = e.path_id
    GROUP BY e.path_id
),
-- Validate no retired/quarantined briefs
blocked_check AS (
    SELECT
        e.ord,
        ref.brief_object_id,
        (
            SELECT dl.severity
            FROM rev.deprecation_events de
            JOIN rev.deprecation_levels dl ON dl.id = de.deprecation_level_id
            WHERE de.versioned_object_id = bvo.id
            ORDER BY de.created_at DESC
            LIMIT 1
        ) as severity
    FROM existing e
    CROSS JOIN LATERAL UNNEST(e.brief_object_ids) AS ref(brief_object_id)
    JOIN rev.versioned_objects bvo ON bvo.object_id = ref.brief_object_id
    WHERE (
        SELECT dl.severity
        FROM rev.deprecation_events de
        JOIN rev.deprecation_levels dl ON dl.id = de.deprecation_level_id
        WHERE de.versioned_object_id = bvo.id
        ORDER BY de.created_at DESC
        LIMIT 1
    ) >= 2  -- retired or quarantined
),
-- Fail if blocked refs (application handles this error)
validate AS (
    SELECT CASE WHEN EXISTS (SELECT 1 FROM blocked_check) THEN 1/0 ELSE 1 END
),
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, entity_type_id, version_major, version_minor, version_patch)
    SELECT e.path_id, e.entity_type_id, 0, 0, lv.latest_patch + 1
    FROM existing e
    JOIN latest_versions lv ON lv.path_id = e.path_id
    CROSS JOIN validate  -- Force validation
    RETURNING id, object_id, path_id, version_major, version_minor, version_patch, created_at
),
new_groups AS (
    INSERT INTO rev.brief_groups (versioned_object_id, title)
    SELECT nv.id, COALESCE(e.new_title, e.old_title)
    FROM new_versions nv
    JOIN existing e ON e.path_id = nv.path_id
    RETURNING id, versioned_object_id, title, created_at
),
-- Resolve brief object_ids → internal ids
brief_refs AS (
    SELECT
        e.ord as group_ord,
        ng.id as group_id,
        b.id as brief_id,
        ref.ordinal
    FROM existing e
    JOIN new_versions nv ON nv.path_id = e.path_id
    JOIN new_groups ng ON ng.versioned_object_id = nv.id
    CROSS JOIN LATERAL UNNEST(e.brief_object_ids) WITH ORDINALITY AS ref(object_id, ordinal)
    JOIN rev.versioned_objects bvo ON bvo.object_id = ref.object_id
    JOIN rev.briefs b ON b.versioned_object_id = bvo.id
),
-- Insert new associations
new_assocs AS (
    INSERT INTO rev.brief_group_briefs (brief_group_id, brief_id, ordinal)
    SELECT group_id, brief_id, ordinal::INT
    FROM brief_refs
    RETURNING *
)
SELECT
    ng.id,
    nv.object_id,
    op.object_id as path_object_id,
    bgpd.name,
    ng.title,
    nv.version_major,
    nv.version_minor,
    nv.version_patch,
    ng.created_at,
    'active' as deprecation_level,
    'active' as effective_deprecation_level,
    e.ord
FROM new_groups ng
JOIN new_versions nv ON nv.id = ng.versioned_object_id
JOIN rev.object_paths op ON op.id = nv.path_id
JOIN rev.brief_group_path_details bgpd ON bgpd.path_id = op.id
JOIN existing e ON e.path_id = nv.path_id
ORDER BY e.ord;
```

---

## Model Changes

### PatchArgs Structs

```rust
// crates/dapple-data/src/model/briefs.rs

#[derive(Clone, Debug)]
pub struct PatchBriefArgs {
    pub titles: Vec<Option<String>>,       // None = keep existing
    pub descriptions: Vec<Option<String>>, // None = keep existing
}

// For entities with associations
#[derive(Clone, Debug)]
pub struct PatchBriefGroupArgs {
    pub titles: Vec<Option<String>>,
    pub brief_object_ids: Vec<Option<Vec<Uuid>>>, // None = keep existing associations
}
```

### Response Models

All PATCH responses now include:
- New `object_id` (changed - this is a new version)
- Same `path_object_id` (unchanged - same lineage)
- Incremented version
- `deprecation_level: "active"` (new versions start active)

---

## Schema Changes

### Request Schemas

```rust
// crates/dapple-prompt-api/src/schema/briefs.rs

/// PATCH request - all fields optional (null = keep existing)
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchBriefSchema {
    pub object_id: Uuid,  // Version to base on
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchBriefSetSchema {
    pub objs: Vec<PatchBriefSchema>,
}

// For entities with associations
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct PatchBriefGroupSchema {
    pub object_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New brief references (replaces existing). Null = keep existing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_object_ids: Option<Vec<Uuid>>,
}
```

---

## Handler Pattern

```rust
// crates/dapple-prompt-api/src/handler/briefs.rs

pub async fn briefs_patch_handler(
    State(data): State<Arc<AppState>>,
    Json(body): Json<PatchBriefSetSchema>,
) -> impl IntoApiResponse {
    let object_ids: Vec<Uuid> = body.objs.iter().map(|o| o.object_id).collect();

    let args = PatchBriefArgs {
        titles: body.objs.iter().map(|o| o.title.clone()).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
    };

    post_response_handler("briefs", patch_briefs(&data.db, object_ids, args).await)
}
```

---

## Implementation Phases

### Phase 1: Update Existing PATCH Implementations

Update these 6 entities to use new `versioned_objects` pattern:

1. **Briefs** - Simple fields only
2. **Assets** - Simple fields only
3. **Value Slots** - Simple fields only
4. **Fluent Messages** - With engine_bodies sub-table
5. **Brief Groups** - With brief associations
6. **Brief Packs** - With brief_group associations

For each:
- Update SQL query to use `versioned_objects`
- Add deprecation fields to response
- Add retired/quarantined blocking for associations
- Update tests

### Phase 2: Simple New PATCH Implementations

1. **Stages** - Description only
2. **Categories** - Description only

For each:
- Create `patch_{entity}.sql`
- Add `Patch{Entity}Args` to model
- Add `patch_{entity}()` to query module
- Add `Patch{Entity}Schema`, `Patch{Entity}SetSchema`
- Add `{entity}_patch_handler()`
- Register route

### Phase 3: Medium Complexity PATCH

1. **Prompts** - Description + fluent_message associations
2. **Hats** - Description + category associations
3. **Tasks** - Description + function fields + prompt associations

### Phase 4: Complex PATCH

1. **Steps** - Description + task associations (with ordinals)
2. **Playbooks** - Description only initially (full structure TBD)

---

## Files to Modify (Existing PATCH)

| Entity | SQL | Model | Query | Schema | Handler |
|--------|-----|-------|-------|--------|---------|
| Briefs | UPDATE | UPDATE | UPDATE | UPDATE | UPDATE |
| Assets | UPDATE | UPDATE | UPDATE | UPDATE | UPDATE |
| Value Slots | UPDATE | UPDATE | UPDATE | UPDATE | UPDATE |
| Fluent Messages | UPDATE | UPDATE | UPDATE | UPDATE | UPDATE |
| Brief Groups | UPDATE | UPDATE | UPDATE | UPDATE | UPDATE |
| Brief Packs | UPDATE | UPDATE | UPDATE | UPDATE | UPDATE |

## Files to Create (New PATCH)

| Entity | SQL | Model | Query | Schema | Handler | Route |
|--------|-----|-------|-------|--------|---------|-------|
| Stages | CREATE | MODIFY | MODIFY | MODIFY | MODIFY | MODIFY |
| Categories | CREATE | MODIFY | MODIFY | MODIFY | MODIFY | MODIFY |
| Prompts | CREATE | (exists) | MODIFY | CREATE | MODIFY | MODIFY |
| Hats | CREATE | MODIFY | MODIFY | MODIFY | MODIFY | MODIFY |
| Tasks | CREATE | MODIFY | MODIFY | MODIFY | MODIFY | MODIFY |
| Steps | CREATE | MODIFY | MODIFY | MODIFY | MODIFY | MODIFY |
| Playbooks | CREATE | MODIFY | MODIFY | MODIFY | MODIFY | MODIFY |

---

## Validation Rules

### On PATCH

1. **Object must exist** - Return 404 if object_id not found
2. **Object must be active or deprecated** - Cannot PATCH retired/quarantined
3. **Referenced objects must be active or deprecated** - Cannot add retired/quarantined references
4. **At least one field must change** - Warn if PATCH is no-op (optional)

### Version Increment Strategy

- PATCH always increments `version_patch`
- Major/minor version changes require explicit API (future)
- New version starts with `deprecation_level = 'active'`

---

## Error Responses

```json
// Object not found
{
  "error": "NOT_FOUND",
  "message": "Brief with object_id '...' not found"
}

// Cannot PATCH deprecated object
{
  "error": "OBJECT_RETIRED",
  "message": "Cannot PATCH retired object. Create a new path instead."
}

// Reference blocked
{
  "error": "REFERENCE_BLOCKED",
  "message": "Cannot reference retired objects",
  "details": {
    "blocked_references": [...]
  }
}
```

---

## Verification

### Per-Entity Verification

```bash
# 1. Verify SQL compiles
cargo sqlx prepare --workspace

# 2. Run unit tests
cargo test --package dapple-data

# 3. Run integration tests
cargo test --package dapple-prompt-api --test user_lifecycle

# 4. Manual verification
curl -X PATCH http://localhost:3000/api/briefs \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "object_id": "existing-brief-uuid",
      "title": "Updated Title"
    }]
  }'

# Verify response has:
# - NEW object_id (different from request)
# - SAME path_object_id
# - Incremented version_patch
# - deprecation_level: "active"
```

---

## Open Questions

1. **PATCH on deprecated objects:** Allow (creates non-deprecated version) or block?
   - Recommendation: Allow - user may want to create fixed version

2. **Partial association update:** Should `brief_object_ids: null` mean:
   - Keep existing associations (current recommendation)
   - Clear all associations
   - Use empty array `[]` for clearing?

3. **Concurrent PATCH:** If two PATCHes on same path happen simultaneously:
   - Both succeed with different version numbers (current approach)
   - Second one fails with conflict error
   - Second one retries with merged version

---

## Notes

- PATCH is append-only - old version remains accessible
- New version starts active regardless of old version's deprecation
- Path-level data (name, slug) cannot be changed via PATCH
- To "rename" an object, create a new path entirely
- Version numbers may have gaps (e.g., 0.0.1 → 0.0.3 if 0.0.2 was created by another PATCH)
