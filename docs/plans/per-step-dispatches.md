# Per-Step Dispatches with Journey Context Seeding

> **STALE (2026-02-20).** Predates the `object_id → id` migration — the `*_object_id` naming
> throughout is now the anti-pattern (PKs are `id: Uuid`). Its §7.5 journey-wide read contract
> is superseded by `crates/dapple-prompt-flow/AGENTS.md` → "Binding read availability". §12's
> "No AGENTS.md updates required" is precisely why that read invariant never reached the
> surface agents read first; now corrected.
>
> **Granularity note.** Dispatch granularity (per-step is fine) is **orthogonal** to read
> availability (always journey-wide, order-independent). Neither per-step dispatch nor
> per-stage brief grain implies *value scoping* — reads pool the whole journey regardless of
> stage/step/order.

**Status:** Draft
**Created:** 2026-02-20
**Last Updated:** 2026-02-20
**Target Version:** 0.1.0
**Dependencies:** None (builds on existing dispatch infrastructure)

---

## 1. Executive Summary

This plan flattens the `CreateDispatchSchema` so callers submit steps directly rather than wrapping them in stages. Today, creating a dispatch requires nesting `stages[] > steps[] > tasks[]`, even when targeting a single step. The backend already has the information needed to derive the stage from the `playbook_step_id` (via `playbook_steps.playbook_stage_id`), so forcing callers to supply the stage is redundant ceremony that complicates the client, bloats the request body, and introduces an error surface where the supplied `playbook_stage_id` could conflict with the one implied by the step.

The second change introduces **journey context seeding**: when `execute_dispatch()` runs, it loads completed task outputs from prior dispatches in the same journey and merges them into the initial `accumulated_data`. This allows dispatch N to see outputs from dispatches 1..N-1 without the client needing to shuttle data between calls.

Finally, a new `ByJourney` query variant is added to `GetDispatchSetSchema` so the UI can poll dispatch status for an active journey.

These changes directly benefit the debrief flow where the user progresses through a playbook one step at a time across multiple dispatches within a single journey.

## 2. Context and Background

### Current State

The `CreateDispatchSchema` requires a deeply nested payload:

```json
{
  "journey_id": "<uuid>",
  "intent": {},
  "stages": [{
    "playbook_stage_id": "<uuid>",
    "params": {},
    "steps": [{
      "playbook_step_id": "<uuid>",
      "params": {},
      "tasks": [{ "playbook_task_id": "<uuid>" }]
    }]
  }]
}
```

The handler in `dispatches_post_handler` converts this into `InsertDispatchArgs` with nested `InsertDispatchStagesArgs` and `InsertDispatchStepsArgs`. The insert path is:

1. `insert_dispatch.sql` -- creates the `rev.dispatches` row
2. `insert_dispatch_stages.sql` -- creates `rev.dispatch_stages` rows, resolving `playbook_stage_object_id` to internal id
3. `insert_dispatch_steps.sql` -- creates `rev.dispatch_steps` rows, resolving `playbook_step_object_id` to internal id
4. `insert_dispatch_tasks.sql` -- creates `rev.dispatch_tasks` rows

Each layer is called in sequence within a single transaction.

### Problems

1. **Redundant stage specification** -- The `playbook_step_id` already references a `playbook_stage` via `rev.playbook_steps.playbook_stage_id`. Making the caller also supply `playbook_stage_id` creates an error surface and duplicates knowledge.

2. **No cross-dispatch context** -- `execute_dispatch()` seeds `accumulated_data` only from `dispatch.intent`. Dispatch N cannot see outputs from dispatches 1..N-1 in the same journey. The client must manually extract outputs from prior dispatches and shuttle them via `intent`, which is fragile and creates large payloads.

3. **No ByJourney query** -- The UI polls dispatch status per journey, but the only query variants are `ByUser` and `ByPlaybook`. The UI must fetch all user dispatches and filter client-side.

### Relevant Project Patterns

- **Append-only**: No UPDATE operations. New dispatches append to the journey's dispatch history.
- **CTE-based resolution**: `object_id -> internal id` lookups via CTEs, not separate queries.
- **UNNEST WITH ORDINALITY**: Batch operations with preserved ordering.
- **UUIDv7 ordering**: `ORDER BY object_id` for creation-time ordering.
- **Single-query atomic inserts**: Principal + dependents in one transaction.

## 3. Database Schema Changes (DDL)

**No DDL migration is required.**

The existing table structure supports this change without modification:

- `rev.dispatch_stages` already has `dispatch_id` and `playbook_stage_id` -- we simply derive the stage from the step.
- `rev.dispatch_steps` already has `dispatch_stage_id` and `playbook_step_id`.
- `rev.dispatch_tasks` already has `dispatch_step_id` and `playbook_task_id`.
- `rev.dispatch_task_attempts` and `rev.dispatch_task_attempt_events` already store task outputs as JSONB.

The key insight is that `rev.playbook_steps.playbook_stage_id` already tells us which stage a step belongs to. The backend can derive this without the caller providing it.

## 4. Object Model Design

### Existing Objects (unchanged)

The internal models (`InsertedDispatchModel`, `DispatchStageModel`, `DispatchStepModel`, `DispatchTaskModel`) and DTOs (`Dispatch`, `DispatchStage`, `DispatchStep`, `DispatchTask`) remain unchanged. The dispatch still stores stages/steps/tasks in the database -- only the **creation API** changes.

### New: Context Seeding Model

```rust
/// Represents a completed task output from a prior dispatch in the same journey.
/// Used for journey context seeding.
pub(crate) struct CompletedTaskContext {
    pub dispatch_object_id: Uuid,
    pub task_object_id: Uuid,
    pub task_name: String,
    pub function: String,
    pub function_input: JsonValue,
    pub context_data: JsonValue,
    pub created_at: OffsetDateTime,
}
```

This is a query-only model (no insert counterpart). It is used by `execute_dispatch()` to load prior task outputs.

## 5. API Contract Changes

### POST /api/dispatches -- Flattened Schema

**Before (current):**
```rust
pub struct CreateDispatchSchema {
    pub journey_id: Uuid,
    pub intent: JsonValue,
    pub stages: Vec<CreateDispatchStageSchema>,
}
```

**After (new):**
```rust
pub struct CreateDispatchSchema {
    pub journey_id: Uuid,
    pub intent: JsonValue,
    pub steps: Vec<CreateDispatchStepSchema>,
}

pub struct CreateDispatchStepSchema {
    pub playbook_step_id: Uuid,
    pub params: JsonValue,
    pub tasks: Vec<CreateDispatchTaskSchema>,
}

pub struct CreateDispatchTaskSchema {
    pub playbook_task_id: Uuid,
}
```

The `CreateDispatchStageSchema` is removed entirely. The backend derives the stage from each step's `playbook_step_id` by joining through `rev.playbook_steps` to `rev.playbook_stages`.

**Backward Compatibility:** This is a breaking change to the API contract. The old `stages`-based schema is removed. The `CreateDispatchStageSchema` type is deleted. Callers must update to the new `steps`-based schema. This is acceptable because:
- The dispatch creation endpoint is internal (used by dapple-ui and tests only)
- The response format (`Dispatch` DTO with `stages[]`) is unchanged
- Only the request body changes

### GET /api/dispatches -- New ByJourney variant

```rust
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDispatchSetSchema {
    ByUser {
        user_id: Uuid,
        options: Option<FilterOptions>,
    },
    ByPlaybook {
        playbook_id: Uuid,
        options: Option<FilterOptions>,
    },
    ByJourney {
        journey_id: Uuid,
        options: Option<FilterOptions>,
    },
}
```

## 6. SQL Query Strategy

### 6.1 Modified: Insert Path (Stage Derivation)

The current three-step insert process remains, but the **handler** now groups steps by their derived stage before calling the insert functions.

The handler logic (not SQL) changes:

1. For each `CreateDispatchStepSchema`, look up `playbook_step_id` to discover its `playbook_stage_id` via `rev.playbook_steps.playbook_stage_id`.
2. Group steps by their derived `playbook_stage_id`.
3. Build `InsertDispatchStagesArgs` from those groups, with `params: json!({})` for the auto-derived stage.
4. Call the existing `insert_dispatch_models` function with the derived structure.

**Important:** The existing SQL files for `insert_dispatch_stages.sql`, `insert_dispatch_steps.sql`, and `insert_dispatch_tasks.sql` remain unchanged. The stage derivation happens in the Rust handler layer.

#### New SQL: Resolve playbook_step to playbook_stage (batch)

Create a new query to batch-resolve `playbook_step_object_id[]` to their parent `playbook_stage_object_id[]`:

**File:** `crates/dapple-data/assets/sql/dispatches/resolve_playbook_stages_for_steps.sql`

```sql
-- Resolve playbook_step_object_ids to their parent playbook_stage_object_ids.
-- Returns one row per input step, with the step and stage identifiers.
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(playbook_step_object_id, ord)
)
SELECT
    args.ord,
    ps.id AS playbook_step_id,
    ps.object_id AS playbook_step_object_id,
    pst.id AS playbook_stage_id,
    pst.object_id AS playbook_stage_object_id
FROM args
JOIN rev.playbook_steps ps ON ps.object_id = args.playbook_step_object_id
JOIN rev.playbook_stages pst ON ps.playbook_stage_id = pst.id
ORDER BY args.ord;
```

This single query resolves all steps to their stages in one round-trip, avoiding N+1 lookups.

#### New Rust Model for Resolution Result

```rust
pub(crate) struct ResolvedPlaybookStepStage {
    pub ord: i32,
    pub playbook_step_id: i64,
    pub playbook_step_object_id: Uuid,
    pub playbook_stage_id: i64,
    pub playbook_stage_object_id: Uuid,
}
```

#### New Rust Query Function

```rust
pub async fn resolve_playbook_stages_for_steps(
    pool: &DbPool,
    playbook_step_object_ids: &[Uuid],
) -> Result<Vec<ResolvedPlaybookStepStage>, DataError> {
    let rows = sqlx::query_file_as!(
        ResolvedPlaybookStepStage,
        "assets/sql/dispatches/resolve_playbook_stages_for_steps.sql",
        playbook_step_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}
```

### 6.2 New: Select Dispatches by Journey

**File:** `crates/dapple-data/assets/sql/dispatches/select_dispatches_by_journey.sql`

This query follows the exact same pattern as `select_dispatches_by_user.sql` but filters by `journey_id` instead of user:

```sql
WITH limited AS (
    SELECT d.*
    FROM rev.dispatches d
    WHERE d.journey_id = (SELECT id FROM rev.journeys WHERE object_id = $1)
    ORDER BY d.id DESC
    LIMIT $2 OFFSET $3
)
SELECT
    d.id,
    d.object_id,
    d.journey_id,
    j.object_id AS journey_object_id,
    d.intent,
    d.created_at,
    -- dispatch_stage columns (same as select_dispatches_by_user.sql)
    dst.id AS "dispatch_stage_id?",
    dst.object_id AS "dispatch_stage_object_id?",
    dst_pst.id AS "dispatch_stage_playbook_stage_id?",
    dst_pst.object_id AS "dispatch_stage_playbook_stage_object_id?",
    dst_pst.playbook_id AS "dispatch_stage_playbook_id?",
    dp.object_id AS "dispatch_stage_playbook_object_id?",
    dst_pst.stage_id AS "dispatch_stage_stage_id?",
    ds2.object_id AS "dispatch_stage_stage_object_id?",
    dst.params AS "dispatch_stage_params?",
    dst.created_at AS "dispatch_stage_created_at?",
    -- dispatch_step columns
    ds.id AS "dispatch_step_id?",
    ds.object_id AS "dispatch_step_object_id?",
    ds.playbook_step_id AS "dispatch_step_playbook_step_id?",
    ps.object_id AS "dispatch_step_playbook_step_object_id?",
    pst.id AS "dispatch_step_playbook_stage_id?",
    pst.object_id AS "dispatch_step_playbook_stage_object_id?",
    s.id AS "dispatch_step_step_id?",
    s.object_id AS "dispatch_step_step_object_id?",
    p.id AS "dispatch_step_playbook_id?",
    p.object_id AS "dispatch_step_playbook_object_id?",
    ds.params AS "dispatch_step_params?",
    ds.created_at AS "dispatch_step_created_at?",
    -- dispatch_task columns
    dt.id AS "dispatch_task_id?",
    dt.object_id AS "dispatch_task_object_id?",
    dt.playbook_task_id AS "dispatch_task_playbook_task_id?",
    pt.object_id AS "dispatch_task_playbook_task_object_id?",
    t.id AS "dispatch_task_task_id?",
    t.object_id AS "dispatch_task_task_object_id?",
    t.created_at AS "dispatch_task_created_at?",
    dta.status AS "dispatch_task_status?"
FROM limited d
JOIN rev.journeys j ON d.journey_id = j.id
LEFT JOIN rev.dispatch_stages dst ON d.id = dst.dispatch_id
LEFT JOIN rev.playbook_stages dst_pst ON dst.playbook_stage_id = dst_pst.id
LEFT JOIN rev.playbooks dp ON dst_pst.playbook_id = dp.id
LEFT JOIN rev.stages ds2 ON dst_pst.stage_id = ds2.id
LEFT JOIN rev.dispatch_steps ds ON dst.id = ds.dispatch_stage_id
LEFT JOIN rev.playbook_steps ps ON ds.playbook_step_id = ps.id
LEFT JOIN rev.playbook_stages pst ON ps.playbook_stage_id = pst.id
LEFT JOIN rev.steps s ON ps.step_id = s.id
LEFT JOIN rev.playbooks p ON pst.playbook_id = p.id
LEFT JOIN rev.dispatch_tasks dt ON dt.dispatch_step_id = ds.id
LEFT JOIN rev.playbook_tasks pt ON dt.playbook_task_id = pt.id
LEFT JOIN rev.tasks t ON pt.task_id = t.id
LEFT JOIN LATERAL (
    SELECT COALESCE(ev.phase_code, a.status) as status
    FROM rev.dispatch_task_attempts a
    LEFT JOIN LATERAL (
        SELECT phase_code
        FROM rev.dispatch_task_attempt_events ev2
        WHERE ev2.attempt_id = a.id
        ORDER BY ev2.created_at DESC, ev2.id DESC
        LIMIT 1
    ) ev ON true
    WHERE a.dispatch_step_id = ds.id
      AND a.task_id = t.id
    ORDER BY a.created_at DESC
    LIMIT 1
) dta ON true
ORDER BY d.id DESC, dst.id NULLS FIRST, ds.id NULLS FIRST, dt.id NULLS FIRST;
```

### 6.3 New: Select Completed Task Context by Journey

**File:** `crates/dapple-data/assets/sql/dispatches/select_completed_task_context_by_journey.sql`

This query loads the most recent successful task output for each task in the journey, used by `execute_dispatch()` for context seeding. It traverses: `journey -> dispatches -> stages -> steps -> task_attempts -> events`.

```sql
-- Load completed task outputs from prior dispatches in a journey.
-- For each (task_id) that has a successful attempt, return the most recent "ok" event's
-- context_data. Excludes the current dispatch (identified by $2) so we only seed
-- from *prior* dispatches.
--
-- Returns one row per unique task that has completed successfully.
WITH journey_dispatches AS (
    SELECT d.id AS dispatch_id
    FROM rev.dispatches d
    WHERE d.journey_id = (SELECT id FROM rev.journeys WHERE object_id = $1)
      AND d.object_id != $2  -- exclude current dispatch
    ORDER BY d.id ASC
),
ok_events AS (
    SELECT DISTINCT ON (a.task_id)
        a.task_id,
        a.dispatch_step_id,
        ev.context_data,
        ev.created_at AS event_created_at
    FROM rev.dispatch_task_attempts a
    JOIN rev.dispatch_steps ds ON a.dispatch_step_id = ds.id
    JOIN rev.dispatch_stages dst ON ds.dispatch_stage_id = dst.id
    JOIN journey_dispatches jd ON dst.dispatch_id = jd.dispatch_id
    JOIN rev.dispatch_task_attempt_events ev ON ev.attempt_id = a.id
    WHERE ev.phase_code = 'ok'
      AND ev.context_data IS NOT NULL
    ORDER BY a.task_id, ev.created_at DESC, ev.id DESC
)
SELECT
    d.object_id AS dispatch_object_id,
    t.object_id AS task_object_id,
    t.name AS task_name,
    t.function,
    t.function_input,
    oe.context_data,
    oe.event_created_at AS created_at
FROM ok_events oe
JOIN rev.tasks t ON oe.task_id = t.id
JOIN rev.dispatch_steps ds ON oe.dispatch_step_id = ds.id
JOIN rev.dispatch_stages dst ON ds.dispatch_stage_id = dst.id
JOIN rev.dispatches d ON dst.dispatch_id = d.id
ORDER BY oe.event_created_at ASC;
```

**Design decisions:**
- `DISTINCT ON (a.task_id)` with `ORDER BY a.task_id, ev.created_at DESC` picks the most recent successful output per task.
- Excludes the current dispatch (`$2`) so we only seed from prior dispatches.
- Orders by `event_created_at ASC` so the caller processes outputs in chronological order.
- Returns the `function` and `function_input` so `execute_dispatch()` can derive the output_path for each prior task and flatten correctly.

## 7. Implementation Guidance

### 7.1 Handler Changes -- `dispatches_post_handler`

**File:** `crates/dapple-prompt-api/src/handler/dispatches.rs`

The handler must:

1. Accept the new `CreateDispatchSchema` with `steps` instead of `stages`.
2. Collect all `playbook_step_object_id`s from the request.
3. Call `resolve_playbook_stages_for_steps()` to batch-resolve step-to-stage mappings.
4. Group the steps by their derived `playbook_stage_object_id`.
5. Build `InsertDispatchArgs` with derived `InsertDispatchStagesArgs`.
6. Call `insert_dispatches()` as before.

**Pseudocode for the grouping logic:**

```rust
// 1. Resolve all playbook_step -> playbook_stage mappings in one query
let step_oids: Vec<Uuid> = body.objs.iter()
    .flat_map(|d| d.steps.iter().map(|s| s.playbook_step_id))
    .collect();

let resolutions = resolve_playbook_stages_for_steps(&data.db, &step_oids).await?;

// 2. Build a lookup: playbook_step_object_id -> playbook_stage_object_id
let step_to_stage: HashMap<Uuid, Uuid> = resolutions.iter()
    .map(|r| (r.playbook_step_object_id, r.playbook_stage_object_id))
    .collect();

// 3. For each dispatch schema, group steps by stage
for d in body.objs.iter() {
    let mut stage_groups: IndexMap<Uuid, Vec<&CreateDispatchStepSchema>> = IndexMap::new();
    for step in &d.steps {
        let stage_oid = step_to_stage.get(&step.playbook_step_id)
            .ok_or_else(|| /* error: unknown playbook_step_id */)?;
        stage_groups.entry(*stage_oid).or_default().push(step);
    }

    // 4. Convert grouped steps into InsertDispatchStagesArgs
    let stages: Vec<InsertDispatchStagesArgs> = stage_groups.into_iter()
        .map(|(stage_oid, steps)| {
            InsertDispatchStagesArgs {
                playbook_stage_object_ids: vec![stage_oid],
                params: vec![json!({})],  // auto-derived stages get empty params
                steps: steps.iter().map(|s| {
                    InsertDispatchStepsArgs {
                        playbook_step_object_ids: vec![s.playbook_step_id],
                        params: vec![s.params.clone()],
                        tasks: vec![InsertDispatchTaskArgs {
                            playbook_task_object_ids: s.tasks.iter()
                                .map(|t| t.playbook_task_id).collect(),
                        }],
                    }
                }).collect(),
            }
        })
        .collect();

    objs.push(InsertDispatchArgs {
        intents: vec![d.intent.clone()],
        stages,
    });
}
```

**Key points:**
- `InsertDispatchArgs`, `InsertDispatchStagesArgs`, `InsertDispatchStepsArgs`, and `InsertDispatchTaskArgs` remain unchanged.
- The existing `insert_dispatch_models()` function and all SQL files remain unchanged.
- Only the handler-level conversion from schema to args changes.
- The `flatten_stages_for_insert()` and `flatten_steps_for_insert()` helper functions in the handler are replaced with the new grouping logic.
- The `flatten_tasks_for_insert()` helper can remain as-is.

### 7.2 Handler Changes -- `dispatches_index_handler`

**File:** `crates/dapple-prompt-api/src/handler/dispatches.rs`

Add the `ByJourney` match arm:

```rust
GetDispatchSetSchema::ByJourney { journey_id, options } => {
    select_dispatches_by_journey(&data.db, journey_id, options.unwrap_or_default()).await
}
```

### 7.3 Query Layer -- `select_dispatches_by_journey`

**File:** `crates/dapple-data/src/query/dispatches.rs`

Add a new public function and internal helper following the existing pattern:

```rust
pub async fn select_dispatches_by_journey(
    pool: &DbPool,
    journey_object_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Dispatch>, DataError> {
    let models = select_dispatch_models_by_journey(pool, journey_object_id, filter_ops).await?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub(crate) async fn select_dispatch_models_by_journey(
    pool: &DbPool,
    journey_object_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<DispatchModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatDispatchModel,
        "assets/sql/dispatches/select_dispatches_by_journey.sql",
        journey_object_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatDispatchModel::fold_models(models)
}
```

### 7.4 Query Layer -- `select_completed_task_context_by_journey`

**File:** `crates/dapple-data/src/query/dispatches.rs`

```rust
pub async fn select_completed_task_context_by_journey(
    pool: &DbPool,
    journey_object_id: Uuid,
    current_dispatch_object_id: Uuid,
) -> Result<Vec<CompletedTaskContext>, DataError> {
    let rows = sqlx::query_file_as!(
        CompletedTaskContext,
        "assets/sql/dispatches/select_completed_task_context_by_journey.sql",
        journey_object_id,
        current_dispatch_object_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}
```

### 7.5 dispatch_runner Changes -- Journey Context Seeding

**File:** `crates/dapple-prompt-flow/src/dispatch_runner.rs`

The `execute_dispatch()` function currently seeds `accumulated_data` only from `dispatch.intent`:

```rust
let mut accumulated_data = serde_json::Map::new();
if let Some(intent_obj) = dispatch.intent.as_object() {
    for (k, v) in intent_obj {
        accumulated_data.insert(k.clone(), v.clone());
    }
}
```

**Change:** After seeding from intent, load and merge prior task outputs:

```rust
let mut accumulated_data = serde_json::Map::new();

// 1. Seed from dispatch intent
if let Some(intent_obj) = dispatch.intent.as_object() {
    for (k, v) in intent_obj {
        accumulated_data.insert(k.clone(), v.clone());
    }
}

// 2. Seed from completed tasks in prior dispatches of the same journey
let prior_contexts = dapple_data::query::dispatches::select_completed_task_context_by_journey(
    db,
    dispatch.journey_object_id,
    dispatch.object_id,
).await.unwrap_or_else(|e| {
    warn!(
        dispatch_object_id = %dispatch.object_id,
        error = %e,
        "Failed to load journey context, proceeding without prior context"
    );
    vec![]
});

for ctx in &prior_contexts {
    // Derive the output_key from the task's function_input, matching the
    // same logic used when accumulating within a single dispatch.
    let output_key = ctx.function_input
        .get("output_path")
        .and_then(|v| v.as_str())
        .unwrap_or("data.nu_output")
        .strip_prefix("data.")
        .unwrap_or("nu_output");

    if let Some(output_val) = ctx.context_data.get(output_key) {
        // Flatten payload contents to top level (same as intra-dispatch accumulation)
        if let Some(obj) = output_val.as_object() {
            for (k, v) in obj {
                accumulated_data.insert(k.clone(), v.clone());
            }
        }
        // Also preserve under the output key for explicit access
        accumulated_data.insert(output_key.to_string(), output_val.clone());
    }
}

info!(
    dispatch_object_id = %dispatch.object_id,
    prior_task_count = prior_contexts.len(),
    accumulated_keys = ?accumulated_data.keys().collect::<Vec<_>>(),
    "Journey context seeded"
);
```

**Design decisions for context seeding:**

- **Graceful degradation**: If the query fails, execution proceeds without prior context. This ensures a database hiccup does not block dispatch execution.
- **Same flattening logic**: The seeding uses the same `output_path` -> `strip_prefix("data.")` -> flatten pattern as intra-dispatch accumulation. This means a script in dispatch N sees prior outputs identically to how it would see outputs from earlier tasks in the same dispatch.
- **Most recent wins**: The query returns one row per task (most recent successful output). If the same task ran in dispatches 1 and 2, we use dispatch 2's output.
- **Chronological ordering**: Prior outputs are processed in `event_created_at ASC` order so later outputs overwrite earlier ones when keys collide.
- **Intent takes priority**: Intent is seeded first, then prior context. Since prior context is processed after intent, prior task outputs will overwrite intent keys with the same name. This is intentional -- if the user provided `topic: "dogs"` in intent for dispatch 1, and task 1 produced `topic: "refined_topic_about_dogs"`, dispatch 2 should see the refined version. If the caller wants to override, they can set the key in the new dispatch's intent (which will be merged first, then overwritten by prior context, then overwritten again by the current dispatch's task outputs).

**Alternative considered and rejected:** Intent overriding prior context. This would require the client to know which keys prior tasks produced, adding coupling. The current approach ("most recent task output wins") is simpler and matches the append-only philosophy.

### 7.6 Schema Changes

**File:** `crates/dapple-prompt-api/src/schema/dispatches.rs`

Full replacement:

```rust
use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::{FilterOptions, JsonObject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use uuid::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDispatchSetSchema {
    pub objs: Vec<CreateDispatchSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDispatchSchema {
    pub journey_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub intent: JsonValue,
    pub steps: Vec<CreateDispatchStepSchema>,
}

#[derive(JsonSchema, Clone, Serialize, Deserialize, Debug)]
pub struct CreateDispatchStepSchema {
    pub playbook_step_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub params: JsonValue,
    pub tasks: Vec<CreateDispatchTaskSchema>,
}

#[derive(JsonSchema, Clone, Serialize, Deserialize, Debug)]
pub struct CreateDispatchTaskSchema {
    pub playbook_task_id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDispatchSetSchema {
    ByUser {
        user_id: Uuid,
        options: Option<FilterOptions>,
    },
    ByPlaybook {
        playbook_id: Uuid,
        options: Option<FilterOptions>,
    },
    ByJourney {
        journey_id: Uuid,
        options: Option<FilterOptions>,
    },
}

impl QueryParamDocs for GetDispatchSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "user_id",
                description: "UUID specific to the user.",
                required: true,
                schema: || param_schema_from::<Uuid>(),
            },
            ParamDoc {
                name: "journey_id",
                description: "UUID of the journey to filter dispatches by.",
                required: false,
                schema: || param_schema_from::<Uuid>(),
            },
            ParamDoc {
                name: "options",
                description: "Query filter options (e.g. `limit`, `page`). Optional.",
                required: false,
                schema: || param_schema_from::<FilterOptions>(),
            },
        ]
    }
}
```

**Removed:** `CreateDispatchStageSchema` (deleted entirely).

## 8. File-by-File Change List

### New Files

| File | Purpose |
|------|---------|
| `crates/dapple-data/assets/sql/dispatches/resolve_playbook_stages_for_steps.sql` | Batch resolve step -> stage mapping |
| `crates/dapple-data/assets/sql/dispatches/select_dispatches_by_journey.sql` | Select dispatches filtered by journey |
| `crates/dapple-data/assets/sql/dispatches/select_completed_task_context_by_journey.sql` | Load prior task outputs for context seeding |

### Modified Files

| File | Change |
|------|--------|
| `crates/dapple-prompt-api/src/schema/dispatches.rs` | Replace `stages` with `steps` in `CreateDispatchSchema`; remove `CreateDispatchStageSchema`; add `ByJourney` to `GetDispatchSetSchema` |
| `crates/dapple-prompt-api/src/handler/dispatches.rs` | Rewrite `dispatches_post_handler` to group steps by derived stage; add `ByJourney` match arm to `dispatches_index_handler`; remove `flatten_stages_for_insert()` |
| `crates/dapple-data/src/model/dispatches.rs` | Add `CompletedTaskContext` model struct |
| `crates/dapple-data/src/model/dispatches/dispatch_steps.rs` | Add `ResolvedPlaybookStepStage` model struct (could also go in a new file under dispatches/) |
| `crates/dapple-data/src/query/dispatches.rs` | Add `select_dispatches_by_journey()`, `select_completed_task_context_by_journey()`, `resolve_playbook_stages_for_steps()` |
| `crates/dapple-prompt-flow/src/dispatch_runner.rs` | Add journey context seeding to `execute_dispatch()` |
| `crates/dapple-prompt-api/tests/consumer/dispatches.rs` | Update `fake_create_schema` to use `steps` instead of `stages` |
| `crates/dapple-prompt-api/tests/user_lifecycle.rs` | Update `dispatch_execution_trial` to use new schema; add `dispatch_context_seeding_trial` |
| `crates/dapple-prompt-flow/tests/dispatch_nushell_pipeline_integration.rs` | Update `dispatch_nushell_pipeline_integration` and `execute_dispatch_with_context_accumulation` to use new schema format |

### Unchanged Files

| File | Reason |
|------|--------|
| `crates/dapple-data/assets/sql/dispatches/insert_dispatch.sql` | No change -- insert logic unchanged |
| `crates/dapple-data/assets/sql/dispatches/dispatch_stages/insert_dispatch_stages.sql` | No change -- called with derived args |
| `crates/dapple-data/assets/sql/dispatches/dispatch_steps/insert_dispatch_steps.sql` | No change |
| `crates/dapple-data/assets/sql/dispatches/dispatch_tasks/insert_dispatch_tasks.sql` | No change |
| `crates/dapple-data/src/model/dispatches/dispatch_stages.rs` | No change -- `InsertDispatchStagesArgs` stays the same |
| `crates/dapple-data/src/model/dispatches/dispatch_steps.rs` | No change -- `InsertDispatchStepsArgs` stays the same |
| `crates/dapple-data/src/model/dispatches/dispatch_tasks.rs` | No change |
| `crates/dapple-data/src/query/dispatches/dispatch_stages.rs` | No change |
| `crates/dapple-data/src/query/dispatches/dispatch_steps.rs` | No change |
| All existing dispatch SELECT queries | No change -- response format unchanged |

## 9. Testing Strategy

### 9.1 Update Existing Tests

#### `tests/consumer/dispatches.rs`

Update `fake_create_schema_with_stage_limit()` to produce the new schema:

```rust
fn fake_create_schema(&self) -> CreateDispatchSchema {
    let guard = self.context.lock().unwrap_or_else(|p| p.into_inner());
    let journey = guard.random_journey_from_any_batch().expect("no journeys");

    // Find a playbook with steps that have tasks
    let playbook = /* same logic as before */;
    let stage = playbook.stages.iter()
        .find(|st| st.steps.iter().any(|step| !step.tasks.is_empty()))
        .expect("no valid stage");

    let step_schemas: Vec<CreateDispatchStepSchema> = stage.steps.iter()
        .filter(|step| !step.tasks.is_empty())
        .map(|step| CreateDispatchStepSchema {
            playbook_step_id: step.object_id,
            params: json!({}),
            tasks: step.tasks.iter()
                .map(|t| CreateDispatchTaskSchema { playbook_task_id: t.object_id })
                .collect(),
        })
        .collect();

    CreateDispatchSchema {
        journey_id: journey.object_id,
        intent: json!({}),
        steps: step_schemas,
    }
}
```

#### `tests/user_lifecycle.rs` -- `dispatch_execution_trial`

Update the dispatch body from:

```json
{
  "stages": [{
    "playbook_stage_id": "<uuid>",
    "params": {},
    "steps": [{ ... }]
  }]
}
```

To:

```json
{
  "steps": [{
    "playbook_step_id": "<uuid>",
    "params": {},
    "tasks": [{ "playbook_task_id": "<uuid>" }]
  }]
}
```

#### `dispatch_nushell_pipeline_integration.rs`

Both integration tests (`dispatch_nushell_pipeline_integration` and `execute_dispatch_with_context_accumulation`) build `InsertDispatchArgs` directly (not via the API schema). These tests exercise the data layer's `insert_dispatches()` which still takes `InsertDispatchArgs` with stages. **These tests can remain unchanged** since they test the data layer, not the API schema.

However, for consistency, they should be updated to test the data layer with the same stage-derived pattern the handler uses. This is optional and can be deferred.

### 9.2 New Integration Test: Journey Context Seeding

Add a new trial to `tests/user_lifecycle.rs`:

```rust
let dispatch_context_seeding_trial = async_trial(&rt, "dispatch_context_seeding", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            let client = reqwest::Client::new();
            let auth = test_auth_header();

            // 1. Create task A: produces { items: [...] }
            // 2. Create task B: reads $in.items from prior context
            // 3. Create step A with task A, step B with task B
            // 4. Create playbook with both steps (in same or different stages)
            // 5. Create journey
            // 6. Create dispatch 1 targeting step A only
            // 7. Poll for dispatch 1 completion
            // 8. Create dispatch 2 targeting step B only
            //    (execute_dispatch should seed $in with task A's output from dispatch 1)
            // 9. Poll for dispatch 2 completion
            // 10. Verify task B saw task A's output via context seeding

            Ok(())
        }
    }
});
```

This trial verifies that:
- Dispatch 2 in the same journey can read outputs from dispatch 1
- The context seeding happens automatically via `execute_dispatch()`
- Task B's script can access `$in.items` even though `items` was produced by a different dispatch

### 9.3 New Integration Test: ByJourney Query

Test the new `ByJourney` query variant:

```rust
// After creating dispatches for a journey, verify ByJourney returns them
let resp = client
    .get(base_url.join(&format!(
        "api/dispatches/?query=by_journey&journey_id={}",
        journey_oid
    )).unwrap())
    .header("authorization", &auth)
    .send()
    .await?;

let body: serde_json::Value = resp.json().await?;
let objs = body["objs"].as_array().expect("should have objs");
assert!(objs.len() >= 2, "should see both dispatches for this journey");
```

### 9.4 Test Dependencies

```text
dispatch_context_seeding_trial  -- Level 7 (after dispatches_trial)
    depends on: categories (for playbook creation)
    creates its own: task, step, playbook, journey, 2 dispatches
```

This trial is self-contained (creates its own data) so it only depends on `categories_trial` for playbook creation.

## 10. Performance Considerations

### Stage Resolution Query

The `resolve_playbook_stages_for_steps.sql` query is a single round-trip that resolves all steps to stages via `UNNEST + JOIN`. For a typical dispatch with 1-5 steps, this adds negligible overhead.

### Context Seeding Query

The `select_completed_task_context_by_journey.sql` query is the most expensive new query. For a journey with many dispatches, it scans all task attempts across all prior dispatches. Mitigations:

- The query uses `DISTINCT ON (a.task_id)` which allows PostgreSQL to use an index scan on `(task_id, created_at DESC)`.
- The existing index `dispatch_step_task_attempts_task_id_id_idx` on `(task_id ASC, id DESC)` supports this.
- For journeys with >100 dispatches, consider adding an explicit index on `(dispatch_step_id, task_id)` for the join path. This is deferred until performance data shows it is needed.

### Graceful Degradation

Context seeding failure does not block dispatch execution. If the query fails or returns no results, `execute_dispatch()` proceeds with only the intent-seeded data. This ensures the critical path (dispatch execution) is not blocked by the advisory context seeding path.

## 11. Open Questions and Risks

### Resolved

1. **How to derive the dispatch_stage from the step's playbook_step_id?**
   Resolved: Single batch query `resolve_playbook_stages_for_steps.sql` joins `playbook_steps -> playbook_stages`.

2. **Should we keep backward compatibility with the stage-level schema?**
   Resolved: No. The old `stages`-based schema is removed. The endpoint is internal, and the response format is unchanged.

3. **How should context seeding handle key collisions?**
   Resolved: Most recent task output wins (chronological order). Intent is seeded first, then overwritten by prior task outputs.

### Open

1. **Stage params for auto-derived stages** -- Currently auto-derived stages get `params: json!({})`. If a use case arises where callers need to set stage-level params, we could add an optional `stage_params` field to `CreateDispatchStepSchema` that overrides the default. Deferred until needed.

2. **Context seeding for very long journeys** -- A journey with hundreds of dispatches and thousands of task outputs could result in a large `accumulated_data` map. If this becomes a problem, we could add a configurable `max_context_items` limit or allow the caller to specify which prior task outputs to include. Deferred until performance data shows it is needed.

3. **Context seeding and task idempotency** -- If the same task runs in multiple dispatches (e.g., a retry), the most recent output wins. This is correct for the "latest output" semantic but could be surprising if a task is expected to produce cumulative results. Document this behavior for guide authors.

## 12. AGENTS.md Updates

No updates to AGENTS.md files are required for this change. The existing patterns for dispatch handling, CTE-based resolution, and handler-level schema conversion already cover the concepts used here. The change is localized to the dispatch subsystem and does not introduce new architectural patterns.

If the implementation agent needs guidance, the key principles are:
- **Stage derivation happens in the handler, not SQL** -- The existing SQL insert files are unchanged.
- **Context seeding is advisory** -- Failure to load prior context does not block execution.
- **Response format is unchanged** -- Only the request schema changes.
