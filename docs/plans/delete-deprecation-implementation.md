# DELETE / Deprecation Implementation Plan

**Status:** Ready for Implementation
**Created:** 2026-01-18
**Updated:** 2026-01-18
**Author:** Claude
**Prerequisites:** Versioned Objects Refactoring Plan (completed)
**Blocks:** None

---

## Summary

This plan implements a graduated deprecation system triggered by the HTTP DELETE verb. Instead of hard deletes, DELETE operations create deprecation events with configurable severity levels. Deprecation poison propagates upstream through the dependency graph, computed lazily at query time.

---

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Delete type | Soft delete only | Append-only architecture, audit trail |
| Soft delete mechanism | Deprecation events table | Supports multiple levels, audit history |
| Deprecation scope | Per-version only | Paths are immutable lineage markers |
| Poison direction | Upstream (deps → dependents) | If Brief is retired, BriefGroup using it is effectively retired |
| Poison timing | Lazy (query time) | No cascade updates needed, always consistent |
| Un-deprecation | Not allowed | Create new version instead |
| API trigger | DELETE verb with level param | Clean REST semantics |
| Hard delete | Never via API | Admin-only via direct DB access if needed |

---

## Deprecation Levels

| Level | Severity | New Refs Allowed | Existing Refs Work | System Behavior |
|-------|----------|------------------|-------------------|-----------------|
| `active` | 0 | Yes | Yes | Normal operation |
| `deprecated` | 1 | Yes | Yes | Courtesy warnings in UI/logs |
| `retired` | 2 | **No** | Yes | Strict warnings, blocks new references |
| `quarantined` | 3 | **No** | **No** | Hard failure, unusable |

### Poison Propagation Example

```
Brief A (retired)
    ↓ referenced by
BriefGroup G (active, but effective = retired)
    ↓ referenced by
BriefPack P (active, but effective = retired)
    ↓ assigned to
Journey J → debrief blocked because BriefPack is effectively retired
```

---

## API Design

### DELETE Endpoint

All versioned entities support DELETE with a deprecation level parameter.

**Request:**
```http
DELETE /api/briefs/{object_id}
Content-Type: application/json

{
  "level": "retired",
  "reason": "Superseded by brief-v2"
}
```

**Request (batch):**
```http
DELETE /api/briefs
Content-Type: application/json

{
  "objs": [
    { "object_id": "uuid-1", "level": "deprecated", "reason": "Old format" },
    { "object_id": "uuid-2", "level": "retired", "reason": "Replaced" }
  ]
}
```

**Response:**
```json
{
  "data": [
    {
      "object_id": "uuid-1",
      "deprecation_level": "deprecated",
      "effective_level": "deprecated",
      "created_at": "2026-01-18T12:00:00Z"
    }
  ]
}
```

### Query Parameter Alternative

For simpler clients, support query parameter:

```http
DELETE /api/briefs/{object_id}?level=retired&reason=Superseded
```

### Deprecation Level in GET Responses

All versioned entity DTOs include deprecation information:

```json
{
  "object_id": "uuid-1",
  "path_object_id": "path-uuid",
  "name": "my-brief",
  "title": "My Brief",
  "version": { "major": 0, "minor": 0, "patch": 1 },
  "deprecation": {
    "level": "deprecated",
    "effective_level": "retired",
    "reason": "Old format",
    "deprecated_at": "2026-01-18T12:00:00Z"
  },
  "created_at": "2026-01-01T00:00:00Z"
}
```

- `level`: Direct deprecation level of this object (null if active)
- `effective_level`: Computed max of self and all dependencies
- `reason`: Human-readable explanation
- `deprecated_at`: When deprecation event was created

---

## Schema Changes

### Request Schemas

```rust
// crates/dapple-prompt-api/src/schema/deprecation.rs

/// DELETE request body for single object
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct DeleteSchema {
    /// Deprecation level: "deprecated", "retired", or "quarantined"
    pub level: String,
    /// Optional human-readable reason
    pub reason: Option<String>,
}

/// DELETE request body for batch operation
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct DeleteSetSchema {
    pub objs: Vec<DeleteObjectSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct DeleteObjectSchema {
    pub object_id: Uuid,
    pub level: String,
    pub reason: Option<String>,
}

/// DELETE response
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct DeleteResponse {
    pub object_id: Uuid,
    pub deprecation_level: String,
    pub effective_level: String,
    pub deprecated_at: DateTime,
}
```

### DTO Additions

```rust
// Added to all versioned entity DTOs

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct DeprecationInfo {
    /// Direct deprecation level (null if active)
    pub level: Option<String>,
    /// Computed effective level including dependencies
    pub effective_level: String,
    /// Reason for deprecation
    pub reason: Option<String>,
    /// When deprecated (null if active)
    pub deprecated_at: Option<DateTime>,
}

// Example: Brief DTO
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct Brief {
    pub object_id: Uuid,
    pub path_object_id: Uuid,
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    pub version: Version,
    pub deprecation: DeprecationInfo,  // NEW
    pub created_at: DateTime,
}
```

---

## SQL Implementation

### Insert Deprecation Event

```sql
-- assets/sql/deprecation/insert_deprecation_events.sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],      -- versioned object_ids (external)
        $2::TEXT[],      -- deprecation level names
        $3::TEXT[],      -- reasons
        $4::BIGINT       -- created_by (user id, nullable)
    ) WITH ORDINALITY AS o(object_id, level_name, reason, created_by, ord)
),
resolved AS (
    SELECT
        vo.id as versioned_object_id,
        dl.id as deprecation_level_id,
        args.reason,
        args.created_by,
        args.ord
    FROM args
    JOIN rev.versioned_objects vo ON vo.object_id = args.object_id
    JOIN rev.deprecation_levels dl ON dl.name = args.level_name
),
inserted AS (
    INSERT INTO rev.deprecation_events (
        versioned_object_id,
        deprecation_level_id,
        reason,
        created_by
    )
    SELECT
        versioned_object_id,
        deprecation_level_id,
        reason,
        created_by
    FROM resolved
    RETURNING
        id,
        versioned_object_id,
        deprecation_level_id,
        reason,
        created_at,
        created_by
)
SELECT
    i.id,
    vo.object_id,
    dl.name as deprecation_level,
    i.reason,
    i.created_at,
    i.created_by,
    r.ord
FROM inserted i
JOIN rev.versioned_objects vo ON vo.id = i.versioned_object_id
JOIN rev.deprecation_levels dl ON dl.id = i.deprecation_level_id
JOIN resolved r ON r.versioned_object_id = i.versioned_object_id
ORDER BY r.ord;
```

### Get Latest Deprecation for Object

```sql
-- assets/sql/deprecation/select_deprecation_by_versioned_object.sql
SELECT
    de.id,
    vo.object_id,
    dl.name as deprecation_level,
    dl.severity,
    de.reason,
    de.created_at,
    de.created_by
FROM rev.deprecation_events de
JOIN rev.versioned_objects vo ON vo.id = de.versioned_object_id
JOIN rev.deprecation_levels dl ON dl.id = de.deprecation_level_id
WHERE vo.object_id = $1
ORDER BY de.created_at DESC
LIMIT 1;
```

### Compute Effective Deprecation Level

For entities with dependencies, compute effective level by walking the graph:

```sql
-- assets/sql/briefs/select_briefs_with_deprecation.sql
WITH args AS (
    SELECT * FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(object_id, ord)
),
brief_data AS (
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
        -- Direct deprecation
        (
            SELECT dl.name
            FROM rev.deprecation_events de
            JOIN rev.deprecation_levels dl ON dl.id = de.deprecation_level_id
            WHERE de.versioned_object_id = vo.id
            ORDER BY de.created_at DESC
            LIMIT 1
        ) as direct_deprecation_level,
        (
            SELECT dl.severity
            FROM rev.deprecation_events de
            JOIN rev.deprecation_levels dl ON dl.id = de.deprecation_level_id
            WHERE de.versioned_object_id = vo.id
            ORDER BY de.created_at DESC
            LIMIT 1
        ) as direct_deprecation_severity,
        (
            SELECT de.reason
            FROM rev.deprecation_events de
            WHERE de.versioned_object_id = vo.id
            ORDER BY de.created_at DESC
            LIMIT 1
        ) as deprecation_reason,
        (
            SELECT de.created_at
            FROM rev.deprecation_events de
            WHERE de.versioned_object_id = vo.id
            ORDER BY de.created_at DESC
            LIMIT 1
        ) as deprecated_at,
        args.ord
    FROM args
    JOIN rev.versioned_objects vo ON vo.object_id = args.object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.brief_path_details bpd ON bpd.path_id = op.id
    JOIN rev.briefs b ON b.versioned_object_id = vo.id
)
SELECT
    bd.*,
    COALESCE(bd.direct_deprecation_level, 'active') as deprecation_level,
    -- For briefs (leaf nodes), effective = direct
    COALESCE(bd.direct_deprecation_level, 'active') as effective_deprecation_level
FROM brief_data bd
ORDER BY bd.ord;
```

### Compute Effective Level for BriefGroup (Has Dependencies)

```sql
-- assets/sql/brief_groups/select_brief_groups_with_deprecation.sql
WITH args AS (
    SELECT * FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(object_id, ord)
),
group_data AS (
    SELECT
        bg.id,
        vo.object_id,
        -- ... other fields ...
        -- Direct deprecation
        (
            SELECT dl.severity
            FROM rev.deprecation_events de
            JOIN rev.deprecation_levels dl ON dl.id = de.deprecation_level_id
            WHERE de.versioned_object_id = vo.id
            ORDER BY de.created_at DESC
            LIMIT 1
        ) as direct_severity,
        args.ord
    FROM args
    JOIN rev.versioned_objects vo ON vo.object_id = args.object_id
    JOIN rev.brief_groups bg ON bg.versioned_object_id = vo.id
),
-- Get max severity from referenced briefs
dependency_severity AS (
    SELECT
        gd.id as group_id,
        COALESCE(MAX(
            (
                SELECT dl.severity
                FROM rev.deprecation_events de
                JOIN rev.deprecation_levels dl ON dl.id = de.deprecation_level_id
                WHERE de.versioned_object_id = bvo.id
                ORDER BY de.created_at DESC
                LIMIT 1
            )
        ), 0) as max_brief_severity
    FROM group_data gd
    JOIN rev.brief_group_briefs bgb ON bgb.brief_group_id = gd.id
    JOIN rev.briefs b ON b.id = bgb.brief_id
    JOIN rev.versioned_objects bvo ON bvo.id = b.versioned_object_id
    GROUP BY gd.id
)
SELECT
    gd.*,
    -- Effective = max(direct, dependencies)
    (
        SELECT dl.name
        FROM rev.deprecation_levels dl
        WHERE dl.severity = GREATEST(
            COALESCE(gd.direct_severity, 0),
            COALESCE(ds.max_brief_severity, 0)
        )
    ) as effective_deprecation_level
FROM group_data gd
LEFT JOIN dependency_severity ds ON ds.group_id = gd.id
ORDER BY gd.ord;
```

### Block References to Retired/Quarantined Objects

```sql
-- In insert_brief_groups.sql, add validation CTE
WITH args AS (...),
-- Check that no referenced briefs are retired or quarantined
blocked_refs AS (
    SELECT
        args.ord,
        b.object_id as brief_object_id,
        dl.name as deprecation_level
    FROM args
    CROSS JOIN LATERAL UNNEST(args.brief_object_ids) AS ref(brief_object_id)
    JOIN rev.versioned_objects bvo ON bvo.object_id = ref.brief_object_id
    JOIN rev.briefs b ON b.versioned_object_id = bvo.id
    JOIN rev.deprecation_events de ON de.versioned_object_id = bvo.id
    JOIN rev.deprecation_levels dl ON dl.id = de.deprecation_level_id
    WHERE dl.severity >= 2  -- retired or quarantined
    AND de.id = (
        SELECT de2.id FROM rev.deprecation_events de2
        WHERE de2.versioned_object_id = bvo.id
        ORDER BY de2.created_at DESC LIMIT 1
    )
)
-- Fail if any blocked refs found
SELECT CASE
    WHEN EXISTS (SELECT 1 FROM blocked_refs)
    THEN (SELECT 1/0)  -- Will raise division by zero, caught by app
    ELSE 1
END;
-- Or use a cleaner approach with a custom function/exception
```

---

## Handler Implementation

```rust
// crates/dapple-prompt-api/src/handler/briefs.rs

/// DELETE /api/briefs/{object_id}
pub async fn briefs_delete_handler(
    State(data): State<Arc<AppState>>,
    Path(object_id): Path<Uuid>,
    Query(params): Query<DeleteQueryParams>,
    body: Option<Json<DeleteSchema>>,
) -> impl IntoApiResponse {
    // Extract level from body or query params
    let (level, reason) = match body {
        Some(Json(b)) => (b.level, b.reason),
        None => (
            params.level.unwrap_or_else(|| "retired".to_string()),
            params.reason,
        ),
    };

    // Validate level
    let valid_levels = ["deprecated", "retired", "quarantined"];
    if !valid_levels.contains(&level.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "Invalid deprecation level: {}. Must be one of: {:?}",
            level, valid_levels
        )));
    }

    let args = InsertDeprecationEventArgs {
        object_ids: vec![object_id],
        level_names: vec![level],
        reasons: vec![reason],
        created_by: None, // TODO: extract from auth context
    };

    delete_response_handler(
        "briefs",
        insert_deprecation_events(&data.db, args).await,
    )
}

/// DELETE /api/briefs (batch)
pub async fn briefs_delete_batch_handler(
    State(data): State<Arc<AppState>>,
    Json(body): Json<DeleteSetSchema>,
) -> impl IntoApiResponse {
    let args = InsertDeprecationEventArgs {
        object_ids: body.objs.iter().map(|o| o.object_id).collect(),
        level_names: body.objs.iter().map(|o| o.level.clone()).collect(),
        reasons: body.objs.iter().map(|o| o.reason.clone()).collect(),
        created_by: None,
    };

    delete_response_handler(
        "briefs",
        insert_deprecation_events(&data.db, args).await,
    )
}
```

### Route Registration

```rust
// crates/dapple-prompt-api/src/route/briefs.rs

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route("/", post_with(briefs_post_handler, post_docs))
        .api_route("/", get_with(briefs_index_handler, index_docs))
        .api_route("/", delete_with(briefs_delete_batch_handler, delete_batch_docs))  // NEW
        .api_route(
            "/{object_id}",
            get_with(briefs_get_handler, get_docs)
                .patch_with(briefs_patch_handler, patch_docs)
                .delete_with(briefs_delete_handler, delete_docs),  // NEW
        )
}
```

---

## Query Function Implementation

```rust
// crates/dapple-data/src/query/deprecation.rs

pub async fn insert_deprecation_events(
    pool: &DbPool,
    args: InsertDeprecationEventArgs,
) -> Result<Vec<DeprecationEventResult>, DataError> {
    // Validate all vectors same length
    assert_eq!(args.object_ids.len(), args.level_names.len());
    assert_eq!(args.object_ids.len(), args.reasons.len());

    let rows = sqlx::query_file_as!(
        DeprecationEventResult,
        "assets/sql/deprecation/insert_deprecation_events.sql",
        &args.object_ids,
        &args.level_names,
        &args.reasons,
        args.created_by
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn select_effective_deprecation_level(
    pool: &DbPool,
    object_id: Uuid,
    entity_type: &str,
) -> Result<DeprecationInfo, DataError> {
    // Entity-specific query that walks dependency graph
    // Returns computed effective level
    todo!()
}
```

---

## Implementation Phases

### Phase 1: Core Infrastructure

1. Create `deprecation_levels` lookup table (if not already in versioned-objects plan)
2. Create `deprecation_events` table (if not already)
3. Create deprecation query functions
4. Add `InsertDeprecationEventArgs`, `DeprecationEventResult` types

**Verification:** Can insert and query deprecation events directly.

### Phase 2: DELETE Endpoints (Leaf Entities)

Start with entities that have no dependencies (simpler effective level calculation):

1. Briefs
2. Assets
3. Value Slots
4. Fluent Messages
5. Tasks
6. Stages
7. Categories
8. Prompts

For each:
- Add `delete_handler` and `delete_batch_handler`
- Register DELETE routes
- Update select queries to include deprecation info
- Update DTO to include `DeprecationInfo`

**Verification:** Can DELETE these entities, deprecation shows in GET.

### Phase 3: DELETE Endpoints (Entities with Dependencies)

Entities that reference others (need effective level propagation):

1. Steps (→ Tasks)
2. Hats (→ Categories)
3. Brief Groups (→ Briefs)
4. Brief Packs (→ Brief Groups)
5. Playbooks (→ Stages, Steps, Tasks, Categories)

For each:
- Implement effective level computation in select queries
- Add reference blocking for retired/quarantined in insert queries

**Verification:** Effective levels computed correctly, blocking works.

### Phase 4: Integration & Polish

1. Add deprecation filtering to index/list endpoints (`?exclude_deprecated=true`)
2. Add bulk deprecation endpoint (`DELETE /api/deprecations/bulk`)
3. Add deprecation history endpoint (`GET /api/{entity}/{id}/deprecation-history`)
4. Update OpenAPI documentation
5. Add integration tests

---

## Files to Create

| File | Description |
|------|-------------|
| `assets/sql/deprecation/insert_deprecation_events.sql` | Create deprecation event |
| `assets/sql/deprecation/select_deprecation_history.sql` | Get deprecation history for object |
| `src/model/deprecation.rs` | Deprecation types |
| `src/query/deprecation.rs` | Deprecation query functions |
| `src/schema/deprecation.rs` | API schemas for DELETE |
| `src/handler/deprecation.rs` | Shared delete handler utilities |

## Files to Modify

| File | Changes |
|------|---------|
| `src/model/{entity}.rs` (13 files) | Add DeprecationInfo to DTO |
| `src/query/{entity}.rs` (13 files) | Add deprecation to select queries |
| `assets/sql/{entity}/select_*.sql` (26+ files) | Include deprecation subqueries |
| `assets/sql/{entity}/insert_*.sql` (5 files) | Add retired/quarantined blocking |
| `src/handler/{entity}.rs` (13 files) | Add delete handlers |
| `src/route/{entity}.rs` (13 files) | Register DELETE routes |

---

## Validation Rules

### On DELETE

1. Object must exist
2. Level must be valid (deprecated, retired, quarantined)
3. Cannot "un-deprecate" (no setting level to "active")

### On INSERT/PATCH (Reference Blocking)

1. Cannot reference `retired` or `quarantined` objects
2. Can reference `deprecated` objects (with warning in response)
3. Check effective level, not just direct level

### On GET

1. Always return deprecation info
2. Compute effective level for entities with dependencies
3. Default filter: exclude quarantined unless `?include_quarantined=true`

---

## Error Responses

```json
// Attempt to reference retired object
{
  "error": "REFERENCE_BLOCKED",
  "message": "Cannot reference retired object",
  "details": {
    "blocked_references": [
      {
        "object_id": "uuid-of-retired-brief",
        "deprecation_level": "retired",
        "reason": "Superseded by v2"
      }
    ]
  }
}

// Attempt to un-deprecate
{
  "error": "INVALID_DEPRECATION",
  "message": "Cannot set deprecation level to 'active'. Create a new version instead."
}
```

---

## Open Questions

1. **Default level for DELETE without parameter:** Should it default to `retired` (safe) or require explicit level?

2. **Cascade display:** When showing effective level, should we also show which dependency caused it?

3. **Deprecation notifications:** Should deprecating an object trigger notifications to owners of dependent objects?

4. **Grace period:** Should `retired` have a grace period before blocking new references?

---

## Cleanup Required (from Phase 1 Implementation)

The following items need to be addressed before or during implementation of this plan:

| Item | Location | Action Required |
|------|----------|-----------------|
| Remove reactivation endpoint | `src/handler/deprecations.rs` | Delete `reactivations_post_handler` - un-deprecation is not allowed per design |
| Remove reactivation route | `src/route/deprecations.rs` | Remove any route registration for reactivations |
| Update tests | Integration tests | Remove any tests for reactivation functionality |

**Rationale:** The design explicitly states "no un-deprecation - create new version instead". The reactivation endpoint was created in error during Phase 1 implementation and must be removed.

---

## Notes

- This system is fully append-only - deprecation events are never deleted
- Effective level computation is O(depth) where depth is the dependency chain length
- For deeply nested structures (playbooks), consider caching effective levels
- Hard delete is intentionally not exposed via API - admin-only via direct DB access
- **There is no way to un-deprecate** - if a version needs to be "reactivated", create a new version instead
