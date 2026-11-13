# Nushell Environment Setup - Phase 4: End-to-End Prompt Flow

This document continues from Phase 3 and focuses on completing the end-to-end prompt execution flow from Nu commands through conduit to LLM responses.

## Prerequisites

- Phase 3 implementation complete (Anthropic backend working)
- Nu commands stubbed (`prompt send`, `prompt retry`, `prompt reset`)
- ContextService fetching context from dispatch_task_attempts
- SSE streaming infrastructure in place

---

## Phase 4 Overview

| Component | Description |
|-----------|-------------|
| **4A** | Wire Nu Commands to conduit HTTP service |
| **4C** | Response Storage - persist LLM responses to database |
| **4D** | Prompt Resolution - look up prompt by object_id for internal tracking |

---

## Phase 4A: Wire Nu Commands to Conduit

### Current State

The Nu commands in `dapple-prompt-flow/src/commands/` are stubbed:
- `prompt send` - Returns mock response
- `prompt retry` - Returns mock response
- `prompt reset` - Returns mock response

They read `DAPPLE_METADATA` from environment but don't actually call conduit.

### Required Changes

#### 1. Add HTTP Client Dependency

Update `dapple-prompt-flow/Cargo.toml`:

```toml
[dependencies]
# ... existing deps ...
reqwest = { version = "0.12", features = ["json", "stream"] }
tokio-stream = "0.1"
```

#### 2. Create Conduit Client

Add `dapple-prompt-flow/src/conduit_client.rs`:

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Request to execute a prompt via conduit
#[derive(Debug, Serialize)]
pub struct PromptRequest {
    pub prompt_object_id: Uuid,
    pub attempt_id: i64,
    pub playbook_task_id: i64,
}

/// SSE event from conduit
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptEvent {
    Started {
        request_id: Uuid,
        model: String,
        prompt_object_id: Uuid,
    },
    Token {
        content: String,
    },
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

/// Client for calling conduit service
pub struct ConduitClient {
    client: Client,
    base_url: String,
}

impl ConduitClient {
    pub fn new(base_url: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
        }
    }

    /// Execute a prompt and return a channel of events
    pub async fn execute_prompt(
        &self,
        request: PromptRequest,
    ) -> Result<mpsc::Receiver<PromptEvent>, ConduitError> {
        let url = format!("{}/prompt", self.base_url);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| ConduitError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ConduitError::Http { status: status.as_u16(), body });
        }

        // Parse SSE stream
        let (tx, rx) = mpsc::channel(100);
        let stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut buffer = String::new();
            // SSE parsing logic...
            // For each complete event, parse and send to channel
        });

        Ok(rx)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConduitError {
    #[error("network error: {0}")]
    Network(String),
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("parse error: {0}")]
    Parse(String),
}
```

#### 3. Update prompt_send Command

In `commands/prompt_send.rs`, replace stub with real HTTP call:

```rust
fn run(
    &self,
    engine_state: &EngineState,
    stack: &mut Stack,
    call: &Call,
    input: PipelineData,
) -> Result<PipelineData, ShellError> {
    let span = call.head;
    let name: String = call.req(engine_state, stack, 0)?;

    // Get metadata from environment
    let metadata = get_dapple_metadata(engine_state, stack, span)?;

    // Get conduit URL
    let conduit_url = metadata.conduit_url
        .or_else(|| std::env::var("CONDUIT_URL").ok())
        .unwrap_or_else(|| "http://localhost:8081".to_string());

    // Look up prompt_object_id from task_prompts by name
    // This requires a new query or the metadata should include prompt mappings
    let prompt_object_id = resolve_prompt_by_name(&name, &metadata)?;

    // Create conduit client and execute
    let client = ConduitClient::new(conduit_url);
    let request = PromptRequest {
        prompt_object_id,
        attempt_id: metadata.attempt_id,
        playbook_task_id: metadata.playbook_task_id,
    };

    // Block on async execution (we're in sync Nu command context)
    let runtime = tokio::runtime::Handle::current();
    let result = runtime.block_on(async {
        let mut rx = client.execute_prompt(request).await?;
        let mut response = String::new();

        while let Some(event) = rx.recv().await {
            match event {
                PromptEvent::Token { content } => response.push_str(&content),
                PromptEvent::Error { message, .. } => {
                    return Err(ConduitError::Parse(message));
                }
                _ => {}
            }
        }

        Ok(response)
    });

    match result {
        Ok(response) => Ok(PipelineData::Value(Value::string(response, span), None)),
        Err(e) => Err(ShellError::GenericError {
            error: "Prompt execution failed".into(),
            msg: e.to_string(),
            span: Some(span),
            help: None,
            inner: vec![],
        }),
    }
}
```

#### 4. Prompt Name Resolution

The `prompt send "summarize"` command needs to resolve the local name to a `prompt_object_id`.

**Option A: Include in DAPPLE_METADATA** (Recommended)

Extend metadata to include prompt mappings:

```rust
#[derive(Debug, Deserialize, Serialize)]
struct DappleMetadata {
    attempt_id: i64,
    playbook_task_id: i64,
    conduit_url: Option<String>,
    // Map of local_name -> prompt_object_id
    prompt_mappings: HashMap<String, Uuid>,
}
```

This is set by the StepEngine when launching the Nu script.

**Option B: Query Conduit**

Add a `/prompt/resolve` endpoint to conduit that looks up by task_id + name:

```
GET /prompt/resolve?task_id={playbook_task_id}&name={local_name}
-> { prompt_object_id: Uuid }
```

Option A is simpler and avoids an extra HTTP round-trip.

#### 5. Update StepEngine to Populate Metadata

In `dapple-prompt-flow/src/engine/mod.rs`, when launching a Nu script:

```rust
fn build_dapple_metadata(
    &self,
    attempt_id: i64,
    playbook_task_id: i64,
    task_prompts: &[TaskPromptMapping],
) -> serde_json::Value {
    let prompt_mappings: HashMap<String, Uuid> = task_prompts
        .iter()
        .map(|tp| (tp.prompt_name.clone(), tp.prompt_object_id))
        .collect();

    serde_json::json!({
        "attempt_id": attempt_id,
        "playbook_task_id": playbook_task_id,
        "conduit_url": self.conduit_url.clone(),
        "prompt_mappings": prompt_mappings
    })
}
```

---

## Phase 4C: Response Storage

### Overview

Persist LLM responses to the database for:
- Audit trail
- Debugging
- Future cost tracking and caching

### Database Schema

#### Migration: `20260119000001_prompt_responses.sql`

```sql
-- Stores LLM responses for audit and tracking
CREATE TABLE rev.prompt_responses (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id           UUID NOT NULL DEFAULT gen_random_uuid(),

    -- Request context
    attempt_id          BIGINT NOT NULL REFERENCES rev.dispatch_task_attempts,
    prompt_id           BIGINT NOT NULL REFERENCES rev.prompts,
    playbook_task_id    BIGINT NOT NULL REFERENCES rev.playbook_tasks,

    -- Engine info
    maker_key           TEXT NOT NULL,
    engine_key          TEXT NOT NULL,
    engine_version      TEXT,

    -- Request/Response
    request_messages    JSONB NOT NULL,  -- Rendered messages sent to LLM
    response_content    TEXT NOT NULL,   -- Full response text

    -- Metrics
    input_tokens        INTEGER NOT NULL DEFAULT 0,
    output_tokens       INTEGER NOT NULL DEFAULT 0,
    latency_ms          INTEGER NOT NULL DEFAULT 0,
    finish_reason       TEXT NOT NULL DEFAULT 'stop',

    -- Timestamps
    started_at          TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at        TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT prompt_responses_object_id_unique UNIQUE (object_id)
);

ALTER TABLE rev.prompt_responses OWNER TO dapple;

CREATE INDEX prompt_responses_attempt_id_idx ON rev.prompt_responses (attempt_id);
CREATE INDEX prompt_responses_prompt_id_idx ON rev.prompt_responses (prompt_id);
CREATE INDEX prompt_responses_started_at_idx ON rev.prompt_responses (started_at DESC);
```

### Insert Query

`assets/sql/prompt_responses/insert_prompt_response.sql`:

```sql
INSERT INTO rev.prompt_responses (
    attempt_id,
    prompt_id,
    playbook_task_id,
    maker_key,
    engine_key,
    engine_version,
    request_messages,
    response_content,
    input_tokens,
    output_tokens,
    latency_ms,
    finish_reason,
    started_at,
    completed_at
)
VALUES (
    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14
)
RETURNING id, object_id;
```

### Update Handler

In `handler/prompt.rs`, after streaming completes:

```rust
// After collecting all tokens...
let response_content = collected_tokens.join("");

// Store response
let prompt_response = InsertPromptResponseArgs {
    attempt_id: req.attempt_id,
    prompt_id: resolved.prompt_id,  // From Phase 4D
    playbook_task_id: req.playbook_task_id,
    maker_key: engine.maker_key.clone(),
    engine_key: engine.engine_key.clone(),
    engine_version: engine.engine_version.clone(),
    request_messages: serde_json::to_value(&rendered.messages).unwrap(),
    response_content: response_content.clone(),
    input_tokens,
    output_tokens,
    latency_ms,
    finish_reason: "stop".to_string(),
    started_at,
    completed_at: Utc::now(),
};

// Fire and forget - don't block the SSE stream
tokio::spawn(async move {
    if let Err(e) = insert_prompt_response(&pool, prompt_response).await {
        log::error!("Failed to store prompt response: {}", e);
    }
});
```

---

## Phase 4D: Prompt Resolution in Conduit

### Current Gap

The handler receives `prompt_object_id` but needs `prompt_id` (internal) for:
- Selecting prompt messages
- Storing response (Phase 4C)

### Add Resolution Query

`assets/sql/prompts/select_prompt_id_by_object_id.sql`:

```sql
SELECT p.id, p.path_id, pp.name as prompt_name
FROM rev.prompts p
JOIN rev.prompt_paths pp ON pp.id = p.path_id
WHERE p.object_id = $1
ORDER BY p.version_major DESC, p.version_minor DESC, p.version_patch DESC
LIMIT 1;
```

### Update TemplateService

Add method to resolve prompt and return both rendered messages and internal ID:

```rust
pub struct ResolvedPrompt {
    pub prompt_id: i64,
    pub prompt_name: String,
    pub messages: Vec<PromptMessage>,
}

impl TemplateService {
    pub async fn resolve_and_render_prompt(
        &self,
        pool: &PgPool,
        prompt_object_id: Uuid,
        maker_key: &str,
        engine_key: &str,
        engine_version: Option<&str>,
        context_data: &serde_json::Value,
        visible_slots: &[VisibleValueSlot],
    ) -> Result<ResolvedPrompt, ConduitError> {
        // Get prompt internal ID and name
        let prompt_info = select_prompt_by_object_id(pool, prompt_object_id).await?;

        // Render messages
        let messages = self.render_messages(
            pool,
            prompt_info.id,
            maker_key,
            engine_key,
            engine_version,
            context_data,
            visible_slots,
        ).await?;

        Ok(ResolvedPrompt {
            prompt_id: prompt_info.id,
            prompt_name: prompt_info.prompt_name,
            messages,
        })
    }
}
```

---

## Implementation Order

1. **Phase 4D: Prompt Resolution** (dependency for 4C)
   - Add `select_prompt_id_by_object_id` query
   - Update TemplateService with `resolve_and_render_prompt`

2. **Phase 4C: Response Storage**
   - Create migration for `prompt_responses` table
   - Add insert query and model
   - Update handler to store responses

3. **Phase 4A: Wire Nu Commands** (completes end-to-end flow)
   - Add reqwest to dapple-prompt-flow
   - Create ConduitClient with SSE parsing
   - Update `prompt_send` to use real HTTP
   - Update `prompt_retry` and `prompt_reset`
   - Update StepEngine to populate `prompt_mappings` in metadata

---

## Files to Create/Modify

### New Files
- `dapple-prompt-flow/src/conduit_client.rs`
- `dapple-data/migrations/20260119000001_prompt_responses.sql`
- `dapple-data/assets/sql/prompt_responses/insert_prompt_response.sql`
- `dapple-data/assets/sql/prompts/select_prompt_id_by_object_id.sql`
- `dapple-data/src/model/prompt_responses.rs`
- `dapple-data/src/query/prompt_responses.rs`

### Modified Files
- `dapple-prompt-flow/Cargo.toml` (add reqwest)
- `dapple-prompt-flow/src/commands/prompt_send.rs` (real HTTP)
- `dapple-prompt-flow/src/commands/prompt_retry.rs` (real HTTP)
- `dapple-prompt-flow/src/commands/prompt_reset.rs` (real HTTP)
- `dapple-prompt-flow/src/engine/mod.rs` (populate metadata)
- `dapple-prompt-conduit/src/service/template.rs` (resolve_and_render)
- `dapple-prompt-conduit/src/handler/prompt.rs` (store response)

---

## Deferred to Phase 5

1. **True Token-Level Streaming** - Use rig Messages API for real-time tokens
2. **Cost Tracking** - Token usage aggregation per attempt/journey
3. **Rate Limiting** - Exponential backoff and circuit breaker
4. **OpenAI Backend** - Similar pattern to Anthropic
5. **NIM Backend** - NVIDIA NIM integration for local/edge models
6. **HuggingFace/Candle** - Local model support with Candle
7. **Response Caching** - Cache identical prompts for cost savings
8. **Streaming to Client** - Real-time token streaming in Nu commands
9. **Prompt Versioning** - Track which prompt version was used
10. **A/B Testing** - Route to different models for comparison

---

## Testing Checklist

### Unit Tests
- [ ] ConduitClient SSE parsing
- [ ] Prompt name resolution from metadata
- [ ] Prompt object_id to internal id resolution

### Integration Tests
- [ ] End-to-end: Nu command → Conduit → Anthropic → Response
- [ ] Response storage verification
- [ ] Verify prompt_responses table after execution

### Manual Tests
- [ ] `prompt send "summarize"` with real Anthropic key
- [ ] Verify SSE events in browser/curl
- [ ] Check prompt_responses table after execution

---

## Environment Variables

```bash
# Existing
DATABASE_URL=postgresql://...
ANTHROPIC_API_KEY=sk-ant-...

# New for Phase 4
CONDUIT_URL=http://localhost:8081  # Default for Nu commands
```

---

## Notes from Phase 3 Review

These items were noted during Phase 3 review:

1. **rig-core feature flag**: Using `derive` feature is correct for 0.28 - anthropic is included in base crate

2. **API pattern difference**: rig-core 0.28 uses `Client::builder().api_key().build()` - this is the correct modern API

3. **LlmConfig location**: Keeping in `service/llm/mod.rs` is fine - co-locates LLM config with LLM code

4. **Nu commands ahead of schedule**: Phase 3 agent added stub implementations - this phase wires them to real conduit

5. **Chunk-based streaming**: Current implementation splits response into 4-char chunks - deferred to Phase 5 for true token streaming
