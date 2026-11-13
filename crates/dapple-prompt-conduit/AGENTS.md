# Agent Instructions — dapple-prompt-conduit

Quick reference for agents working with the prompt execution conduit.

> **See also:** [`docs/books/journey-builder-guide/conduit/`](../../docs/books/journey-builder-guide/) for narrative documentation of LLM routing, embedding pipeline, and RAG context injection.

## Architecture Overview

The conduit routes prompt execution and embedding generation to appropriate backends:

```
Prompt Path:  Dispatch → ModelService → ModelConfig → Backend Selection → Execution
                                                        ↑ RAG context injected (optional)
Embed Path:   POST /embed → EmbeddingService → ModernBertBackend → Vec<Vec<f32>>
RAG Path:     RagService → EmbeddingService → pgvector search → formatted context
```

## ModelService

The `ModelService` resolves model configuration from InferenceProfiles:

```rust
pub struct ModelService {
    pool: Arc<DbPool>,
}

impl ModelService {
    /// Resolve model for a playbook task
    pub async fn resolve_model_for_task(
        &self,
        playbook_task_id: Uuid,
    ) -> Result<ModelConfig, ConduitError>;

    /// Resolve model from InferenceProfiles (full implementation)
    pub async fn resolve_model_for_dispatch(
        &self,
        journey_id: Uuid,
        prompt_path_id: Uuid,
    ) -> Result<ModelConfig, ConduitError>;
}
```

## ModelConfig

The resolved configuration for prompt execution:

```rust
pub struct ModelConfig {
    pub lineage_key: String,     // "claude-3-5", "vits", "flan-t5"
    pub provider_key: String,    // "anthropic", "self-hosted"
    pub tier: String,            // "sonnet", "ljspeech"
    pub release: Option<String>, // "20241022", "v0.1"
    pub variant_id: Option<Uuid>,
    pub priority: i32,
    // Sampling parameters...
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<usize>,
    pub max_tokens: Option<usize>,
}
```

## Backend Routing

The prompt handler routes to specialized services based on `lineage_key`:

| Lineage | Handler | Service |
|---------|---------|---------|
| `vits` | `handle_tts_prompt()` | `TtsService` |
| `flan-t5` | `handle_summarization_prompt()` | `SummarizationService` |
| `modernbert` | `handle_mlm_prompt()` | `MlmService` |
| Others | `handle_llm_prompt()` | `LlmService` |

**Note:** ModernBERT serves two distinct roles — `MlmService` for masked language modeling via `/prompt`, and `EmbeddingService` for vector generation via `/embed`. These are separate services sharing the same model architecture.

## LlmBackend Trait

All LLM backends implement this trait:

```rust
#[async_trait]
pub trait LlmBackend: Send + Sync {
    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        model: &ModelConfig,
    ) -> Result<TokenStream, ConduitError>;

    fn supports(&self, model: &ModelConfig) -> bool;
}
```

## Adding a New Backend

1. Create a new module in `src/service/llm/`
2. Implement `LlmBackend` trait
3. Add to `LlmService::new()` backend list
4. Check `supports()` by `lineage_key` or `provider_key`

Example:

```rust
impl LlmBackend for NewBackend {
    fn supports(&self, model: &ModelConfig) -> bool {
        model.provider_key == "new-provider"
            || model.lineage_key == "new-lineage"
    }
}
```

## Release Chain

Template resolution uses a fallback chain:

```
["@provider:lineage/tier:release",  // Most specific
 "@provider:lineage/tier",
 "lineage/tier:release",
 "lineage/tier",
 "lineage"]                         // Least specific
```

Use `ModelConfig::build_chain()` to generate this.

## Common Patterns

### Getting Model Config for a Task

```rust
let model = state.model_service
    .resolve_model_for_task(playbook_task_id)
    .await?;
```

### Checking Backend Support

```rust
for backend in &self.backends {
    if backend.supports(&model) {
        return backend.execute_streaming(&prompt, &model).await;
    }
}
// Fall back to stub
self.fallback.execute_streaming(&prompt, &model).await
```

## Embedding Service

The embedding service generates vector representations of text for similarity search and RAG.

### EmbeddingBackend Trait

```rust
#[async_trait]
pub trait EmbeddingBackend: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ConduitError>;
    fn dimension(&self) -> usize;
    fn name(&self) -> &str;
}
```

### EmbeddingConfig

```rust
pub struct EmbeddingConfig {
    pub model_id: String,         // HuggingFace ID
    pub dimension: usize,         // Default: 768 (Matryoshka truncation)
    pub required: bool,           // Panic on init failure
    pub hf_token: Option<String>, // HuggingFace token for gated models
}
```

`EmbeddingConfig::from_env()` reads `EMBEDDING_ENABLED`, `EMBEDDING_MODEL_ID`, `EMBEDDING_DIMENSION`, `EMBEDDING_REQUIRED`, `HF_TOKEN`. Returns `None` if disabled.

### EmbeddingService

Facade over backends:

```rust
impl EmbeddingService {
    pub async fn with_modernbert(config: &EmbeddingConfig) -> Result<Self, ConduitError>
    pub fn new_stub(dimension: usize) -> Self
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ConduitError>
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>, ConduitError>
    pub fn dimension(&self) -> usize
    pub fn backend_name(&self) -> &str
}
```

### Backends

| Backend | Name | Purpose |
|---------|------|---------|
| `ModernBertBackend` | `modernbert-onnx` | ONNX-based, 768-dim, Matryoshka truncation, lazy-loaded |
| `StubEmbeddingBackend` | `stub` | Deterministic hash-based pseudo-random vectors for testing |

### Endpoint: `POST /embed`

```rust
// Request
pub struct EmbedRequest { pub texts: Vec<String> }

// Response
pub struct EmbedResponse { pub embeddings: Vec<Vec<f32>>, pub dimension: usize }
```

Returns 503 if embedding service not configured, 500 on embedding error.

## RAG Service

Retrieval-Augmented Generation injects relevant guide content into prompt context.

### RagContext

```rust
pub struct RagContext {
    pub guide_ids: Vec<Uuid>,           // Guides to search within
    pub audience_tags: Vec<String>,     // Tag filters (e.g., ["agent", "universal"])
    pub locale: String,                 // Default: "en"
    pub limit: i32,                     // Default: 10
    pub exclude_block_ids: Vec<Uuid>,   // Blocks from prior steps (dedup)
    pub query_hint: Option<String>,     // Override query text
}
```

### RagService

```rust
impl RagService {
    pub fn new(pool: Arc<DbPool>, embedding_service: Arc<EmbeddingService>) -> Self
    pub async fn retrieve(&self, rag_context: &RagContext, prompt_text: &str) -> Result<RagResult, ConduitError>
}

pub struct RagResult {
    pub context_text: String,           // Formatted "# Reference Context" with numbered chunks
    pub retrieved_block_ids: Vec<Uuid>, // For downstream dedup
    pub chunk_count: usize,
}
```

**Pipeline:** query_hint (or prompt_text) → `embed_one()` → pgvector search (with exclusion) → format as `# Reference Context` with similarity percentages.

**Integration:** RAG context is prepended as the first system message during prompt execution. Failures are logged as warnings and never fail the prompt.

## Context Frames (prompt assembly)

Every dispatch-backed `/prompt` execution assembles its system messages as a
**frame stack**, built by successive index-0 inserts in `handler/prompt.rs`:

```
[platform_frame, guide_frame, task_context, rag, ...template messages, user(context_data)]
```

- **`# Platform Frame`** — conduit-owned constant (`assets/platform_frame.md`,
  `include_str!`'d as `PLATFORM_FRAME`). Guide-independent: what a conduit is,
  how its output is used, how to read the other sections. Injected whenever
  `dispatch_id` + `task_id` are present, independent of the task-context fetch
  succeeding. Editing it is a conduit deploy, never a guide re-ingest.
- **`# Guide Frame`** — the pinned guide version's raw CONDUITS.md, arriving
  on `TaskContext.guide_frame` from the api's task_context endpoint; rendered
  by `render_guide_frame` (fixed header + preamble + author markdown). Absent
  ⇒ no message.
- **`# Dispatch Context`** — volatile per-task state (`render_task_context`),
  now including a `## Position` section (journey-pinned stage/step names +
  descriptions + "Stage N/M") when the api resolved them. Its `## Bindings`
  arrive **already blinded** server-side by the overlay cascade — the conduit
  does no overlay filtering on this path.
- **`# Reference Context`** — RAG excerpts with deliberately **neutral
  headers** (`## Excerpt i/N` + type/relevance). Guide/chapter titles are
  audience-voiced copy and must NEVER be re-added to conduit-facing output;
  CONDUITS.md is the sole interpretive key for retrieved prose.

Each section is independent and optional; absent sections drop out of the
order. All rendered messages (frames included) persist to
`rev.prompt_responses.request_messages`.

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `EMBEDDING_ENABLED` | `false` | Enable embedding service |
| `EMBEDDING_MODEL_ID` | `lightonai/modernbert-embed-large` | HuggingFace model ID |
| `EMBEDDING_DIMENSION` | `768` | Vector dimension (Matryoshka truncation) |
| `EMBEDDING_REQUIRED` | `false` | Panic on init failure |
| `HF_TOKEN` | — | HuggingFace token for gated models |
| `CONDUIT_HOST` | `0.0.0.0` | Server bind host |
| `CONDUIT_PORT` | `8081` | Server bind port |

## Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/health_check` | GET | Health check |
| `/prompt` | POST | Execute prompt with optional RAG context injection |
| `/embed` | POST | Generate vector embeddings |
| `/summarize` | POST | Summarization |
| `/synthesize` | POST | TTS synthesis |

## Key Files

| File | Purpose |
|------|---------|
| `src/service/model.rs` | ModelConfig, ModelService |
| `src/service/llm/mod.rs` | LlmBackend trait, LlmService |
| `src/service/llm/anthropic.rs` | Claude backend |
| `src/service/llm/mistral.rs` | Mistral backend |
| `src/service/llm/stub.rs` | Testing stub |
| `src/handler/prompt.rs` | Prompt execution handler |
| `src/handler/embed.rs` | Embedding endpoint handler |
| `src/service/embedding/mod.rs` | EmbeddingBackend trait, EmbeddingConfig, EmbeddingService |
| `src/service/embedding/modernbert.rs` | ModernBERT ONNX backend |
| `src/service/embedding/stub.rs` | Stub embedding backend |
| `src/service/rag.rs` | RagContext, RagService, RagResult, format_rag_context |
