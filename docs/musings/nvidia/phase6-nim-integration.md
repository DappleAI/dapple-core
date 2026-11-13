# Phase 6: NVIDIA NIM Integration

This document covers integrating NVIDIA NIM (Inference Microservices) as an LLM backend for dapple-prompt-conduit.

## What is NIM?

[NVIDIA NIM](https://developer.nvidia.com/nim) provides optimized inference microservices for deploying AI models:

- **Docker containers** with pre-packaged models (Llama, Mistral, Nemotron, etc.)
- **OpenAI-compatible API** - same endpoints as OpenAI (`/v1/chat/completions`)
- **Self-hosted** - runs on your NVIDIA GPU infrastructure
- **Optimized** - TensorRT-LLM, vLLM, or SGLang backends for high throughput

## Why NIM?

| Benefit | Description |
|---------|-------------|
| **Data privacy** | Models run on your infrastructure, data never leaves |
| **Cost control** | No per-token API costs after hardware investment |
| **Latency** | Local inference, no network round-trip to cloud |
| **Model choice** | Run open models (Llama 3, Mistral, etc.) or fine-tuned models |
| **OpenAI compatible** | Minimal code changes - same API shape |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    dapple-prompt-conduit                     │
├─────────────────────────────────────────────────────────────┤
│  LlmService                                                  │
│  ├── AnthropicBackend (maker_key: "anthropic")              │
│  ├── NimBackend (maker_key: "nim")        ◄── NEW           │
│  ├── OpenAiBackend (maker_key: "openai")  ◄── Future        │
│  └── StubBackend (maker_key: "stub")                        │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      NIM Container                           │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  meta/llama-3.1-8b-instruct                          │   │
│  │  POST http://nim-host:8000/v1/chat/completions       │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Phase 6 Overview

| Component | Description |
|-----------|-------------|
| **6A** | NIM backend implementation (OpenAI-compatible client) |
| **6B** | Streaming support via SSE |
| **6C** | Model routing and configuration |
| **6D** | Health checks and failover |

---

## Phase 6A: NIM Backend Implementation

Since NIM uses OpenAI-compatible API, we can use a generic OpenAI client.

### Add Dependencies

Update `dapple-prompt-conduit/Cargo.toml`:

```toml
[dependencies]
# ... existing deps ...

# OpenAI-compatible client (works with NIM)
async-openai = "0.27"
```

### Create NIM Backend

Create `dapple-prompt-conduit/src/service/llm/nim.rs`:

```rust
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestUserMessage, ChatCompletionRequestAssistantMessage,
        CreateChatCompletionRequest, CreateChatCompletionStreamResponse,
    },
};
use async_trait::async_trait;
use futures::stream::{self, StreamExt};

use super::{LlmBackend, TokenStream};
use crate::error::ConduitError;
use crate::service::engine::EngineConfig;
use crate::service::template::RenderedPrompt;

/// NIM backend using OpenAI-compatible API
pub struct NimBackend {
    base_url: String,
    api_key: Option<String>,
}

impl NimBackend {
    pub fn new(base_url: String, api_key: Option<String>) -> Self {
        Self { base_url, api_key }
    }

    fn create_client(&self) -> Client<OpenAIConfig> {
        let mut config = OpenAIConfig::new()
            .with_api_base(&self.base_url);

        // NIM may or may not require an API key depending on deployment
        if let Some(key) = &self.api_key {
            config = config.with_api_key(key);
        } else {
            // Some OpenAI clients require a key even if NIM doesn't check it
            config = config.with_api_key("nim-local");
        }

        Client::with_config(config)
    }

    fn build_messages(&self, prompt: &RenderedPrompt) -> Vec<ChatCompletionRequestMessage> {
        prompt.messages.iter().map(|m| {
            let role = m.role.as_deref().unwrap_or("user");
            match role {
                "system" => ChatCompletionRequestMessage::System(
                    ChatCompletionRequestSystemMessage {
                        content: m.content.clone().into(),
                        name: None,
                    }
                ),
                "assistant" => ChatCompletionRequestMessage::Assistant(
                    ChatCompletionRequestAssistantMessage {
                        content: Some(m.content.clone()),
                        name: None,
                        tool_calls: None,
                        refusal: None,
                        ..Default::default()
                    }
                ),
                _ => ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessage {
                        content: m.content.clone().into(),
                        name: None,
                    }
                ),
            }
        }).collect()
    }
}

#[async_trait]
impl LlmBackend for NimBackend {
    fn supports(&self, engine: &EngineConfig) -> bool {
        engine.maker_key == "nim"
    }

    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        engine: &EngineConfig,
    ) -> Result<TokenStream, ConduitError> {
        let client = self.create_client();
        let messages = self.build_messages(prompt);

        log::info!(
            "Executing NIM completion: model={}, prompt={}, base_url={}",
            engine.engine_key,
            prompt.prompt_name,
            self.base_url
        );

        let request = CreateChatCompletionRequest {
            model: engine.engine_key.clone(),
            messages,
            stream: Some(true),
            max_tokens: Some(4096),
            ..Default::default()
        };

        // Create streaming request
        let mut stream = client
            .chat()
            .create_stream(request)
            .await
            .map_err(|e| ConduitError::LlmError(format!("NIM request failed: {}", e)))?;

        // Transform to our TokenStream
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        tokio::spawn(async move {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(response) => {
                        for choice in response.choices {
                            if let Some(content) = choice.delta.content {
                                if tx.send(Ok(content)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(ConduitError::LlmError(e.to_string()))).await;
                        break;
                    }
                }
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports() {
        let backend = NimBackend::new("http://localhost:8000".to_string(), None);

        let nim_engine = EngineConfig {
            maker_key: "nim".to_string(),
            engine_key: "meta/llama-3.1-8b-instruct".to_string(),
            engine_version: None,
        };

        let anthropic_engine = EngineConfig {
            maker_key: "anthropic".to_string(),
            engine_key: "claude-3-sonnet".to_string(),
            engine_version: None,
        };

        assert!(backend.supports(&nim_engine));
        assert!(!backend.supports(&anthropic_engine));
    }
}
```

---

## Phase 6B: Streaming Support

NIM's OpenAI-compatible API supports SSE streaming out of the box. The `async-openai` crate handles this via `create_stream()`.

The stream returns `CreateChatCompletionStreamResponse` with:
- `choices[].delta.content` - The token text
- `choices[].finish_reason` - "stop" when complete

This maps directly to our existing `PromptEvent::Token` and `PromptEvent::Done`.

---

## Phase 6C: Model Routing and Configuration

### Database: llm_engines Table

NIM models are registered in the existing `llm_engines` table:

```sql
INSERT INTO rev.llm_engines (maker_key, engine_key, engine_version, name) VALUES
    ('nim', 'meta/llama-3.1-8b-instruct', NULL, 'Llama 3.1 8B Instruct (NIM)'),
    ('nim', 'meta/llama-3.1-70b-instruct', NULL, 'Llama 3.1 70B Instruct (NIM)'),
    ('nim', 'mistralai/mistral-7b-instruct-v0.3', NULL, 'Mistral 7B Instruct (NIM)'),
    ('nim', 'nvidia/nemotron-4-340b-instruct', NULL, 'Nemotron 4 340B (NIM)');
```

### Configuration

```bash
# NIM endpoint (required if using NIM)
NIM_BASE_URL=http://nim-host:8000/v1

# Optional API key (depends on NIM deployment)
NIM_API_KEY=

# Multiple NIM endpoints for different models (optional)
NIM_LLAMA_URL=http://llama-nim:8000/v1
NIM_MISTRAL_URL=http://mistral-nim:8000/v1
```

### LlmConfig Update

```rust
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub anthropic_api_key: Option<String>,
    pub nim_base_url: Option<String>,
    pub nim_api_key: Option<String>,
    // Future
    pub openai_api_key: Option<String>,
}

impl LlmConfig {
    pub fn from_env() -> Self {
        Self {
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            nim_base_url: std::env::var("NIM_BASE_URL").ok(),
            nim_api_key: std::env::var("NIM_API_KEY").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
        }
    }
}
```

### LlmService Update

```rust
impl LlmService {
    pub fn new(config: &LlmConfig) -> Self {
        let mut backends: Vec<Box<dyn LlmBackend>> = Vec::new();

        // Add Anthropic backend
        if let Some(api_key) = &config.anthropic_api_key {
            backends.push(Box::new(AnthropicBackend::new(api_key.clone())));
        }

        // Add NIM backend
        if let Some(base_url) = &config.nim_base_url {
            backends.push(Box::new(NimBackend::new(
                base_url.clone(),
                config.nim_api_key.clone(),
            )));
        }

        // Stub is always the fallback
        let fallback = Box::new(StubBackend::new());

        Self { backends, fallback }
    }
}
```

---

## Phase 6D: Health Checks and Failover

### Health Check Endpoint

NIM exposes `/v1/health/ready` for readiness checks:

```rust
impl NimBackend {
    pub async fn health_check(&self) -> Result<bool, ConduitError> {
        let url = format!("{}/health/ready", self.base_url.trim_end_matches("/v1"));

        let response = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| ConduitError::LlmError(format!("NIM health check failed: {}", e)))?;

        Ok(response.status().is_success())
    }
}
```

### Failover Strategy

For production, consider:

1. **Circuit breaker** - Don't retry if NIM is down (from Phase 5 deferred)
2. **Fallback to cloud** - If NIM unavailable, fall back to Anthropic
3. **Multiple NIM instances** - Load balance across NIMs

```rust
// Example: Fallback chain
async fn execute_with_fallback(
    &self,
    prompt: &RenderedPrompt,
    engine: &EngineConfig,
) -> Result<TokenStream, ConduitError> {
    // Try NIM first
    if let Some(nim) = &self.nim_backend {
        if nim.health_check().await.unwrap_or(false) {
            return nim.execute_streaming(prompt, engine).await;
        }
        log::warn!("NIM unhealthy, falling back to cloud");
    }

    // Fallback to Anthropic
    if let Some(anthropic) = &self.anthropic_backend {
        return anthropic.execute_streaming(prompt, engine).await;
    }

    Err(ConduitError::NoBackendConfigured {
        maker_key: engine.maker_key.clone(),
        engine_key: engine.engine_key.clone(),
    })
}
```

---

## Implementation Order

1. **Phase 6A: NIM Backend**
   - Add `async-openai` dependency
   - Implement `NimBackend` with OpenAI-compatible client
   - Add to `LlmService` backend list

2. **Phase 6B: Streaming** (included in 6A)
   - Streaming works via `create_stream()`
   - Map delta.content to TokenStream

3. **Phase 6C: Configuration**
   - Add `NIM_BASE_URL`, `NIM_API_KEY` env vars
   - Update `LlmConfig`
   - Seed `llm_engines` with NIM models

4. **Phase 6D: Health Checks**
   - Add health check method
   - Optional: failover logic

---

## Files to Create/Modify

### New Files
- `dapple-prompt-conduit/src/service/llm/nim.rs`

### Modified Files
- `dapple-prompt-conduit/Cargo.toml` (add async-openai)
- `dapple-prompt-conduit/src/service/llm/mod.rs` (export NimBackend, add to LlmService)
- `dapple-prompt-conduit/src/config.rs` (add NIM config)
- `dapple-data/migrations/seed_llm_engines.sql` (add NIM models)

---

## Testing

### Local NIM Setup

```bash
# Pull and run NIM container (requires NGC API key)
docker run -d --gpus all \
  -e NGC_API_KEY=$NGC_API_KEY \
  -p 8000:8000 \
  nvcr.io/nim/meta/llama-3.1-8b-instruct:latest
```

### Integration Test

```rust
#[tokio::test]
#[ignore] // Requires running NIM instance
async fn test_nim_streaming() {
    let backend = NimBackend::new(
        "http://localhost:8000/v1".to_string(),
        None,
    );

    let prompt = RenderedPrompt {
        prompt_name: "test".to_string(),
        messages: vec![PromptMessage {
            role: Some("user".to_string()),
            content: "Say 'hello' and nothing else.".to_string(),
        }],
    };

    let engine = EngineConfig {
        maker_key: "nim".to_string(),
        engine_key: "meta/llama-3.1-8b-instruct".to_string(),
        engine_version: None,
    };

    let mut stream = backend.execute_streaming(&prompt, &engine).await.unwrap();
    let mut response = String::new();

    while let Some(result) = stream.next().await {
        response.push_str(&result.unwrap());
    }

    assert!(response.to_lowercase().contains("hello"));
}
```

---

## Future: NeMo Integration (Hand-wavy Vision)

NIM deploys models. But what if you want **custom models**?

That's where [NeMo](https://docs.nvidia.com/nemo-framework/user-guide/latest/overview.html) comes in:

1. **Fine-tune** a base model (Llama, Mistral) on your domain data using NeMo
2. **Export** the fine-tuned model to TensorRT-LLM format
3. **Deploy** via NIM with LoRA adapters or full model replacement
4. **Call** from dapple-prompt-conduit using the same NIM backend

### Example Flow

```
Your Data → NeMo Fine-tuning → LoRA Adapter → NIM + Adapter → Conduit
                                    ↓
                            "nim/custom-model"
```

This would be a separate initiative involving:
- Data pipeline for training data preparation (NeMo Curator)
- Training infrastructure (DGX or cloud GPUs)
- Model versioning and deployment workflow
- Integration with dapple's asset system for training data

**Not in scope for Phase 6** - but the NIM backend we're building now will support these custom models when they exist.

---

## References

- [NVIDIA NIM Documentation](https://docs.nvidia.com/nim/large-language-models/latest/introduction.html)
- [NIM Getting Started](https://docs.nvidia.com/nim/large-language-models/latest/getting-started.html)
- [async-openai crate](https://crates.io/crates/async-openai)
- [NeMo Framework](https://docs.nvidia.com/nemo-framework/user-guide/latest/overview.html)
