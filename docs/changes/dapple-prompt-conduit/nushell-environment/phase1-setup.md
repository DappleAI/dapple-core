# Nushell Environment Setup Plan

This document captures the architectural decisions and DDL changes needed to set up the Nushell execution environment for dapple-prompt-flow.

## Overview

The Nushell environment provides a sandboxed, virtual-machine-like shell where:
- Standard shell commands operate on domain objects (assets, context, prompts)
- Prompting commands are first-class citizens
- LLM context is managed via static overlay definitions

## Architecture Components

### 1. dapple-prompt-flow (existing)
- StepEngine with Nushell integration
- Nu commands for prompt execution
- Communicates with Conduit via HTTP/SSE

### 2. dapple-prompt-conduit (new crate)
- Standalone service (own binary, Docker container)
- Uses dapple-data directly for DB access
- Responsibilities:
  - Fetch prompt + fluent_messages by object_id
  - Resolve overlay visibility (stage → step → task cascade)
  - Select appropriate engine body based on playbook config
  - Render Handlebars templates with context
  - Route to LLM backend (swiftide, rig, NIM)
  - Stream rich events back (Token, Metadata, Done, Error)
  - Manage LLM context (RAG) with sticky proxy per step

### 3. Overlay System
- Static visibility definitions at stage/step/task levels
- Controls what value_slots (including assets) the LLM sees
- Cascades: stage → step → task (later overrides earlier)

---

## DDL Changes

### 1. Remove unique constraint on task_prompts

**Rationale**: Allow tasks to call multiple prompts by name. Currently enforces 1:1.

```sql
-- Migration: YYYYMMDD_task_prompts_allow_multiple.sql

ALTER TABLE rev.task_prompts
  DROP CONSTRAINT task_prompts_task_id_key;

-- Add index for lookups by task
CREATE INDEX IF NOT EXISTS task_prompts_task_id_idx
  ON rev.task_prompts (task_id);
```

---

### 2. Playbook Stage Overlays

**Rationale**: Define which value_slots are exposed or masked at the stage level.

```sql
-- Migration: YYYYMMDD_playbook_stage_overlays.sql

CREATE TABLE rev.playbook_stage_overlays (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL,

    playbook_stage_id BIGINT NOT NULL REFERENCES rev.playbook_stages (id),
    value_slot_path_id BIGINT NOT NULL REFERENCES rev.value_slot_paths (id),

    -- 'expose' = add to visible set, 'mask' = remove from visible set
    visibility TEXT NOT NULL CHECK (visibility IN ('expose', 'mask')),

    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,

    UNIQUE (object_id),
    UNIQUE (playbook_stage_id, value_slot_path_id)
);

ALTER TABLE rev.playbook_stage_overlays OWNER TO dapple;

CREATE INDEX playbook_stage_overlays_stage_idx
  ON rev.playbook_stage_overlays (playbook_stage_id);
```

---

### 3. Playbook Step Overlays

**Rationale**: Override stage-level visibility at the step level.

```sql
-- Migration: YYYYMMDD_playbook_step_overlays.sql

CREATE TABLE rev.playbook_step_overlays (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL,

    playbook_step_id BIGINT NOT NULL REFERENCES rev.playbook_steps (id),
    value_slot_path_id BIGINT NOT NULL REFERENCES rev.value_slot_paths (id),

    -- 'expose' = add to visible set, 'mask' = remove from visible set
    visibility TEXT NOT NULL CHECK (visibility IN ('expose', 'mask')),

    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,

    UNIQUE (object_id),
    UNIQUE (playbook_step_id, value_slot_path_id)
);

ALTER TABLE rev.playbook_step_overlays OWNER TO dapple;

CREATE INDEX playbook_step_overlays_step_idx
  ON rev.playbook_step_overlays (playbook_step_id);
```

---

### 4. Playbook Task Overlays

**Rationale**: Finest granularity - override step-level visibility at the task level.

```sql
-- Migration: YYYYMMDD_playbook_task_overlays.sql

CREATE TABLE rev.playbook_task_overlays (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID DEFAULT gen_random_uuid() NOT NULL,

    playbook_task_id BIGINT NOT NULL REFERENCES rev.playbook_tasks (id),
    value_slot_path_id BIGINT NOT NULL REFERENCES rev.value_slot_paths (id),

    -- 'expose' = add to visible set, 'mask' = remove from visible set
    visibility TEXT NOT NULL CHECK (visibility IN ('expose', 'mask')),

    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,

    UNIQUE (object_id),
    UNIQUE (playbook_task_id, value_slot_path_id)
);

ALTER TABLE rev.playbook_task_overlays OWNER TO dapple;

CREATE INDEX playbook_task_overlays_task_idx
  ON rev.playbook_task_overlays (playbook_task_id);
```

---

## Overlay Resolution Logic

Conduit computes visible slots by cascading static definitions:

```
Input: playbook_task_id

1. Find parent playbook_step_id and playbook_stage_id

2. Initialize visible_slots = {} (empty set, explicit expose model)
   -- OR visible_slots = all_bound_slots (full set, explicit mask model)
   -- TBD: decide default

3. Apply stage overlays:
   FOR each row in playbook_stage_overlays WHERE playbook_stage_id = ?:
     IF visibility = 'expose': visible_slots.add(value_slot_path_id)
     IF visibility = 'mask': visible_slots.remove(value_slot_path_id)

4. Apply step overlays (overrides stage):
   FOR each row in playbook_step_overlays WHERE playbook_step_id = ?:
     IF visibility = 'expose': visible_slots.add(value_slot_path_id)
     IF visibility = 'mask': visible_slots.remove(value_slot_path_id)

5. Apply task overlays (overrides step):
   FOR each row in playbook_task_overlays WHERE playbook_task_id = ?:
     IF visibility = 'expose': visible_slots.add(value_slot_path_id)
     IF visibility = 'mask': visible_slots.remove(value_slot_path_id)

6. Return visible_slots
```

---

## Conduit Request/Response

### Request (from Flow to Conduit)

```rust
struct PromptRequest {
    prompt_object_id: Uuid,
    attempt_id: i64,  // Conduit fetches full context from this
}
```

### Response Stream (SSE events)

```rust
enum PromptEvent {
    Started {
        request_id: Uuid,
        model: String,
        prompt_object_id: Uuid,
    },
    Token(String),
    Done {
        output_tokens: u32,
        input_tokens: u32,
        latency_ms: u64,
        finish_reason: String,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
}
```

---

## Playbook Step Integrity Constraint

All prompts reachable from a playbook_step must have fluent_message_engine_bodies for the step's required engine.

**Validation approach** (TBD - application-level or DB constraint):
- When creating/updating playbook_step, verify all linked prompts have compatible engine bodies
- Fail fast at playbook creation time, not at dispatch time

---

## Nu Commands (dapple-prompt-flow)

### prompt send

```nushell
# Call prompt by path name (resolved via task_prompts binding)
$in | prompt send "summarize"

# Returns stream of tokens, can be collected or processed
$in | prompt send "summarize" | str join
```

### prompt retry

```nushell
# Retry last prompt with same or new input
prompt retry
$new_input | prompt retry
```

### prompt reset

```nushell
# Reset LLM context for this step
prompt reset
```

---

## Open Questions

1. **Default visibility**: Start empty (explicit expose) or start full (explicit mask)?

2. **Conduit response storage**: Should Conduit write responses to a new table? Or does Flow handle persistence via attempt events?

3. **LLM context (RAG) management**: How does Conduit load/prepare RAG context from visible value_slots? Integration with swiftide/rig?

---

## Implementation Order

1. DDL migrations (Data Engineer)
2. dapple-data models + queries for overlay tables
3. dapple-prompt-conduit crate scaffold
4. Conduit overlay resolution logic
5. Conduit prompt fetching + template rendering
6. Conduit LLM backend integration (swiftide/rig)
7. Conduit SSE streaming
8. dapple-prompt-flow Nu command registration
9. Integration tests
