# Nushell Environment Setup - Phase 3: rig-core + Anthropic Integration

This document continues from Phase 2 and addresses the first real LLM backend integration using rig-core with Anthropic Claude.

## Prerequisites

- Phase 2 implementation complete (stub LLM service working)
- `ANTHROPIC_API_KEY` environment variable available
- Understanding of rig-core's CompletionModel trait

---

## Phase 3A: Fix context_data Deviation (PRIORITY 1)

### Problem

The current `PromptRequest` in `handler/prompt.rs` includes `context_data: serde_json::Value` passed directly from the caller. This deviates from the original design where Conduit should fetch context from the `attempt_id`.

**Current (incorrect):**
```rust
pub struct PromptRequest {
    pub prompt_object_id: Uuid,
    pub attempt_id: i64,
    pub playbook_task_id: i64,
    pub context_data: serde_json::Value,  // ❌ Should not be passed in
}
```

**Expected (correct):**
```rust
pub struct PromptRequest {
    pub prompt_object_id: Uuid,
    pub attempt_id: i64,
    pub playbook_task_id: i64,
    // ✓ context_data fetched internally via ContextService
}
```

### Required Changes

#### 1. Add ContextService

Create `dapple-prompt-conduit/src/service/context.rs`:

```rust
use dapple_data::DbPool;
use serde_json::Value;
use uuid::Uuid;

use crate::error::ConduitError;

/// Context data from a dispatch task attempt
#[derive(Debug, Clone)]
pub struct AttemptContext {
    pub context_data: Value,
    pub context_metadata: Value,
    pub task_id: i64,
    pub task_object_id: Uuid,
}

pub struct ContextService {
    pool: DbPool,
}

impl ContextService {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Fetch context data for a dispatch task attempt
    pub async fn fetch_context_for_attempt(
        &self,
        attempt_id: i64,
    ) -> Result<AttemptContext, ConduitError> {
        let row = sqlx::query_file_as!(
            AttemptContextRow,
            "assets/sql/dispatches/dispatch_task_attempts/select_context_by_id.sql",
            attempt_id
        )
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|e| ConduitError::Database(e.to_string()))?
        .ok_or_else(|| ConduitError::NotFound(format!(
            "Attempt {} not found", attempt_id
        )))?;

        Ok(AttemptContext {
            context_data: row.context_data,
            context_metadata: row.context_metadata,
            task_id: row.task_id,
            task_object_id: row.task_object_id,
        })
    }
}

#[derive(Debug)]
struct AttemptContextRow {
    context_data: Value,
    context_metadata: Value,
    task_id: i64,
    task_object_id: Uuid,
}
```

#### 2. Add SQL Query

Create `assets/sql/dispatches/dispatch_task_attempts/select_context_by_id.sql`:

```sql
-- Fetch context data for a dispatch task attempt
-- Parameters:
--   $1: attempt_id (BIGINT) - internal attempt ID

SELECT
    dsta.context_data,
    dsta.context_metadata,
    dsta.task_id,
    t.object_id AS task_object_id
FROM rev.dispatch_step_task_attempts dsta
JOIN rev.tasks t ON t.id = dsta.task_id
WHERE dsta.id = $1;
```

#### 3. Update PromptRequest

In `handler/prompt.rs`, remove `context_data` field:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct PromptRequest {
    pub prompt_object_id: Uuid,
    pub attempt_id: i64,
    pub playbook_task_id: i64,
    // context_data removed - fetched via ContextService
}
```

#### 4. Update Handler

In `handle_prompt()`, fetch context via ContextService:

```rust
// Before step 3 (resolve visible slots), add:

// 2.5. Fetch context data from attempt
let attempt_context = match state.context_service
    .fetch_context_for_attempt(req.attempt_id)
    .await
{
    Ok(ctx) => {
        log::debug!(
            "Fetched context for attempt {}: task={}",
            req.attempt_id,
            ctx.task_object_id
        );
        ctx
    }
    Err(e) => {
        let error = PromptEvent::error("context_error", e.to_string(), false);
        yield Ok(Event::default().data(error.to_sse_data()));
        return;
    }
};

// Then use attempt_context.context_data in template rendering:
let rendered = match state.template_service.render_prompt(
    state.pool.as_ref(),
    req.prompt_object_id,
    &engine.maker_key,
    &engine.engine_key,
    engine.engine_version.as_deref(),
    &attempt_context.context_data,  // ✓ From ContextService
    &visible_slots,
).await { ... }
```

#### 5. Update AppState

In `service/mod.rs`, add ContextService:

```rust
pub struct AppState {
    pub pool: Arc<DbPool>,
    pub template_service: TemplateService,
    pub overlay_service: OverlayService,
    pub engine_service: EngineService,
    pub llm_service: LlmService,
    pub context_service: ContextService,  // Add this
}
```

---

## Phase 3B: rig-core Integration Architecture

### Overview

The LlmService will use a trait-based approach to support multiple backends:
- **Stub**: For testing (current implementation)
- **Anthropic via rig-core**: First real backend
- **Future**: OpenAI, NIM, custom backends

### Add Dependencies

Update `dapple-prompt-conduit/Cargo.toml`:

```toml
[dependencies]
# ... existing deps ...

# LLM backend - rig-core with anthropic feature
rig-core = { version = "0.23", features = ["anthropic"] }
```

### LLM Backend Trait

Define a common trait in `service/llm/mod.rs`:

```rust
use async_trait::async_trait;
use crate::error::ConduitError;
use crate::service::engine::EngineConfig;
use crate::service::template::RenderedPrompt;
use futures::stream::Stream;
use std::pin::Pin;

pub type TokenStream = Pin<Box<dyn Stream<Item = Result<String, ConduitError>> + Send>>;

/// Trait for LLM backends
#[async_trait]
pub trait LlmBackend: Send + Sync {
    /// Execute a rendered prompt and return streaming tokens
    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        engine: &EngineConfig,
    ) -> Result<TokenStream, ConduitError>;

    /// Check if this backend can handle the given engine config
    fn supports(&self, engine: &EngineConfig) -> bool;
}
```

### Backend Selection Logic

The `LlmService` routes to the appropriate backend based on `engine.maker_key`:

```rust
pub struct LlmService {
    backends: Vec<Box<dyn LlmBackend>>,
    fallback: Box<dyn LlmBackend>,
}

impl LlmService {
    pub fn new(config: &LlmConfig) -> Self {
        let mut backends: Vec<Box<dyn LlmBackend>> = Vec::new();

        // Add Anthropic backend if API key is available
        if let Some(api_key) = &config.anthropic_api_key {
            backends.push(Box::new(AnthropicBackend::new(api_key.clone())));
        }

        // Stub is always the fallback
        let fallback = Box::new(StubBackend::new());

        Self { backends, fallback }
    }

    pub async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        engine: &EngineConfig,
    ) -> Result<TokenStream, ConduitError> {
        // Find a backend that supports this engine
        for backend in &self.backends {
            if backend.supports(engine) {
                return backend.execute_streaming(prompt, engine).await;
            }
        }

        // Fall back to stub (useful for testing or when maker_key = "stub")
        log::warn!(
            "No backend found for engine {} / {}, using stub",
            engine.maker_key,
            engine.engine_key
        );
        self.fallback.execute_streaming(prompt, engine).await
    }
}
```

---

## Phase 3C: Anthropic Backend Implementation

### File Structure

```
dapple-prompt-conduit/src/service/llm/
├── mod.rs           # LlmBackend trait, LlmService
├── stub.rs          # StubBackend (existing, refactored)
└── anthropic.rs     # AnthropicBackend (new)
```

### AnthropicBackend Implementation

Create `service/llm/anthropic.rs`:

```rust
use async_trait::async_trait;
use futures::stream::{self, Stream, StreamExt};
use rig::providers::anthropic::{self, ClientBuilder};
use rig::completion::{Prompt, ToolDefinition};
use std::pin::Pin;

use crate::error::ConduitError;
use crate::service::engine::EngineConfig;
use crate::service::template::RenderedPrompt;
use super::{LlmBackend, TokenStream};

/// Anthropic backend using rig-core
pub struct AnthropicBackend {
    api_key: String,
}

impl AnthropicBackend {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    fn create_client(&self) -> anthropic::Client {
        ClientBuilder::new(&self.api_key)
            .anthropic_version("2023-06-01")
            .build()
    }

    fn map_model(&self, engine_key: &str) -> &str {
        // Map our engine_key to rig's model constants
        match engine_key {
            "claude-3-opus" | "claude-3-opus-20240229" => anthropic::CLAUDE_3_OPUS,
            "claude-3-sonnet" | "claude-3-sonnet-20240229" => anthropic::CLAUDE_3_SONNET,
            "claude-3-haiku" | "claude-3-haiku-20240307" => anthropic::CLAUDE_3_HAIKU,
            "claude-3.5-sonnet" | "claude-3-5-sonnet-20241022" => anthropic::CLAUDE_3_5_SONNET,
            "claude-3.5-haiku" | "claude-3-5-haiku-20241022" => anthropic::CLAUDE_3_5_HAIKU,
            // Default to latest sonnet for unknown keys
            _ => {
                log::warn!("Unknown Anthropic model key '{}', defaulting to Claude 3.5 Sonnet", engine_key);
                anthropic::CLAUDE_3_5_SONNET
            }
        }
    }

    fn build_prompt_text(&self, prompt: &RenderedPrompt) -> String {
        // For simple completion, combine messages into a single prompt
        // System messages go first, then user/assistant alternating
        let mut parts = Vec::new();

        for msg in &prompt.messages {
            let role = msg.role.as_deref().unwrap_or("user");
            match role {
                "system" => parts.insert(0, msg.content.clone()),
                "user" | "human" => parts.push(format!("Human: {}", msg.content)),
                "assistant" => parts.push(format!("Assistant: {}", msg.content)),
                _ => parts.push(msg.content.clone()),
            }
        }

        parts.push("Assistant:".to_string());
        parts.join("\n\n")
    }
}

#[async_trait]
impl LlmBackend for AnthropicBackend {
    fn supports(&self, engine: &EngineConfig) -> bool {
        engine.maker_key == "anthropic"
    }

    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        engine: &EngineConfig,
    ) -> Result<TokenStream, ConduitError> {
        let client = self.create_client();
        let model_id = self.map_model(&engine.engine_key);

        log::info!(
            "Executing Anthropic completion: model={}, prompt={}",
            model_id,
            prompt.prompt_name
        );

        // Create the completion model with max_tokens (required for Anthropic)
        let model = client.completion_model(model_id);

        // Build the prompt text from rendered messages
        let prompt_text = self.build_prompt_text(prompt);

        // Execute the completion
        // Note: rig-core's streaming API returns a stream of completion chunks
        let response = model
            .prompt(&prompt_text)
            .await
            .map_err(|e| ConduitError::LlmError(format!("Anthropic completion failed: {}", e)))?;

        // For now, we split the response into chunks to simulate streaming
        // TODO: Use rig's actual streaming API when available for messages
        let chunks: Vec<String> = response
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)  // ~1 token per chunk (rough approximation)
            .map(|c| c.iter().collect())
            .collect();

        let stream = stream::iter(chunks.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }
}
```

### Token-Level Streaming (Advanced)

For true token-level streaming, we need to use rig's streaming completion API. This requires the Messages API:

```rust
// Advanced streaming implementation (when rig supports it)
async fn execute_streaming_messages(
    &self,
    prompt: &RenderedPrompt,
    engine: &EngineConfig,
) -> Result<TokenStream, ConduitError> {
    let client = self.create_client();
    let model_id = self.map_model(&engine.engine_key);

    // Build messages for the Messages API
    let messages: Vec<anthropic::Message> = prompt.messages.iter()
        .map(|m| {
            let role = match m.role.as_deref() {
                Some("system") => anthropic::Role::System,
                Some("assistant") => anthropic::Role::Assistant,
                _ => anthropic::Role::User,
            };
            anthropic::Message {
                role,
                content: m.content.clone(),
            }
        })
        .collect();

    // Use rig's streaming API
    let stream = client
        .agent(model_id)
        .max_tokens(4096)
        .stream_prompt(&messages)
        .await
        .map_err(|e| ConduitError::LlmError(format!("Anthropic stream failed: {}", e)))?;

    // Transform the stream to our TokenStream type
    let token_stream = stream.map(|result| {
        result
            .map(|chunk| chunk.delta.text.unwrap_or_default())
            .map_err(|e| ConduitError::LlmError(e.to_string()))
    });

    Ok(Box::pin(token_stream))
}
```

> **Note**: The exact streaming API depends on rig-core's version. Check the latest rig documentation for the correct streaming method signatures.

---

## Phase 3D: Configuration

### Environment Variables

```bash
# Required for Anthropic backend
ANTHROPIC_API_KEY=sk-ant-api03-...

# Optional: Default model if not specified in playbook_step_engine_configs
ANTHROPIC_DEFAULT_MODEL=claude-3-5-sonnet-20241022

# Optional: For future backends
OPENAI_API_KEY=sk-...
HUGGINGFACE_API_KEY=hf_...
```

### LlmConfig Struct

In `config.rs`:

```rust
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub anthropic_api_key: Option<String>,
    pub anthropic_default_model: Option<String>,
    // Future backends
    pub openai_api_key: Option<String>,
    pub huggingface_api_key: Option<String>,
}

impl LlmConfig {
    pub fn from_env() -> Self {
        Self {
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            anthropic_default_model: std::env::var("ANTHROPIC_DEFAULT_MODEL").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            huggingface_api_key: std::env::var("HUGGINGFACE_API_KEY").ok(),
        }
    }

    pub fn has_any_backend(&self) -> bool {
        self.anthropic_api_key.is_some() || self.openai_api_key.is_some()
    }
}
```

### Update Main

In `main.rs`:

```rust
use crate::config::LlmConfig;

#[tokio::main]
async fn main() {
    // ... existing setup ...

    let llm_config = LlmConfig::from_env();

    if !llm_config.has_any_backend() {
        log::warn!(
            "No LLM API keys configured. Only stub backend will be available. \
             Set ANTHROPIC_API_KEY or OPENAI_API_KEY environment variables."
        );
    }

    let llm_service = LlmService::new(&llm_config);

    // ... rest of setup ...
}
```

---

## Phase 3E: Error Handling

Add LLM-specific error variants to `error.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConduitError {
    // ... existing variants ...

    #[error("LLM backend error: {0}")]
    LlmError(String),

    #[error("LLM rate limited: retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("LLM context too long: {tokens} tokens exceeds limit of {limit}")]
    ContextTooLong { tokens: u32, limit: u32 },

    #[error("No LLM backend configured for engine {maker_key}/{engine_key}")]
    NoBackendConfigured { maker_key: String, engine_key: String },
}
```

Update `PromptEvent::error` to include `retryable` hint:

```rust
// In handler, when handling rate limits:
Err(ConduitError::RateLimited { retry_after_ms }) => {
    let error = PromptEvent::error(
        "rate_limited",
        format!("Rate limited, retry after {}ms", retry_after_ms),
        true,  // retryable
    );
    yield Ok(Event::default().data(error.to_sse_data()));
    return;
}
```

---

## Phase 3F: Testing

### Unit Tests

```rust
// In service/llm/anthropic.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_mapping() {
        let backend = AnthropicBackend::new("test-key".to_string());

        assert_eq!(backend.map_model("claude-3-opus"), anthropic::CLAUDE_3_OPUS);
        assert_eq!(backend.map_model("claude-3.5-sonnet"), anthropic::CLAUDE_3_5_SONNET);
        assert_eq!(backend.map_model("unknown"), anthropic::CLAUDE_3_5_SONNET); // fallback
    }

    #[test]
    fn test_supports() {
        let backend = AnthropicBackend::new("test-key".to_string());

        let anthropic_engine = EngineConfig {
            maker_key: "anthropic".to_string(),
            engine_key: "claude-3-sonnet".to_string(),
            engine_version: None,
        };

        let openai_engine = EngineConfig {
            maker_key: "openai".to_string(),
            engine_key: "gpt-4".to_string(),
            engine_version: None,
        };

        assert!(backend.supports(&anthropic_engine));
        assert!(!backend.supports(&openai_engine));
    }
}
```

### Integration Test

```rust
// In tests/anthropic_integration.rs
#[tokio::test]
#[ignore] // Run manually with: cargo test anthropic_integration -- --ignored
async fn test_anthropic_streaming() {
    // Requires ANTHROPIC_API_KEY to be set
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY must be set for integration tests");

    let backend = AnthropicBackend::new(api_key);

    let prompt = RenderedPrompt {
        prompt_name: "test-prompt".to_string(),
        messages: vec![
            PromptMessage {
                role: Some("user".to_string()),
                content: "Say 'hello' and nothing else.".to_string(),
            }
        ],
    };

    let engine = EngineConfig {
        maker_key: "anthropic".to_string(),
        engine_key: "claude-3-haiku".to_string(),  // Use cheapest model for tests
        engine_version: None,
    };

    let mut stream = backend.execute_streaming(&prompt, &engine).await.unwrap();
    let mut response = String::new();

    while let Some(result) = stream.next().await {
        let token = result.unwrap();
        response.push_str(&token);
    }

    assert!(response.to_lowercase().contains("hello"));
}
```

---

## Implementation Order

1. **Phase 3A: Fix context_data deviation**
   - Add `select_context_by_id.sql`
   - Create `ContextService`
   - Update `PromptRequest` to remove `context_data`
   - Update handler to use `ContextService`
   - Update `AppState`

2. **Phase 3B: LLM trait refactoring**
   - Create `LlmBackend` trait
   - Move existing stub to `StubBackend`
   - Create `LlmService` with backend routing

3. **Phase 3C: Anthropic backend**
   - Add rig-core dependency
   - Implement `AnthropicBackend`
   - Add model key mapping
   - Add streaming (chunk-based initially)

4. **Phase 3D: Configuration**
   - Add `LlmConfig` struct
   - Update main.rs to load from env
   - Add warning when no backends configured

5. **Phase 3E: Error handling**
   - Add LLM error variants
   - Handle rate limits
   - Handle context length errors

6. **Phase 3F: Testing**
   - Unit tests for model mapping
   - Integration test (manual, requires API key)

---

## Files to Create/Modify

### New Files
- `assets/sql/dispatches/dispatch_task_attempts/select_context_by_id.sql`
- `dapple-prompt-conduit/src/service/context.rs`
- `dapple-prompt-conduit/src/service/llm/mod.rs`
- `dapple-prompt-conduit/src/service/llm/stub.rs`
- `dapple-prompt-conduit/src/service/llm/anthropic.rs`

### Modified Files
- `dapple-prompt-conduit/Cargo.toml` (add rig-core)
- `dapple-prompt-conduit/src/config.rs` (add LlmConfig)
- `dapple-prompt-conduit/src/error.rs` (add LLM errors)
- `dapple-prompt-conduit/src/handler/prompt.rs` (remove context_data, use ContextService)
- `dapple-prompt-conduit/src/service/mod.rs` (add ContextService, update LlmService)
- `dapple-prompt-conduit/src/main.rs` (load LlmConfig)

### Deleted Files
- `dapple-prompt-conduit/src/service/llm.rs` (moved to llm/mod.rs and llm/stub.rs)

---

## Deferred to Phase 4

1. **True token-level streaming** - Requires deeper rig-core Messages API integration
2. **OpenAI backend** - Similar pattern to Anthropic
3. **NIM backend** - Custom implementation for NVIDIA NIM
4. **HuggingFace/Candle** - Local model support
5. **Rate limiting and retry logic** - Exponential backoff, circuit breaker
6. **Cost tracking** - Token usage aggregation per journey/dispatch
7. **Response storage** - Persist LLM responses to database
8. **Nu commands** - `prompt send`, `prompt retry`, `prompt reset` in dapple-prompt-flow

---

## References

- [rig-core on crates.io](https://crates.io/crates/rig-core)
- [rig documentation](https://docs.rs/rig-core/latest/rig/)
- [Anthropic API docs](https://docs.anthropic.com/en/api/messages)
- [rig Anthropic integration](https://docs.rig.rs/docs/integrations/model_providers/anthropic)
