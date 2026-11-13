# Directive Architecture Plan

**Status:** Draft
**Created:** 2026-01-22
**Last Updated:** 2026-01-22
**Target Version:** 0.0.3
**Dependencies:** Existing playbook hierarchy (playbook_stages, playbook_steps), versioned object infrastructure

## Summary

Directives are versioned objects that define conditional execution, retry, and branching logic for playbook workflows. A directive contains a `logic` JSONB column with a JSONLogic expression that includes a `run` primitive to execute stages/steps.

**Design approach:**
- Directives are independent versioned objects (like briefs or prompts)
- Stages and steps reference specific directive versions via relationship tables
- One directive can be reused across multiple stages/steps
- Directive versions can be updated without creating new playbook versions

**Key points:**
- `run` is a side-effectful primitive that executes the stage/step and returns its result
- JSONLogic evaluation is cheap; `run` is the expensive operation
- Default behavior when no directive attached: unconditional `run`, fail on error
- Each stage/step references at most one directive version (0..1 relationship)

## The `run` Primitive

`run` is a custom JSONLogic operator that executes the current stage or step.

```json
{ "run": [] }
```

**Returns:**
```json
{
  "status": "ok" | "error" | "timeout" | "canceled",
  "error_code": "RATE_LIMITED" | "TIMEOUT" | null,
  "data": { ... },
  "signals": { ... },
  "duration_ms": 1234
}
```

**Guarantees:**
- `run` is invoked at most once per directive evaluation
- Multiple references to `run` in the same expression access the same result
- After `run` executes, result is accessible via `{"var": "run.status"}`, `{"var": "run.data.X"}`, etc.

## Action Return Values

| Action | Meaning | Engine Behavior |
|--------|---------|-----------------|
| `"proceed"` | Continue normally | Move to next stage/step |
| `"skip"` | Skip this element | Move to next (no execution if `run` not called) |
| `"fail"` | Halt with error | Mark dispatch as failed, stop execution |
| `"retry"` | Re-execute with backoff | Wait `retry_delay_ms`, re-evaluate logic (up to `max_attempts`) |
| `"loop"` | Re-execute immediately | Re-evaluate logic without delay (up to `max_attempts`) |
| `"{uuid}"` | Branch to target | Jump to stage/step with that object_id |

**Branch scope:**
- Step directives can only branch to steps within the same stage
- Stage directives can branch to any stage in the playbook

## Database Schema (DDL)

### Entity Type Registration

```sql
-- Migration: 20260123000001_directive_entity_type.sql

INSERT INTO rev.entity_types (key, description) VALUES
    ('directive', 'Conditional execution logic for stages and steps');
```

### Directive Path Details

```sql
-- Migration: 20260123000002_directive_path_details.sql

CREATE TABLE rev.directive_path_details (
    path_id BIGINT PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL,
    UNIQUE (name)
);
```

### Directives Table

```sql
-- Migration: 20260123000003_directives.sql

CREATE TABLE rev.directives (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    versioned_object_id BIGINT NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),

    logic JSONB NOT NULL,
    max_attempts INT NOT NULL DEFAULT 1 CHECK (max_attempts > 0),
    retry_delay_ms INT CHECK (retry_delay_ms IS NULL OR retry_delay_ms >= 0),
    description TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE rev.directives OWNER TO dapple;

CREATE INDEX idx_directives_versioned_object_id
    ON rev.directives(versioned_object_id);
```

### Playbook Stage Directives (Relationship Table)

```sql
-- Migration: 20260123000004_playbook_stage_directives.sql

CREATE TABLE rev.playbook_stage_directives (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,

    playbook_stage_id BIGINT NOT NULL UNIQUE REFERENCES rev.playbook_stages(id),
    directive_id BIGINT NOT NULL REFERENCES rev.directives(id),

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE rev.playbook_stage_directives OWNER TO dapple;

CREATE INDEX idx_playbook_stage_directives_stage_id
    ON rev.playbook_stage_directives(playbook_stage_id);

CREATE INDEX idx_playbook_stage_directives_directive_id
    ON rev.playbook_stage_directives(directive_id);
```

### Playbook Step Directives (Relationship Table)

```sql
-- Migration: 20260123000005_playbook_step_directives.sql

CREATE TABLE rev.playbook_step_directives (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,

    playbook_step_id BIGINT NOT NULL UNIQUE REFERENCES rev.playbook_steps(id),
    directive_id BIGINT NOT NULL REFERENCES rev.directives(id),

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE rev.playbook_step_directives OWNER TO dapple;

CREATE INDEX idx_playbook_step_directives_step_id
    ON rev.playbook_step_directives(playbook_step_id);

CREATE INDEX idx_playbook_step_directives_directive_id
    ON rev.playbook_step_directives(directive_id);
```

## JSONLogic Examples

### Skip if Cached

```json
{
  "if": [
    { "var": "data.cache.has_result" },
    "skip",
    { "if": [
      { "==": [{ "var": { "run": [] }.status }, "ok"] },
      "proceed",
      "fail"
    ]}
  ]
}
```

### Branch by Classification

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "error"] },
    "fail",
    { "if": [
      { "==": [{ "var": "run.data.classification" }, "premium"] },
      "550e8400-e29b-41d4-a716-446655440000",
      "proceed"
    ]}
  ]
}
```

### Retry on Transient Failure

With `max_attempts: 3`, `retry_delay_ms: 1000`:

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "ok"] },
    "proceed",
    { "if": [
      { "in": [{ "var": "run.error_code" }, ["RATE_LIMITED", "TIMEOUT"]] },
      "retry",
      "fail"
    ]}
  ]
}
```

### Poll Until Complete

With `max_attempts: 20`, `retry_delay_ms: 3000`:

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "error"] },
    "fail",
    { "if": [
      { "==": [{ "var": "run.signals.processing_complete" }, true] },
      "proceed",
      "loop"
    ]}
  ]
}
```

### Validation Before Execution

```json
{
  "if": [
    { "!": { "var": "data.required_input.email" } },
    "fail",
    { "if": [
      { "==": [{ "var": { "run": [] }.status }, "ok"] },
      "proceed",
      "fail"
    ]}
  ]
}
```

### Default (No Directive Attached)

When no directive is attached, engine behaves as:

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "ok"] },
    "proceed",
    "fail"
  ]
}
```

## Execution Semantics

```python
def evaluate_stage_directive(stage, context):
    directive = get_stage_directive(stage.id)

    if directive is None:
        result = execute_stage(stage)
        return "proceed" if result.status == "ok" else "fail"

    attempt = 0
    while attempt < directive.max_attempts:
        attempt += 1

        eval_context = {
            "data": context.data,
            "metadata": context.metadata,
            "attempt": attempt,
            "max_attempts": directive.max_attempts,
        }

        action = evaluate_jsonlogic(
            directive.logic,
            eval_context,
            run_handler=lambda: execute_stage(stage)
        )

        if action in ("retry", "loop"):
            if attempt < directive.max_attempts:
                if action == "retry" and directive.retry_delay_ms:
                    sleep(directive.retry_delay_ms)
                continue
            else:
                return "fail"  # Exhausted attempts

        return action  # proceed, skip, fail, or branch UUID

    return "fail"
```

## Object Model

### Directive as Versioned Object

Directives follow the standard versioned object pattern:
- Path table stores name (immutable across versions)
- Versioned object table stores version info
- Detail table stores version-specific logic, max_attempts, retry_delay_ms

### DTO Structures

```rust
/// Public DTO for Directive
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Directive {
    pub object_id: Uuid,
    pub path_object_id: Uuid,
    pub name: String,
    pub version: String,  // "major.minor.patch"
    pub logic: Value,
    pub max_attempts: i32,
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
    pub created_at: DateTime,
}

/// Reference to a specific directive version
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct DirectiveRef {
    pub object_id: Uuid,
}

/// Relationship entry for stage → directive
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct PlaybookStageDirective {
    pub object_id: Uuid,
    pub playbook_stage_object_id: Uuid,
    pub directive: DirectiveRef,
    pub created_at: DateTime,
}

/// Relationship entry for step → directive
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct PlaybookStepDirective {
    pub object_id: Uuid,
    pub playbook_step_object_id: Uuid,
    pub directive: DirectiveRef,
    pub created_at: DateTime,
}
```

### Embedding in PlaybookStage/PlaybookStep DTOs

```rust
/// Updated PlaybookStage DTO with optional directive reference
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PlaybookStage {
    pub object_id: Uuid,
    pub playbook_object_id: Uuid,
    pub steps: Vec<PlaybookStep>,
    pub directive: Option<DirectiveRef>,  // NEW
    pub created_at: DateTime,
}

/// Updated PlaybookStep DTO with optional directive reference
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PlaybookStep {
    pub object_id: Uuid,
    pub playbook_object_id: Uuid,
    pub step: StepReference,
    pub tasks: Vec<PlaybookTask>,
    pub directive: Option<DirectiveRef>,  // NEW
    pub created_at: DateTime,
}
```

### Create Schemas

```rust
/// Create a new directive (versioned object)
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize)]
pub struct CreateDirectiveSchema {
    pub name: String,
    pub logic: Value,
    pub max_attempts: Option<i32>,       // Default: 1
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
}

/// Attach a directive to a playbook stage
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize)]
pub struct AttachStageDirectiveSchema {
    pub playbook_stage_object_id: Uuid,
    pub directive_object_id: Uuid,
}

/// Attach a directive to a playbook step
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize)]
pub struct AttachStepDirectiveSchema {
    pub playbook_step_object_id: Uuid,
    pub directive_object_id: Uuid,
}

/// Alternative: Include directive reference in playbook creation
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize)]
pub struct CreatePlaybookStageSchema {
    pub stage_object_id: Uuid,
    pub directive_object_id: Option<Uuid>,  // Reference existing directive version
}

#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize)]
pub struct CreatePlaybookStepSchema {
    pub step_object_id: Uuid,
    pub directive_object_id: Option<Uuid>,  // Reference existing directive version
}
```

## API Endpoints

### Directive CRUD (Versioned Object Pattern)

```
POST   /api/directives              - Create new directive (creates path + first version)
GET    /api/directives              - List directives (latest versions)
GET    /api/directives?object_ids=  - Get directives by object_id
GET    /api/directives?path_ids=    - Get directives by path_object_id (all versions)
PATCH  /api/directives              - Create new version of existing directive
```

### Stage/Step Directive Attachments

The attachment happens during playbook creation or update:
- Pass `directive_object_id` in CreatePlaybookStageSchema/CreatePlaybookStepSchema
- Attachments are owned by the playbook version (created/deleted with playbook)

No separate endpoints for attachments - they are associative dependents of playbook stages/steps.

## SQL Query Strategy

### Insert Directive

```sql
-- insert_directives.sql
WITH args AS (
    SELECT * FROM UNNEST(
        $1::TEXT[],        -- names
        $2::JSONB[],       -- logic
        $3::INT[],         -- max_attempts
        $4::INT[],         -- retry_delay_ms
        $5::TEXT[]         -- descriptions
    ) WITH ORDINALITY AS t(name, logic, max_attempts, retry_delay_ms, description, ord)
),
new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT et.id
    FROM args
    CROSS JOIN rev.entity_types et
    WHERE et.key = 'directive'
    ORDER BY args.ord
    RETURNING id, object_id, row_number() OVER () AS ord
),
new_path_details AS (
    INSERT INTO rev.directive_path_details (path_id, name)
    SELECT np.id, a.name
    FROM new_paths np
    JOIN args a ON np.ord = a.ord
    RETURNING path_id, name
),
new_versioned_objects AS (
    INSERT INTO rev.versioned_objects (path_id, version_major, version_minor, version_patch)
    SELECT np.id, 0, 0, 1
    FROM new_paths np
    ORDER BY np.ord
    RETURNING id, object_id, path_id, version_major, version_minor, version_patch, created_at,
              row_number() OVER () AS ord
),
new_directives AS (
    INSERT INTO rev.directives (
        versioned_object_id,
        logic,
        max_attempts,
        retry_delay_ms,
        description
    )
    SELECT
        nvo.id,
        a.logic,
        COALESCE(a.max_attempts, 1),
        a.retry_delay_ms,
        a.description
    FROM new_versioned_objects nvo
    JOIN args a ON nvo.ord = a.ord
    RETURNING id, versioned_object_id, logic, max_attempts, retry_delay_ms, description, created_at
)
SELECT
    nd.id,
    nvo.object_id,
    op.object_id AS path_object_id,
    npd.name,
    nvo.version_major,
    nvo.version_minor,
    nvo.version_patch,
    nd.logic,
    nd.max_attempts,
    nd.retry_delay_ms,
    nd.description,
    nd.created_at,
    nvo.ord
FROM new_directives nd
JOIN new_versioned_objects nvo ON nvo.id = nd.versioned_object_id
JOIN new_paths np ON np.ord = nvo.ord
JOIN rev.object_paths op ON op.id = np.id
JOIN new_path_details npd ON npd.path_id = np.id
ORDER BY nvo.ord;
```

### Attach Directive to Stage

```sql
-- insert_playbook_stage_directives.sql
WITH args AS (
    SELECT * FROM UNNEST(
        $1::UUID[],  -- playbook_stage_object_ids
        $2::UUID[]   -- directive_object_ids
    ) WITH ORDINALITY AS t(stage_object_id, directive_object_id, ord)
),
stage_refs AS (
    SELECT a.ord, ps.id AS stage_id, ps.object_id AS stage_object_id
    FROM args a
    JOIN rev.playbook_stages ps ON ps.object_id = a.stage_object_id
),
directive_refs AS (
    SELECT a.ord, d.id AS directive_id, vo.object_id AS directive_object_id
    FROM args a
    JOIN rev.versioned_objects vo ON vo.object_id = a.directive_object_id
    JOIN rev.directives d ON d.versioned_object_id = vo.id
),
new_attachments AS (
    INSERT INTO rev.playbook_stage_directives (playbook_stage_id, directive_id)
    SELECT sr.stage_id, dr.directive_id
    FROM stage_refs sr
    JOIN directive_refs dr ON sr.ord = dr.ord
    ORDER BY sr.ord
    RETURNING id, object_id, playbook_stage_id, directive_id, created_at
)
SELECT
    na.id,
    na.object_id,
    na.playbook_stage_id,
    sr.stage_object_id AS playbook_stage_object_id,
    na.directive_id,
    dr.directive_object_id,
    na.created_at,
    sr.ord
FROM new_attachments na
JOIN stage_refs sr ON sr.stage_id = na.playbook_stage_id
JOIN directive_refs dr ON dr.directive_id = na.directive_id
ORDER BY sr.ord;
```

### Select Directives for Playbook Stages

```sql
-- select_directives_for_playbook_stages.sql
WITH stage_ids AS (
    SELECT * FROM UNNEST($1::BIGINT[]) WITH ORDINALITY AS t(stage_id, ord)
)
SELECT
    psd.id,
    psd.object_id,
    psd.playbook_stage_id,
    ps.object_id AS playbook_stage_object_id,
    psd.directive_id,
    vo.object_id AS directive_object_id,
    op.object_id AS directive_path_object_id,
    dpd.name AS directive_name,
    vo.version_major,
    vo.version_minor,
    vo.version_patch,
    d.logic,
    d.max_attempts,
    d.retry_delay_ms,
    d.description,
    psd.created_at,
    si.ord
FROM stage_ids si
JOIN rev.playbook_stages ps ON ps.id = si.stage_id
LEFT JOIN rev.playbook_stage_directives psd ON psd.playbook_stage_id = ps.id
LEFT JOIN rev.directives d ON d.id = psd.directive_id
LEFT JOIN rev.versioned_objects vo ON vo.id = d.versioned_object_id
LEFT JOIN rev.object_paths op ON op.id = vo.path_id
LEFT JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
ORDER BY si.ord;
```

## Module Organization

```
crates/dapple-data/
├── migrations/
│   ├── 20260123000001_directive_entity_type.sql
│   ├── 20260123000002_directive_path_details.sql
│   ├── 20260123000003_directives.sql
│   ├── 20260123000004_playbook_stage_directives.sql
│   └── 20260123000005_playbook_step_directives.sql
├── assets/sql/
│   └── directives/
│       ├── insert_directives.sql
│       ├── select_directives_by_object_ids.sql
│       ├── select_directives_by_path_ids.sql
│       ├── update_directives.sql  (creates new version)
│       ├── playbook_stage_directives/
│       │   ├── insert_playbook_stage_directives.sql
│       │   └── select_directives_for_playbook_stages.sql
│       └── playbook_step_directives/
│           ├── insert_playbook_step_directives.sql
│           └── select_directives_for_playbook_steps.sql
└── src/
    └── model/
        ├── directives.rs
        └── playbook_directives.rs

crates/dapple-prompt-flow/
└── src/
    ├── directives/
    │   ├── mod.rs
    │   ├── evaluator.rs
    │   ├── run_handler.rs
    │   └── actions.rs
    └── dispatch_runner.rs

crates/dapple-prompt-api/
└── src/
    └── endpoints/
        └── directives.rs
```

## Validation Requirements

At directive creation time:
1. Expression must be valid JSONLogic syntax
2. Expression must contain exactly one `{"run": []}` operation
3. Branch target UUIDs must reference valid stages/steps in scope (validated at attachment time)
4. All `{"var": "..."}` references must use known namespaces: `data`, `metadata`, `run`, `attempt`

At attachment time:
1. Directive must exist and be non-deprecated
2. Stage/step must not already have a directive attached
3. For step directives: branch targets must be steps in the same stage

## Audit Trail

Flow decisions are recorded in `dispatch_step_task_attempt_events`:

```json
{
  "phase_code": "flow_directive",
  "context_metadata": {
    "directive_object_id": "...",
    "directive_path_object_id": "...",
    "attempt": 1,
    "action": "retry",
    "run_result": { "status": "error", "error_code": "RATE_LIMITED" }
  }
}
```

## Acceptance Criteria

- [ ] Entity type 'directive' registered in entity_types table
- [ ] directive_path_details table created
- [ ] directives table created with versioned_object_id FK
- [ ] playbook_stage_directives table created with UNIQUE on stage_id
- [ ] playbook_step_directives table created with UNIQUE on step_id
- [ ] Directive CRUD endpoints implemented (POST, GET, PATCH)
- [ ] PlaybookStage DTO includes optional `directive` reference
- [ ] PlaybookStep DTO includes optional `directive` reference
- [ ] Playbook creation accepts directive_object_id for stages/steps
- [ ] Playbook GET responses include directive references
- [ ] Directive expression validation at creation time
- [ ] Branch target validation at attachment time
- [ ] `run` primitive implemented in flow evaluator
- [ ] dispatch_runner evaluates directives before/during stage execution
- [ ] Retry/loop behavior respects max_attempts and retry_delay_ms
- [ ] Flow decisions recorded in dispatch_task_attempt_events
- [ ] Directive versioning: PATCH creates new version, old version unchanged
- [ ] Integration tests for skip, retry, loop, branch patterns
- [ ] Integration tests for directive reuse across stages/steps
- [ ] Unit tests for directive evaluator

## AGENTS.md Updates

Add the following section to AGENTS.md under "Common Agent Tasks" or as a new top-level section titled "Directive Pattern":

---

### Directive Pattern

Directives are versioned objects that define conditional execution, retry, and branching logic for playbook stages and steps. They enable dynamic workflow behavior without modifying playbook structure.

#### When to Use Directives

Use directives when you need:
- **Conditional execution** — Skip stages/steps based on data state
- **Retry logic** — Retry transient failures (rate limits, timeouts)
- **Polling patterns** — Wait for external processing to complete
- **Branching** — Route execution to different stages/steps based on results
- **Validation gates** — Fail early if required data is missing

Do NOT use directives for:
- Simple linear execution (default behavior is sufficient)
- Complex business logic (use task handlers instead)
- User-facing decisions (use BriefPacks and DispatchIntentBindings)

#### The `run` Primitive

Every directive must contain exactly one `{"run": []}` operation in its JSONLogic expression. This primitive:

1. Executes the stage/step being controlled
2. Returns a result object with `status`, `error_code`, `data`, `signals`, `duration_ms`
3. Is invoked at most once per evaluation (memoized)

```json
// Access run result via var references
{ "var": "run.status" }        // "ok" | "error" | "timeout" | "canceled"
{ "var": "run.error_code" }    // "RATE_LIMITED" | "TIMEOUT" | null
{ "var": "run.data.field" }    // Data returned by the execution
{ "var": "run.signals.flag" }  // Signal values from execution
```

#### Action Return Values

| Action | Meaning | Engine Behavior |
|--------|---------|-----------------|
| `"proceed"` | Continue normally | Move to next stage/step |
| `"skip"` | Skip this element | Move to next (no execution if `run` not called) |
| `"fail"` | Halt with error | Mark dispatch as failed, stop execution |
| `"retry"` | Re-execute with backoff | Wait `retry_delay_ms`, re-evaluate (up to `max_attempts`) |
| `"loop"` | Re-execute immediately | Re-evaluate without delay (up to `max_attempts`) |
| `"{uuid}"` | Branch to target | Jump to stage/step with that object_id |

#### Directive Object Model

Directives follow the standard versioned object pattern:

```rust
// Directive is independent and reusable
POST /api/directives  // Create new directive
PATCH /api/directives // Create new version of existing directive
GET /api/directives   // Query directives by object_id or path_object_id

// Stages and steps reference directives via relationship tables
// References are made during playbook creation/update
CreatePlaybookStageSchema {
    stage_object_id: Uuid,
    directive_object_id: Option<Uuid>,  // Reference existing directive version
}
```

Key relationships:
- **Directives are principals** — Independent versioned objects with their own CRUD
- **Stages/steps reference directives** — Via relationship tables (playbook_stage_directives, playbook_step_directives)
- **One directive per element** — Each stage/step has at most one directive attached
- **Reusable** — Same directive version can be attached to multiple stages/steps

#### Branch Scope Rules

- **Step directives** can only branch to steps within the same stage
- **Stage directives** can branch to any stage in the playbook

#### Implementation Patterns

**Do:**
- Create directives as independent objects before attaching to playbooks
- Validate branch targets at attachment time (not directive creation)
- Record flow decisions in dispatch_step_task_attempt_events for audit
- Respect max_attempts and retry_delay_ms settings

**Don't:**
- Create directives inline with playbook creation (no embedding)
- Allow directives to be modified after attachment (create new version instead)
- Skip validation of `run` primitive (exactly one required)
- Ignore branch scope rules (step-to-step must be same stage)

#### Default Behavior

When no directive is attached to a stage/step, the engine behaves as:

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "ok"] },
    "proceed",
    "fail"
  ]
}
```

This means: run unconditionally, proceed on success, fail on error.

---

## README.md Updates

Add the following section to README.md, either in "Key Features" as a bullet point or as a new section titled "Directives":

---

### Directives

Directives provide conditional execution, retry, and branching logic for playbook workflows. They use [JSONLogic](https://jsonlogic.com/) expressions to make runtime decisions about stage and step execution.

#### Overview

A directive is a versioned object that controls whether/how a stage or step executes:

- **Skip** execution based on cached data or preconditions
- **Retry** transient failures (rate limits, timeouts) with configurable backoff
- **Poll** for external processing completion
- **Branch** to different stages/steps based on classification results
- **Fail fast** when required data is missing

#### Creating Directives

```bash
# Create a retry-on-transient-failure directive
curl -X POST http://localhost:8000/api/directives \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "retry-transient-errors",
      "logic": {
        "if": [
          { "==": [{ "var": { "run": [] }.status }, "ok"] },
          "proceed",
          { "if": [
            { "in": [{ "var": "run.error_code" }, ["RATE_LIMITED", "TIMEOUT"]] },
            "retry",
            "fail"
          ]}
        ]
      },
      "max_attempts": 3,
      "retry_delay_ms": 2000,
      "description": "Retry up to 3 times on rate limits or timeouts"
    }]
  }'
```

#### Attaching Directives

Directives are attached to playbook stages and steps during playbook creation:

```bash
# Create playbook with directive-controlled stage
curl -X POST http://localhost:8000/api/playbooks \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "My Workflow",
      "stages": [{
        "stage_object_id": "stage-uuid",
        "directive_object_id": "directive-uuid"  # Attach directive
      }]
    }]
  }'
```

#### JSONLogic Expression Examples

**Skip if Already Cached:**
```json
{
  "if": [
    { "var": "data.cache.has_result" },
    "skip",
    { "if": [
      { "==": [{ "var": { "run": [] }.status }, "ok"] },
      "proceed",
      "fail"
    ]}
  ]
}
```

**Branch by Classification:**
```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "error"] },
    "fail",
    { "if": [
      { "==": [{ "var": "run.data.classification" }, "premium"] },
      "550e8400-e29b-41d4-a716-446655440000",
      "proceed"
    ]}
  ]
}
```

**Poll Until Complete:**
```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "error"] },
    "fail",
    { "if": [
      { "==": [{ "var": "run.signals.processing_complete" }, true] },
      "proceed",
      "loop"
    ]}
  ]
}
```

**Validation Gate:**
```json
{
  "if": [
    { "!": { "var": "data.required_input.email" } },
    "fail",
    { "if": [
      { "==": [{ "var": { "run": [] }.status }, "ok"] },
      "proceed",
      "fail"
    ]}
  ]
}
```

#### The `run` Primitive

The `run` primitive is a custom JSONLogic operator that executes the stage/step:

```json
{ "run": [] }
```

It returns:
```json
{
  "status": "ok" | "error" | "timeout" | "canceled",
  "error_code": "RATE_LIMITED" | "TIMEOUT" | null,
  "data": { ... },
  "signals": { ... },
  "duration_ms": 1234
}
```

Access results via `{"var": "run.status"}`, `{"var": "run.data.field"}`, etc.

#### Action Values

| Action | Description |
|--------|-------------|
| `"proceed"` | Continue to next stage/step |
| `"skip"` | Skip this element, continue |
| `"fail"` | Halt execution with error |
| `"retry"` | Re-execute after `retry_delay_ms` |
| `"loop"` | Re-execute immediately |
| `"{uuid}"` | Branch to stage/step with that object_id |

#### API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/directives` | Create new directive |
| `GET` | `/api/directives` | List directives (latest versions) |
| `GET` | `/api/directives?object_ids=` | Get directives by specific version |
| `GET` | `/api/directives?path_ids=` | Get all versions of directives |
| `PATCH` | `/api/directives` | Create new version of existing directive |

Directives are versioned objects: PATCH creates a new version with a new `object_id`, leaving the original unchanged.

#### Default Behavior

Stages and steps without attached directives execute unconditionally: run, proceed on success, fail on error.
