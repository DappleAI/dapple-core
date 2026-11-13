# Versioned Objects Refactoring Plan

**Status:** Completed
**Created:** 2026-01-18
**Updated:** 2026-01-18
**Author:** Claude
**Prerequisites:** None (foundation for other plans)
**Blocks:** PATCH Routes Plan, DELETE/Deprecation Plan
**Implementation Branch:** unified-versioned-relational-objects

---

## Summary

This plan introduces a unified versioning and deprecation architecture by creating centralized `object_paths`, `versioned_objects`, and `deprecation_events` tables. All 13 versioned entities will reference these central tables, enabling consistent version tracking and a single deprecation system across the entire data model.

---

## Motivation

### Current State Problems

1. **13 separate path tables** with duplicated structure and varying column names
2. **13 separate versioned tables** each with their own version_major/minor/patch columns
3. **No deprecation system** - cannot mark versions as deprecated/retired/quarantined
4. **Inconsistent patterns** - some paths have name/slug, others have name only, others have FKs to lookup tables
5. **Cross-entity queries are hard** - "show all v0.0.1 objects" requires 13 separate queries

### Desired State Benefits

1. **Single source of truth** for version information
2. **Unified deprecation system** with poison propagation
3. **Consistent cross-entity queries** via `versioned_objects` table
4. **Cleaner entity tables** - only entity-specific fields remain
5. **Audit trail** for deprecation changes

---

## Architecture

### Core Tables

```
┌─────────────────────────────────────────────────────────────────┐
│                    rev.entity_types (lookup)                    │
│  id (SMALLINT), name (TEXT), created_at                        │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                      rev.object_paths                           │
│  id, object_id, entity_type_id (FK), created_at                │
└─────────────────────┬───────────────────────────────────────────┘
                      │
                      │ 1:1 (optional, where entity has path-level data)
                      ▼
┌─────────────────────────────────────────────────────────────────┐
│              rev.{entity}_path_details                          │
│  path_id (FK → object_paths, PK), name, slug, ...              │
└─────────────────────────────────────────────────────────────────┘
                      │
                      │ 1:many (path → versions)
                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                   rev.versioned_objects                         │
│  id, object_id, path_id (FK), entity_type_id (FK),             │
│  version_major, version_minor, version_patch, created_at       │
└─────────────────────┬───────────────────────────────────────────┘
                      │
                      │ 1:1 (versioned_object → entity-specific data)
                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                    rev.briefs (etc.)                            │
│  id, versioned_object_id (FK), title, description, ...         │
│  (entity-specific versioned fields only)                       │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                rev.deprecation_levels (lookup)                  │
│  id (SMALLINT), name, severity, created_at                     │
│  Values: active(0), deprecated(1), retired(2), quarantined(3)  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                  rev.deprecation_events                         │
│  id, versioned_object_id (FK), deprecation_level_id (FK),      │
│  reason (TEXT), created_at, created_by (FK → users)            │
└─────────────────────────────────────────────────────────────────┘
```

### Deprecation Semantics

**Levels (in order of severity):**

| Level | Severity | Behavior |
|-------|----------|----------|
| `active` | 0 | Normal operation, no restrictions |
| `deprecated` | 1 | Courtesy warning. New objects CAN still reference. |
| `retired` | 2 | Strict warning. New objects CANNOT reference. Existing refs still work. |
| `quarantined` | 3 | Hard failure. Not usable in any context. |

**Poison Propagation:**
- Direction: Upstream (dependencies poison dependents)
- Timing: Lazy (computed at query time)
- Example: If Brief B is `retired`, any BriefGroup containing B has effective level >= `retired`

**Computing Effective Deprecation Level:**
```sql
-- For a versioned object, compute max(own_level, max(dependency_levels))
-- This is done at query time, not stored
```

---

## DDL Changes

### 1. Lookup Tables

```sql
-- Migration: YYYYMMDD_create_entity_types.sql

CREATE TABLE rev.entity_types (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.entity_types (name) VALUES
    ('task'),
    ('step'),
    ('stage'),
    ('playbook'),
    ('category'),
    ('hat'),
    ('prompt'),
    ('fluent_message'),
    ('brief'),
    ('brief_group'),
    ('brief_pack'),
    ('value_slot'),
    ('asset');

ALTER TABLE rev.entity_types OWNER TO dapple;
```

```sql
-- Migration: YYYYMMDD_create_deprecation_levels.sql

CREATE TABLE rev.deprecation_levels (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    severity SMALLINT NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.deprecation_levels (name, severity, description) VALUES
    ('active', 0, 'Normal operation, no restrictions'),
    ('deprecated', 1, 'Courtesy warning, new references allowed'),
    ('retired', 2, 'Strict warning, new references blocked'),
    ('quarantined', 3, 'Hard failure, unusable');

ALTER TABLE rev.deprecation_levels OWNER TO dapple;
```

### 2. Core Tables

```sql
-- Migration: YYYYMMDD_create_object_paths.sql

CREATE TABLE rev.object_paths (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    entity_type_id SMALLINT NOT NULL REFERENCES rev.entity_types (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

ALTER TABLE rev.object_paths OWNER TO dapple;

CREATE INDEX object_paths_entity_type_idx ON rev.object_paths (entity_type_id);
```

```sql
-- Migration: YYYYMMDD_create_versioned_objects.sql

CREATE TABLE rev.versioned_objects (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    path_id BIGINT NOT NULL REFERENCES rev.object_paths (id),
    entity_type_id SMALLINT NOT NULL REFERENCES rev.entity_types (id),
    version_major SMALLINT NOT NULL DEFAULT 0,
    version_minor SMALLINT NOT NULL DEFAULT 0,
    version_patch SMALLINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,

    UNIQUE (path_id, version_major, version_minor, version_patch)
);

ALTER TABLE rev.versioned_objects OWNER TO dapple;

CREATE INDEX versioned_objects_path_idx ON rev.versioned_objects (path_id);
CREATE INDEX versioned_objects_entity_type_idx ON rev.versioned_objects (entity_type_id);
CREATE INDEX versioned_objects_version_idx ON rev.versioned_objects (version_major, version_minor, version_patch);
```

```sql
-- Migration: YYYYMMDD_create_deprecation_events.sql

CREATE TABLE rev.deprecation_events (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    versioned_object_id BIGINT NOT NULL REFERENCES rev.versioned_objects (id),
    deprecation_level_id SMALLINT NOT NULL REFERENCES rev.deprecation_levels (id),
    reason TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    created_by BIGINT  -- FK to users table if applicable
);

ALTER TABLE rev.deprecation_events OWNER TO dapple;

CREATE INDEX deprecation_events_versioned_object_idx ON rev.deprecation_events (versioned_object_id);
CREATE INDEX deprecation_events_level_idx ON rev.deprecation_events (deprecation_level_id);
CREATE INDEX deprecation_events_created_at_idx ON rev.deprecation_events (created_at DESC);
```

### 3. Path Detail Tables (One Per Entity with Path-Level Data)

```sql
-- Migration: YYYYMMDD_create_path_detail_tables.sql

-- Tasks: name, slug
CREATE TABLE rev.task_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    UNIQUE (name),
    UNIQUE (slug)
);

-- Steps: name, slug
CREATE TABLE rev.step_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    slug TEXT NOT NULL
);

-- Stages: name, slug
CREATE TABLE rev.stage_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    slug TEXT NOT NULL
);

-- Playbooks: name, slug
CREATE TABLE rev.playbook_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    slug TEXT NOT NULL
);

-- Categories: name, slug
CREATE TABLE rev.category_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    UNIQUE (name)
);

-- Hats: name, slug
CREATE TABLE rev.hat_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    UNIQUE (name)
);

-- Prompts: name only
CREATE TABLE rev.prompt_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL
);

-- Fluent Messages: name, description
CREATE TABLE rev.fluent_message_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    description TEXT NOT NULL
);

-- Briefs: name only
CREATE TABLE rev.brief_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    UNIQUE (name)
);

-- Brief Groups: name only
CREATE TABLE rev.brief_group_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    UNIQUE (name)
);

-- Brief Packs: name only
CREATE TABLE rev.brief_pack_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    UNIQUE (name)
);

-- Value Slots: name, value_kind_id
CREATE TABLE rev.value_slot_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    value_kind_id SMALLINT NOT NULL REFERENCES rev.value_kinds (id),
    UNIQUE (name)
);

-- Assets: asset_kind_id only
CREATE TABLE rev.asset_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    asset_kind_id SMALLINT NOT NULL REFERENCES rev.asset_kinds (id)
);

-- Set ownership
ALTER TABLE rev.task_path_details OWNER TO dapple;
ALTER TABLE rev.step_path_details OWNER TO dapple;
ALTER TABLE rev.stage_path_details OWNER TO dapple;
ALTER TABLE rev.playbook_path_details OWNER TO dapple;
ALTER TABLE rev.category_path_details OWNER TO dapple;
ALTER TABLE rev.hat_path_details OWNER TO dapple;
ALTER TABLE rev.prompt_path_details OWNER TO dapple;
ALTER TABLE rev.fluent_message_path_details OWNER TO dapple;
ALTER TABLE rev.brief_path_details OWNER TO dapple;
ALTER TABLE rev.brief_group_path_details OWNER TO dapple;
ALTER TABLE rev.brief_pack_path_details OWNER TO dapple;
ALTER TABLE rev.value_slot_path_details OWNER TO dapple;
ALTER TABLE rev.asset_path_details OWNER TO dapple;
```

### 4. Modify Existing Entity Tables

For each of the 13 versioned entities, we need to:
1. Add `versioned_object_id` column
2. Remove `{entity}_path_id`, `version_major`, `version_minor`, `version_patch` columns
3. Drop the old `{entity}_paths` table

**Example for briefs:**

```sql
-- Migration: YYYYMMDD_migrate_briefs_to_versioned_objects.sql

-- Step 1: Add new column
ALTER TABLE rev.briefs
    ADD COLUMN versioned_object_id BIGINT REFERENCES rev.versioned_objects (id);

-- Step 2: Migrate existing data
WITH migrated_paths AS (
    INSERT INTO rev.object_paths (object_id, entity_type_id, created_at)
    SELECT bp.object_id,
           (SELECT id FROM rev.entity_types WHERE name = 'brief'),
           bp.created_at
    FROM rev.brief_paths bp
    RETURNING id, object_id
),
migrated_path_details AS (
    INSERT INTO rev.brief_path_details (path_id, name)
    SELECT mp.id, bp.name
    FROM migrated_paths mp
    JOIN rev.brief_paths bp ON bp.object_id = mp.object_id
    RETURNING path_id
),
migrated_versions AS (
    INSERT INTO rev.versioned_objects (object_id, path_id, entity_type_id, version_major, version_minor, version_patch, created_at)
    SELECT b.object_id,
           mp.id,
           (SELECT id FROM rev.entity_types WHERE name = 'brief'),
           b.version_major,
           b.version_minor,
           b.version_patch,
           b.created_at
    FROM rev.briefs b
    JOIN rev.brief_paths bp ON bp.id = b.brief_path_id
    JOIN migrated_paths mp ON mp.object_id = bp.object_id
    RETURNING id, object_id
)
UPDATE rev.briefs b
SET versioned_object_id = mv.id
FROM migrated_versions mv
WHERE b.object_id = mv.object_id;

-- Step 3: Make column NOT NULL and add constraint
ALTER TABLE rev.briefs
    ALTER COLUMN versioned_object_id SET NOT NULL,
    ADD CONSTRAINT briefs_versioned_object_id_unique UNIQUE (versioned_object_id);

-- Step 4: Drop old columns and table (after verifying migration)
ALTER TABLE rev.briefs
    DROP COLUMN brief_path_id,
    DROP COLUMN version_major,
    DROP COLUMN version_minor,
    DROP COLUMN version_patch;

DROP TABLE rev.brief_paths;
```

**Repeat similar migration for all 13 entities:**
- tasks, steps, stages, playbooks, categories, hats
- prompts, fluent_messages
- briefs, brief_groups, brief_packs
- value_slots, assets

---

## Rust Model Changes

### New Core Types

```rust
// crates/dapple-data/src/model/versioned_objects.rs

/// Lookup: entity types
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityType {
    pub id: i16,
    pub name: String,
}

/// Lookup: deprecation levels
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeprecationLevel {
    pub id: i16,
    pub name: String,
    pub severity: i16,
}

/// Central path registry
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ObjectPath {
    pub id: i64,
    pub object_id: Uuid,
    pub entity_type_id: i16,
    pub created_at: OffsetDateTime,
}

/// Central version registry
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VersionedObject {
    pub id: i64,
    pub object_id: Uuid,
    pub path_id: i64,
    pub entity_type_id: i16,
    pub version_major: i16,
    pub version_minor: i16,
    pub version_patch: i16,
    pub created_at: OffsetDateTime,
}

/// Deprecation event (append-only log)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeprecationEvent {
    pub id: i64,
    pub versioned_object_id: i64,
    pub deprecation_level_id: i16,
    pub reason: Option<String>,
    pub created_at: OffsetDateTime,
    pub created_by: Option<i64>,
}

/// Insert args for creating a new versioned object (creates path + version atomically)
#[derive(Clone, Debug)]
pub struct InsertVersionedObjectArgs {
    pub entity_type_name: String,
    // Path-level details vary by entity - passed separately
}

/// Insert args for deprecation
#[derive(Clone, Debug)]
pub struct InsertDeprecationEventArgs {
    pub versioned_object_ids: Vec<i64>,
    pub deprecation_level_names: Vec<String>,
    pub reasons: Vec<Option<String>>,
    pub created_by: Option<i64>,
}
```

### Modified Entity Models

```rust
// crates/dapple-data/src/model/briefs.rs (example)

/// Internal model - now references versioned_object_id
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct BriefModel {
    pub id: i64,
    pub versioned_object_id: i64,
    // From versioned_objects join:
    pub object_id: Uuid,
    pub path_id: i64,
    pub path_object_id: Uuid,
    pub version_major: i16,
    pub version_minor: i16,
    pub version_patch: i16,
    // From brief_path_details join:
    pub name: String,
    // Brief-specific:
    pub title: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

/// DTO unchanged - version info still exposed
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize)]
pub struct Brief {
    pub object_id: Uuid,
    pub path_object_id: Uuid,
    pub name: String,  // From path details
    pub title: String,
    pub description: Option<String>,
    pub version: Version,
    pub created_at: DateTime,
}
```

### New Traits

```rust
// crates/dapple-data/src/traits.rs

/// Marker trait for entities using unified versioning
pub trait UnifiedVersionedObject: RelationalObject {
    /// The entity type name for lookup
    fn entity_type_name() -> &'static str;

    /// Get versioned_object_id
    fn versioned_object_id(&self) -> i64;
}

/// Trait for computing effective deprecation level
pub trait DeprecationAware {
    /// Get direct deprecation level (from deprecation_events)
    fn direct_deprecation_level(&self) -> Option<DeprecationLevel>;

    /// Get effective deprecation level (max of self and dependencies)
    fn effective_deprecation_level(&self) -> DeprecationLevel;
}
```

---

## Query Changes

### Insert Pattern (New)

```sql
-- insert_brief.sql (creates path + version + entity atomically)
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],           -- names (path-level)
        $2::TEXT[],           -- titles (entity-level)
        $3::TEXT[]            -- descriptions (entity-level)
    ) WITH ORDINALITY AS o(name, title, description, ord)
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE name = 'brief'
),
new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT et.id FROM args, entity_type et
    RETURNING id, object_id
),
new_path_details AS (
    INSERT INTO rev.brief_path_details (path_id, name)
    SELECT np.id, args.name
    FROM new_paths np
    JOIN args ON true  -- matched by ordinality
    RETURNING path_id
),
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, entity_type_id, version_major, version_minor, version_patch)
    SELECT np.id, et.id, 0, 0, 1
    FROM new_paths np, entity_type et
    RETURNING id, object_id, path_id, version_major, version_minor, version_patch, created_at
),
new_briefs AS (
    INSERT INTO rev.briefs (versioned_object_id, title, description)
    SELECT nv.id, args.title, args.description
    FROM new_versions nv
    JOIN args ON true  -- matched by ordinality
    RETURNING id, versioned_object_id, title, description, created_at
)
SELECT
    nb.id,
    nv.object_id,
    np.object_id as path_object_id,
    npd.name,
    nb.title,
    nb.description,
    nv.version_major,
    nv.version_minor,
    nv.version_patch,
    nb.created_at,
    args.ord
FROM new_briefs nb
JOIN new_versions nv ON nv.id = nb.versioned_object_id
JOIN new_paths np ON np.id = nv.path_id
JOIN new_path_details npd ON npd.path_id = np.id
JOIN args ON true  -- would need proper ordinality tracking
ORDER BY args.ord;
```

### Select Pattern (New)

```sql
-- select_briefs_by_object_ids.sql
WITH args AS (
    SELECT * FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(object_id, ord)
)
SELECT
    b.id,
    vo.object_id,
    op.object_id as path_object_id,
    bpd.name,
    b.title,
    b.description,
    vo.version_major,
    vo.version_minor,
    vo.version_patch,
    b.created_at,
    -- Latest deprecation level (or null if active)
    (
        SELECT dl.name
        FROM rev.deprecation_events de
        JOIN rev.deprecation_levels dl ON dl.id = de.deprecation_level_id
        WHERE de.versioned_object_id = vo.id
        ORDER BY de.created_at DESC
        LIMIT 1
    ) as deprecation_level,
    args.ord
FROM args
JOIN rev.versioned_objects vo ON vo.object_id = args.object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.brief_path_details bpd ON bpd.path_id = op.id
JOIN rev.briefs b ON b.versioned_object_id = vo.id
ORDER BY args.ord;
```

### Deprecation Query

```sql
-- insert_deprecation_events.sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::BIGINT[],    -- versioned_object_ids
        $2::TEXT[],      -- deprecation_level_names
        $3::TEXT[],      -- reasons
        $4::BIGINT       -- created_by (scalar, applied to all)
    ) WITH ORDINALITY AS o(versioned_object_id, level_name, reason, created_by, ord)
),
levels AS (
    SELECT args.*, dl.id as deprecation_level_id
    FROM args
    JOIN rev.deprecation_levels dl ON dl.name = args.level_name
)
INSERT INTO rev.deprecation_events (versioned_object_id, deprecation_level_id, reason, created_by)
SELECT versioned_object_id, deprecation_level_id, reason, created_by
FROM levels
RETURNING id, versioned_object_id, deprecation_level_id, reason, created_at, created_by;
```

---

## Implementation Phases

### Phase 1: Foundation (No Breaking Changes)

1. Create lookup tables (`entity_types`, `deprecation_levels`)
2. Create core tables (`object_paths`, `versioned_objects`, `deprecation_events`)
3. Create path detail tables (all 13)
4. Add new Rust models and traits

**Verification:** Tables exist, can insert test data, no existing functionality affected.

### Phase 2: Dual-Write Mode

1. Modify INSERT queries to write to BOTH old and new tables
2. Modify SELECT queries to read from new tables with fallback to old
3. Run data migration to populate new tables from old data

**Verification:** All existing tests pass, new tables populated correctly.

### Phase 3: Cutover

1. Switch SELECT queries to read exclusively from new tables
2. Switch INSERT queries to write exclusively to new tables
3. Remove old path columns from entity tables
4. Drop old path tables

**Verification:** All tests pass, no references to old tables.

### Phase 4: Deprecation API

1. Add deprecation endpoints (`POST /api/deprecations`)
2. Add deprecation level to entity DTOs
3. Implement lazy poison propagation in select queries
4. Add `retired` reference blocking in insert queries

**Verification:** Can deprecate objects, poison propagates, retired blocking works.

---

## Files to Create

| File | Description |
|------|-------------|
| `migrations/YYYYMMDD_create_entity_types.sql` | Entity types lookup |
| `migrations/YYYYMMDD_create_deprecation_levels.sql` | Deprecation levels lookup |
| `migrations/YYYYMMDD_create_object_paths.sql` | Central path registry |
| `migrations/YYYYMMDD_create_versioned_objects.sql` | Central version registry |
| `migrations/YYYYMMDD_create_deprecation_events.sql` | Deprecation log |
| `migrations/YYYYMMDD_create_path_detail_tables.sql` | All 13 path detail tables |
| `migrations/YYYYMMDD_migrate_{entity}.sql` | One per entity (13 total) |
| `src/model/versioned_objects.rs` | Core versioning types |
| `src/model/deprecation.rs` | Deprecation types |
| `src/query/versioned_objects.rs` | Versioning queries |
| `src/query/deprecation.rs` | Deprecation queries |

## Files to Modify

| File | Changes |
|------|---------|
| `src/model/{entity}.rs` (13 files) | Replace path_id + version with versioned_object_id |
| `src/query/{entity}.rs` (13 files) | Update INSERT/SELECT to use new tables |
| `assets/sql/{entity}/*.sql` (39+ files) | Update all queries |
| `src/traits.rs` | Add UnifiedVersionedObject, DeprecationAware |
| Handler/schema files | Add deprecation_level to DTOs |

---

## Migration Strategy

### Data Migration Order

1. **Lookup tables first** - entity_types, deprecation_levels
2. **Core tables** - object_paths, versioned_objects, deprecation_events
3. **Path details** - all 13 path detail tables (can be parallel)
4. **Entity migrations** - ordered by dependency:
   - Independent: tasks, stages, categories, prompts, fluent_messages, value_slots, assets
   - Depends on above: steps (→tasks), hats (→categories), briefs
   - Depends on briefs: brief_groups
   - Depends on brief_groups: brief_packs
   - Depends on stages/steps: playbooks

### Rollback Plan

Each migration phase should be independently reversible:
- Phase 1: DROP new tables
- Phase 2: Remove dual-write code, new tables become stale
- Phase 3: Restore old columns from backup/new tables
- Phase 4: Remove deprecation endpoints

---

## Open Questions

1. **Unique constraints on path details:** Should name/slug uniqueness be enforced per-entity or globally? Currently per-entity.

2. **Deprecation created_by:** Should this FK to users table? Or just store the value without FK for flexibility?

3. **Path object_id in DTO:** Should we continue exposing `path_object_id` in DTOs, or switch to `path_id` (internal)?

4. **Version display:** Should version in DTO remain `Version { major, minor, patch }` or change to string `"0.0.1"`?

---

## Notes

- This is a significant refactoring but done incrementally to minimize risk
- Dual-write mode ensures no data loss during transition
- All existing API contracts remain unchanged (DTOs preserve same shape)
- Deprecation is a new capability, not a replacement of existing functionality
- The `entity_type` denormalization in `versioned_objects` is intentional for query performance
