# Nushell Environment Setup - Phase 2: Conduit Implementation

This document continues from the Phase 1 plan and addresses the next implementation steps for dapple-prompt-conduit.

## ⚠️ PRIORITY 1: Fix task_prompts local_name Deviation

The Phase 1 implementation added a `local_name` column to `task_prompts`. This deviates from the original design where prompt lookup should use `prompt_paths.name`.

### Problem

Current (incorrect):
```sql
-- task_prompts has local_name column
SELECT ... FROM task_prompts WHERE task_id = $1 AND local_name = $2
```

Expected (correct):
```sql
-- Lookup via prompt_paths.name through the join
SELECT ... FROM task_prompts tp
JOIN prompts p ON p.id = tp.prompt_id
JOIN prompt_paths pp ON pp.id = p.prompt_path_id
WHERE tp.task_id = $1 AND pp.name = $2
```

### Required Changes

#### 1. Migration to remove local_name

```sql
-- Migration: YYYYMMDD_task_prompts_remove_local_name.sql

-- Drop the constraint first
ALTER TABLE rev.task_prompts
  DROP CONSTRAINT IF EXISTS task_prompts_task_id_local_name_key;

-- Remove the column
ALTER TABLE rev.task_prompts
  DROP COLUMN IF EXISTS local_name;

-- The task_prompts_task_id_idx already exists from Phase 1, keep it
```

#### 2. Update select query

Replace `select_task_prompt_by_task_id_and_local_name.sql` with `select_task_prompt_by_task_id_and_prompt_name.sql`:

```sql
-- Select a task_prompt by task internal ID and prompt path name
-- Parameters:
--   $1: task_id (BIGINT) - internal task ID
--   $2: prompt_name (TEXT) - prompt path name (from prompt_paths.name)

SELECT
    tp.id,
    tp.object_id,
    tp.task_id,
    t.object_id AS task_object_id,
    tp.prompt_id,
    p.object_id AS prompt_object_id,
    pp.name AS prompt_name,
    tp.created_at
FROM rev.task_prompts tp
JOIN rev.tasks t ON t.id = tp.task_id
JOIN rev.prompts p ON p.id = tp.prompt_id
JOIN rev.prompt_paths pp ON pp.id = p.prompt_path_id
WHERE tp.task_id = $1 AND pp.name = $2;
```

#### 3. Update Rust models

Remove `local_name` from:
- `InsertedTaskPromptModel`
- `TaskPromptModel`
- `TaskPrompt` (DTO)
- `InsertTaskPromptsArgs`

Add `prompt_name` (derived from join) to:
- `TaskPromptModel`
- `TaskPrompt` (DTO)

```rust
// In task_prompts.rs

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskPromptModel {
    pub id: i64,
    pub object_id: Uuid,
    pub task_id: i64,
    pub task_object_id: Uuid,
    pub prompt_id: i64,
    pub prompt_object_id: Uuid,
    pub prompt_name: String,  // From prompt_paths.name via join
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct TaskPrompt {
    pub object_id: Uuid,
    pub task: crate::reference::TaskReference,
    pub prompt: crate::reference::PromptReference,
    pub prompt_name: String,  // From prompt_paths.name
    pub created_at: DateTime,
}

// InsertTaskPromptsArgs does NOT need prompt_name - it's derived from the prompt
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertTaskPromptsArgs {
    pub task_object_ids: Vec<Uuid>,
    pub prompt_object_ids: Vec<Uuid>,
    // No local_names - name comes from prompt_paths.name
}
```

#### 4. Update insert query

The insert query should return prompt_name from the join:

```sql
-- insert_task_prompts.sql (update RETURNING to include prompt_name)

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],   -- task_object_ids
        $2::UUID[]    -- prompt_object_ids
    ) WITH ORDINALITY AS t(task_object_id, prompt_object_id, ord)
),
task_ids AS (
    SELECT a.ord, t.id AS task_id, a.task_object_id
    FROM args a
    JOIN rev.tasks t ON t.object_id = a.task_object_id
),
prompt_ids AS (
    SELECT a.ord, p.id AS prompt_id, a.prompt_object_id, pp.name AS prompt_name
    FROM args a
    JOIN rev.prompts p ON p.object_id = a.prompt_object_id
    JOIN rev.prompt_paths pp ON pp.id = p.prompt_path_id
),
inserted AS (
    INSERT INTO rev.task_prompts (task_id, prompt_id)
    SELECT ti.task_id, pi.prompt_id
    FROM task_ids ti
    JOIN prompt_ids pi ON pi.ord = ti.ord
    RETURNING id, object_id, task_id, prompt_id, created_at
)
SELECT
    i.id,
    i.object_id,
    i.task_id,
    ti.task_object_id,
    i.prompt_id,
    pi.prompt_object_id,
    pi.prompt_name,
    i.created_at
FROM inserted i
JOIN task_ids ti ON ti.task_id = i.task_id
JOIN prompt_ids pi ON pi.prompt_id = i.prompt_id
ORDER BY ti.ord;
```

#### 5. Delete old query file

Remove: `select_task_prompt_by_task_id_and_local_name.sql`

---

## Phase 2A: Template Service Implementation

After fixing the local_name deviation, implement the actual template rendering.

### Fetch Prompt with Fluent Messages

```sql
-- assets/sql/prompts/select_prompt_with_fluent_messages.sql
-- Fetch a prompt and all its fluent messages for rendering
-- Parameters:
--   $1: prompt_object_id (UUID)
--   $2: engine_maker (TEXT) - e.g., 'openai', 'anthropic'
--   $3: engine_key (TEXT) - e.g., 'gpt-4', 'claude-3-opus'

WITH target_prompt AS (
    SELECT p.id, p.object_id, pp.name AS prompt_name
    FROM rev.prompts p
    JOIN rev.prompt_paths pp ON pp.id = p.prompt_path_id
    WHERE p.object_id = $1
)
SELECT
    tp.prompt_name,
    pfm.role,
    -- Prefer engine-specific body, fall back to localized body
    COALESCE(
        fmeb.body,
        fmlb.body
    ) AS body,
    ROW_NUMBER() OVER (ORDER BY pfm.id) AS ordinal
FROM target_prompt tp
JOIN rev.prompt_fluent_messages pfm ON pfm.prompt_id = tp.id
JOIN rev.fluent_messages fm ON fm.id = pfm.fluent_message_id
LEFT JOIN rev.fluent_message_engine_bodies fmeb
    ON fmeb.fluent_message_id = fm.id
    AND fmeb.maker_key = $2
    AND fmeb.engine_key = $3
LEFT JOIN rev.fluent_message_localized_bodies fmlb
    ON fmlb.fluent_message_id = fm.id
    AND fmlb.locale_id = (SELECT id FROM rev.locales WHERE key = 'en-US' LIMIT 1)
ORDER BY ordinal;
```

### Template Service Implementation

```rust
// In dapple-prompt-conduit/src/service/template.rs

use dapple_data::DbPool;
use dapple_data::model::value_slots::ValueSlotPath;
use handlebars::Handlebars;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::error::ConduitError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct RenderedPrompt {
    pub messages: Vec<PromptMessage>,
    pub prompt_name: String,
}

pub struct TemplateService {
    handlebars: Handlebars<'static>,
}

impl TemplateService {
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();
        handlebars.set_strict_mode(true); // Fail on missing variables
        Self { handlebars }
    }

    /// Render a prompt for a specific engine
    pub async fn render_prompt(
        &self,
        pool: &DbPool,
        prompt_object_id: Uuid,
        engine_maker: &str,
        engine_key: &str,
        context_data: &Value,
        visible_slots: &[ValueSlotPath],
    ) -> Result<RenderedPrompt, ConduitError> {
        // 1. Fetch prompt messages with appropriate bodies
        let raw_messages = self.fetch_prompt_messages(
            pool,
            prompt_object_id,
            engine_maker,
            engine_key
        ).await?;

        // 2. Build template context from context_data + visible_slots
        let template_context = self.build_template_context(context_data, visible_slots);

        // 3. Render each message body through Handlebars
        let mut messages = Vec::new();
        for raw in raw_messages {
            let rendered_body = self.handlebars
                .render_template(&raw.body, &template_context)
                .map_err(|e| ConduitError::Template(format!(
                    "Failed to render message: {}", e
                )))?;

            messages.push(PromptMessage {
                role: raw.role,
                content: rendered_body,
            });
        }

        Ok(RenderedPrompt {
            messages,
            prompt_name: raw_messages.first()
                .map(|m| m.prompt_name.clone())
                .unwrap_or_default(),
        })
    }

    fn build_template_context(
        &self,
        context_data: &Value,
        visible_slots: &[ValueSlotPath],
    ) -> Map<String, Value> {
        let mut context = Map::new();

        // Add all context_data fields
        if let Value::Object(data) = context_data {
            for (k, v) in data {
                context.insert(k.clone(), v.clone());
            }
        }

        // Add visible slot names as metadata
        let slot_names: Vec<Value> = visible_slots
            .iter()
            .map(|s| Value::String(s.name.clone()))
            .collect();
        context.insert("_visible_slots".to_string(), Value::Array(slot_names));

        context
    }

    async fn fetch_prompt_messages(
        &self,
        pool: &DbPool,
        prompt_object_id: Uuid,
        engine_maker: &str,
        engine_key: &str,
    ) -> Result<Vec<RawPromptMessage>, ConduitError> {
        // Use query: select_prompt_with_fluent_messages.sql
        let rows = sqlx::query_file_as!(
            RawPromptMessage,
            "assets/sql/prompts/select_prompt_with_fluent_messages.sql",
            prompt_object_id,
            engine_maker,
            engine_key
        )
        .fetch_all(pool)
        .await
        .map_err(|e| ConduitError::Database(e.to_string()))?;

        if rows.is_empty() {
            return Err(ConduitError::NotFound(format!(
                "Prompt {} not found or has no messages", prompt_object_id
            )));
        }

        Ok(rows)
    }
}

#[derive(Debug, Clone)]
struct RawPromptMessage {
    prompt_name: String,
    role: Option<String>,
    body: String,
    ordinal: i64,
}
```

---

## Phase 2B: Engine Selection from Playbook Context

The engine (openai/gpt-4, anthropic/claude-3, etc.) is determined by the playbook, not chosen by Conduit. This enables A/B testing.

### Add Engine Config to Playbook Step

This may require a new column or table. Options:

**Option A**: Add to playbook_steps directly
```sql
ALTER TABLE rev.playbook_steps
  ADD COLUMN engine_maker TEXT,
  ADD COLUMN engine_key TEXT;
```

**Option B**: Separate config table (more flexible)
```sql
CREATE TABLE rev.playbook_step_engine_config (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    playbook_step_id BIGINT NOT NULL REFERENCES rev.playbook_steps (id),
    engine_maker TEXT NOT NULL,  -- 'openai', 'anthropic', 'nvidia'
    engine_key TEXT NOT NULL,    -- 'gpt-4', 'claude-3-opus', 'nemotron'
    engine_version TEXT,         -- optional specific version
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (playbook_step_id)
);
```

### Update Conduit Request

The request from Flow to Conduit should NOT include engine - Conduit fetches it:

```rust
pub struct PromptRequest {
    pub prompt_object_id: Uuid,
    pub attempt_id: i64,
    pub playbook_task_id: i64,
    // Engine is resolved from playbook_step, not passed in
}
```

### Engine Resolution Query

```sql
-- select_engine_for_playbook_task.sql
SELECT
    ec.engine_maker,
    ec.engine_key,
    ec.engine_version
FROM rev.playbook_tasks pt
JOIN rev.playbook_steps ps ON ps.id = pt.playbook_step_id
JOIN rev.playbook_step_engine_config ec ON ec.playbook_step_id = ps.id
WHERE pt.id = $1;
```

---

## Phase 2C: LLM Integration Architecture

> **DEFERRED**: This section requires a dedicated plan. The LLM backend integration (swiftide, rig, NIM) involves significant architectural decisions around:
> - Backend trait design
> - RAG context loading
> - Sticky proxy/session management
> - Connection pooling and failover
>
> For Phase 2, we will use a **stub LLM service** that echoes rendered prompts.

### Stub LLM Service (Phase 2 Implementation)

```rust
// In dapple-prompt-conduit/src/service/llm.rs

use crate::error::ConduitError;
use crate::service::template::RenderedPrompt;
use futures::stream::{self, Stream};
use std::pin::Pin;

pub type TokenStream = Pin<Box<dyn Stream<Item = Result<String, ConduitError>> + Send>>;

/// Stub LLM service for Phase 2 testing
/// Returns the rendered prompt content as a streaming response
pub struct LlmService {
    // No real backends yet - just stub behavior
}

impl LlmService {
    pub fn new_stub() -> Self {
        Self {}
    }

    /// Execute a prompt and return streaming tokens (stub implementation)
    /// Echoes the rendered prompt messages as tokens for testing
    pub async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        engine_maker: &str,
        engine_key: &str,
    ) -> Result<TokenStream, ConduitError> {
        // Build a response from the rendered messages
        let response = format!(
            "[STUB: {} / {}]\n{}",
            engine_maker,
            engine_key,
            prompt.messages.iter()
                .map(|m| format!("[{}]: {}", m.role.as_deref().unwrap_or("none"), m.content))
                .collect::<Vec<_>>()
                .join("\n")
        );

        // Stream the response in chunks
        let chunks: Vec<String> = response
            .chars()
            .collect::<Vec<_>>()
            .chunks(20)
            .map(|c| c.iter().collect())
            .collect();

        let stream = stream::iter(chunks.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }
}
```

---

## Phase 2D: Updated Handler Flow (with Stub LLM)

```rust
// In handler/prompt.rs

pub async fn handle_prompt(
    State(state): State<AppState>,
    Json(req): Json<PromptRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ConduitError> {
    let request_id = Uuid::new_v4();
    let start = Instant::now();

    // 1. Resolve engine from playbook step
    let engine = state.engine_service
        .resolve_engine_for_task(req.playbook_task_id)
        .await?;

    // 2. Resolve visible value slots (overlay cascade)
    let visible_slots = state.overlay_service
        .resolve_visible_slots(req.playbook_task_id)
        .await?;

    // 3. Fetch context data from attempt
    let context_data = state.context_service
        .fetch_context_for_attempt(req.attempt_id)
        .await?;

    // 4. Render prompt template
    let rendered = state.template_service
        .render_prompt(
            &state.pool,
            req.prompt_object_id,
            &engine.maker,
            &engine.key,
            &context_data,
            &visible_slots,
        )
        .await?;

    // 5. Execute via LLM service (streaming)
    let token_stream = state.llm_service
        .execute_streaming(&rendered, &engine.maker, &engine.key)
        .await?;

    // 6. Wrap in SSE events
    let stream = async_stream::stream! {
        yield Ok(Event::default().data(
            PromptEvent::started(request_id, &engine.key, req.prompt_object_id).to_sse_data()
        ));

        let mut output_tokens = 0u32;

        pin_mut!(token_stream);
        while let Some(result) = token_stream.next().await {
            match result {
                Ok(token) => {
                    output_tokens += 1;
                    yield Ok(Event::default().data(
                        PromptEvent::token(&token).to_sse_data()
                    ));
                }
                Err(e) => {
                    yield Ok(Event::default().data(
                        PromptEvent::error("stream_error", e.to_string(), true).to_sse_data()
                    ));
                    return;
                }
            }
        }

        let latency_ms = start.elapsed().as_millis() as u64;
        yield Ok(Event::default().data(
            PromptEvent::done(output_tokens, 0, latency_ms, "stop").to_sse_data()
        ));
    };

    Ok(Sse::new(stream))
}
```

---

## Implementation Order

1. **FIRST: Fix local_name deviation**
   - Migration to drop local_name column
   - Update/replace SQL queries
   - Update Rust models
   - Delete old query file

2. **Template Service**
   - Add select_prompt_with_fluent_messages.sql
   - Implement fetch_prompt_messages()
   - Implement render_prompt() with Handlebars

3. **Engine Resolution**
   - Decide on schema (column vs table)
   - Add migration
   - Add query and EngineService

4. **Stub LLM Service**
   - Implement stub that echoes rendered prompts
   - Returns streaming tokens for testing

5. **Update Handler**
   - Wire all services together
   - Test end-to-end with stub LLM

---

## Deferred to Phase 3 (Separate Plan Required)

1. **LLM Backend Architecture**: Real integration with swiftide, rig, NIM
   - Backend trait design
   - Connection pooling and failover
   - Rate limiting and cost tracking

2. **RAG Context Loading**: How do visible value_slots get loaded into LLM context?

3. **Sticky Proxy**: Session stickiness per playbook_step

4. **Response Storage**: Where does Conduit write responses? New table?

5. **Nu Commands**: Integration with dapple-prompt-flow StepEngine

---

## Files to Create/Modify

### New Files
- `migrations/YYYYMMDD_task_prompts_remove_local_name.sql`
- `migrations/YYYYMMDD_playbook_step_engine_config.sql`
- `assets/sql/prompts/select_prompt_with_fluent_messages.sql`
- `assets/sql/playbook_steps/select_engine_for_playbook_task.sql`
- `dapple-prompt-conduit/src/service/engine.rs`
- `dapple-prompt-conduit/src/service/context.rs`

### Modified Files
- `assets/sql/tasks/task_prompts/insert_task_prompts.sql`
- `assets/sql/tasks/task_prompts/select_task_prompts_*.sql`
- `dapple-data/src/model/tasks/task_prompts.rs`
- `dapple-prompt-conduit/src/service/template.rs`
- `dapple-prompt-conduit/src/service/llm.rs` (stub implementation)
- `dapple-prompt-conduit/src/service/mod.rs`
- `dapple-prompt-conduit/src/handler/prompt.rs`

### Deleted Files
- `assets/sql/tasks/task_prompts/select_task_prompt_by_task_id_and_local_name.sql`
