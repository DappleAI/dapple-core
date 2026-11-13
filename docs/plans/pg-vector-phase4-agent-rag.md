# pgvector Phase 4: Agent RAG Pipeline

**Status:** Draft
**Created:** 2026-02-24
**Last Updated:** 2026-02-24
**Parent Plan:** `docs/plans/pg-vector-searching.md` (sections 4b, 4c, 5)
**Dependencies:** Phase 2 (embedding pipeline), Phase 3 (search SQL + `/embed` endpoint)
**Target Version:** N/A (incremental)

---

## Executive Summary

Phase 2 delivered the write path (embedding generation on guide ingest). Phase 3 delivered the human search read path (search endpoint in the API, embedding via Conduit). Phase 4 delivers the agent RAG pipeline: when agents execute tasks within the dispatch pipeline, Conduit enriches their prompt context with relevant guide content retrieved from pgvector.

The key design decision in this plan is **rejecting Lance in favor of a simpler pgvector-only approach**. The original plan proposed an in-process Lance vector store to accumulate context across dispatch steps. After investigating the actual codebase, Lance adds disproportionate complexity for Phase 4:

1. Lance is not a current dependency and would introduce a significant native library dependency
2. The accumulated context set per dispatch is small (10--100 chunks) -- brute-force deduplication via a `HashSet<Uuid>` is equally fast
3. Lance's sub-millisecond vector search value only matters at scale; pgvector already provides vector search and the data is already there
4. Conduit's `AppState` is immutable and cloned per-request -- holding mutable per-dispatch state requires a `DashMap` or similar, which adds lifecycle management complexity that can be deferred

Instead, Phase 4 uses a **stateless per-request approach**: Flow passes RAG context fields to Conduit with each prompt request, Conduit queries pgvector directly, and injects retrieved chunks into the prompt. No dispatch-scoped state is held in Conduit. Cross-step context accumulation is handled by Flow's existing `accumulated_data` mechanism, which already persists across steps and dispatches within a journey.

This approach can be upgraded to Lance later (Phase 5) if the accumulated context set grows large enough to warrant in-process vector search.

---

## 1. Current Prompt Execution Flow

Understanding the current pipeline is essential for knowing where RAG injection fits.

### 1a. Flow: dispatch_runner.rs (`execute_dispatch`)

The primary execution path is `execute_dispatch()` (not `run_dispatch_from_dto` which uses pre-built workflows). For each task:

1. Look up the `Task` entity from the database to get `function` and `function_input`
2. Build a single-workflow `Step` with a Nushell function handler
3. Insert a `dispatch_task_attempt` row (status = "running")
4. Build `prompt_data` by querying `task_prompts` and `prompts` for the task
5. Set `DAPPLE_PROMPT_CONDUIT_URL` and `prompt_data` into message metadata
6. Merge `accumulated_data` + `stage.params` + `step_dto.params` into `message.data`
7. Execute via the dataflow engine (Nushell script runs)
8. Accumulate output into `accumulated_data` for subsequent tasks

### 1b. Nushell: `prompt send` command

When a Nushell script calls `prompt send "name"`:

1. Read `DAPPLE_METADATA` from the Nushell environment (set by Flow)
2. Look up the prompt by name in `prompt_data`
3. Build a `PromptRequest { prompt_id, attempt_id, playbook_task_id, context_data }`
4. POST to Conduit's `/prompt` endpoint
5. Stream SSE response, collect tokens

### 1c. Conduit: `handle_prompt`

When Conduit receives a prompt request:

1. Resolve `ModelConfig` from `playbook_task_id` via `ModelService`
2. Route by `lineage_key` (vits -> TTS, flan-t5 -> summarization, etc.)
3. For LLM prompts:
   a. Fetch `AttemptContext` from the database (context_data for the attempt)
   b. Resolve visible value slots via `OverlayService`
   c. Render prompt template via `TemplateService` (locale/engine fallback, $variable substitution)
   d. Inject `req.context_data` as a user message (if present)
   e. Execute via `LlmService` (streaming)
   f. Store response in `rev.prompt_responses`

### 1d. Key Observation: context_data Injection Point

The prompt handler already injects `req.context_data` as a user message (lines 170-185 of prompt.rs). RAG context can use the same pattern -- but injected as a **system message** rather than a user message, placed before the user's actual request.

---

## 2. Architectural Decision: No Lance, No Dispatch-Scoped State

### Why Not Lance

| Factor | Lance | pgvector-only |
|--------|-------|---------------|
| **New dependency** | `lancedb` crate + Arrow ecosystem | None (pgvector already in use) |
| **Context set size** | Designed for millions of vectors | 10-100 chunks per dispatch |
| **Search latency** | Sub-millisecond in-process | 5-20ms pgvector (already proven) |
| **State management** | Requires `DashMap<Uuid, LanceDb>` in AppState | Stateless |
| **Lifecycle** | Create/drop per dispatch, cleanup on failure | No lifecycle to manage |
| **Deduplication** | Lance handles via schema | `HashSet<Uuid>` in Flow |
| **Cross-step accumulation** | Lance accumulates vectors | Flow's `accumulated_data` tracks seen block IDs |
| **Testing complexity** | Need to mock Lance or use temp dirs | Just SQL queries |

### Recommended: Stateless Per-Request pgvector Query

Each prompt request includes enough context for Conduit to query pgvector and inject relevant chunks. No state persists in Conduit between requests.

**Deduplication across steps** is handled by Flow: Flow tracks which `block_id`s have already been retrieved (in `accumulated_data`) and passes them to Conduit as `exclude_block_ids`. This is equivalent to Lance's dedup-by-`source_block_id` but without the in-process store.

**Upgrade path to Lance**: If future dispatches involve hundreds of steps each retrieving dozens of chunks, the accumulated context set could grow large enough to warrant in-process search. At that point, add Lance as a Phase 5 enhancement -- the `RagContext` struct and Conduit integration point remain the same; only the retrieval backend changes from pgvector to Lance.

---

## 3. New Types

### 3a. RagContext (passed from Flow to Conduit)

```rust
// In dapple-prompt-conduit/src/handler/prompt.rs (or a shared types crate)
// Added as an optional field on the existing PromptRequest

/// RAG context for enriching agent prompts with guide content.
/// Assembled by Flow from dispatch context, passed per-request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RagContext {
    /// Guide IDs to search within. Derived from the playbook's source guide.
    /// Empty means no RAG retrieval.
    #[serde(default)]
    pub guide_ids: Vec<Uuid>,

    /// Audience tags to filter by (e.g., ["agent", "universal"]).
    /// Derived from the task's tags or stage/step params.
    #[serde(default)]
    pub audience_tags: Vec<String>,

    /// Locale for the current execution (e.g., "en", "pt-br").
    #[serde(default = "default_locale")]
    pub locale: String,

    /// Maximum number of chunks to retrieve.
    #[serde(default = "default_rag_limit")]
    pub limit: i32,

    /// Block IDs already retrieved in prior steps of this dispatch.
    /// Conduit excludes these from results to avoid redundant context.
    #[serde(default)]
    pub exclude_block_ids: Vec<Uuid>,

    /// Optional semantic query override. If not set, Conduit uses
    /// the rendered prompt text as the query embedding source.
    #[serde(default)]
    pub query_hint: Option<String>,
}

fn default_locale() -> String { "en".to_string() }
fn default_rag_limit() -> i32 { 10 }
```

### 3b. Extended PromptRequest

```rust
// Existing PromptRequest with new optional field
#[derive(Debug, Deserialize, Serialize)]
pub struct PromptRequest {
    pub prompt_id: Uuid,
    pub attempt_id: Uuid,
    pub playbook_task_id: Uuid,
    #[serde(default)]
    pub context_data: Option<Value>,

    /// RAG context for guide content retrieval.
    /// When present and non-empty guide_ids, Conduit will query pgvector
    /// and inject retrieved chunks as system context.
    #[serde(default)]
    pub rag_context: Option<RagContext>,
}
```

### 3c. RagResult (internal to Conduit)

```rust
// Internal struct representing retrieved RAG chunks ready for injection
pub struct RagResult {
    /// Formatted text block ready to inject into the prompt
    pub context_text: String,
    /// Block IDs that were retrieved (for Flow to accumulate)
    pub retrieved_block_ids: Vec<Uuid>,
    /// Number of chunks retrieved
    pub chunk_count: usize,
}
```

---

## 4. RagService (new service in Conduit)

### 4a. Location

`crates/dapple-prompt-conduit/src/service/rag.rs`

### 4b. Responsibilities

1. Accept a `RagContext` and the rendered prompt text
2. Generate a query embedding (via `EmbeddingService`)
3. Query pgvector using `search_guide_content_embeddings_by_guide_ids`
4. Filter out `exclude_block_ids`
5. Format retrieved chunks into a context string
6. Return `RagResult`

### 4c. Design

```rust
pub struct RagService {
    pool: Arc<DbPool>,
    embedding_service: Arc<EmbeddingService>,
}

impl RagService {
    pub fn new(pool: Arc<DbPool>, embedding_service: Arc<EmbeddingService>) -> Self {
        Self { pool, embedding_service }
    }

    /// Retrieve and format RAG context for prompt injection.
    ///
    /// Returns None if rag_context is empty or has no guide_ids.
    /// Returns an empty RagResult (with context_text = "") if no
    /// relevant chunks are found -- this is not an error.
    pub async fn retrieve(
        &self,
        rag_context: &RagContext,
        prompt_text: &str,
    ) -> Result<RagResult, ConduitError> {
        if rag_context.guide_ids.is_empty() {
            return Ok(RagResult {
                context_text: String::new(),
                retrieved_block_ids: vec![],
                chunk_count: 0,
            });
        }

        // 1. Build query text: prefer query_hint, fall back to prompt text
        let query_text = rag_context.query_hint
            .as_deref()
            .unwrap_or(prompt_text);

        // 2. Generate query embedding
        let query_embedding = self.embedding_service
            .embed_one(query_text)
            .await?;

        // 3. Query pgvector
        let results = search_guide_content_embeddings_by_guide_ids(
            &self.pool,
            &query_embedding,
            &rag_context.guide_ids,
            &rag_context.audience_tags,
            &rag_context.locale,
            rag_context.limit,
        ).await
        .map_err(ConduitError::Database)?;

        // 4. Filter out already-seen blocks
        let exclude_set: HashSet<Uuid> = rag_context.exclude_block_ids
            .iter()
            .copied()
            .collect();

        let filtered: Vec<_> = results
            .into_iter()
            .filter(|r| !exclude_set.contains(&r.block_id))
            .collect();

        // 5. Format into context text
        let retrieved_block_ids: Vec<Uuid> = filtered
            .iter()
            .map(|r| r.block_id)
            .collect();

        let context_text = format_rag_context(&filtered);

        Ok(RagResult {
            context_text,
            retrieved_block_ids,
            chunk_count: filtered.len(),
        })
    }
}

/// Format search results into a system-level context block.
///
/// The format is designed to be clear to LLMs without wasting tokens
/// on elaborate markup. Each chunk is separated by a blank line with
/// its source metadata as a header comment.
fn format_rag_context(results: &[GuideEmbeddingSearchResult]) -> String {
    if results.is_empty() {
        return String::new();
    }

    let mut parts = Vec::with_capacity(results.len() + 1);
    parts.push("# Reference Context\n\nThe following excerpts from the guide are relevant to this task:\n".to_string());

    for (i, result) in results.iter().enumerate() {
        let guide_title = result.metadata
            .get("guide_title")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown Guide");
        let chapter_title = result.metadata
            .get("chapter_title")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let directive_type = &result.directive_type_key;

        let header = if chapter_title.is_empty() {
            format!("## [{}/{}] {}", i + 1, results.len(), guide_title)
        } else {
            format!("## [{}/{}] {} > {}", i + 1, results.len(), guide_title, chapter_title)
        };

        parts.push(format!(
            "{}\n*Type: {} | Relevance: {:.0}%*\n\n{}",
            header,
            directive_type,
            result.similarity * 100.0,
            result.chunk_text,
        ));
    }

    parts.join("\n\n")
}
```

### 4d. Registration in AppState

```rust
// In service/mod.rs AppState
pub struct AppState {
    // ... existing fields ...
    pub rag_service: Option<Arc<RagService>>,
}
```

The `RagService` is only created when both `embedding_service` and `pool` are available. It reuses the existing `EmbeddingService` instance.

---

## 5. Prompt Handler Changes (Conduit)

### 5a. RAG Injection Point

In `handle_prompt` (the LLM path), RAG retrieval happens **after** template rendering but **before** LLM execution. The retrieved context is prepended as a system message.

```rust
// Inside the SSE stream in handle_prompt, after step 4 (template rendering)
// and before step 5 (LLM execution):

// 4.5. RAG context retrieval (if requested)
if let Some(ref rag_ctx) = req.rag_context {
    if let Some(ref rag_service) = state.rag_service {
        // Use the first system message content as semantic query
        let prompt_text = rendered.messages
            .first()
            .map(|m| m.content.as_str())
            .unwrap_or("");

        match rag_service.retrieve(rag_ctx, prompt_text).await {
            Ok(rag_result) if !rag_result.context_text.is_empty() => {
                log::info!(
                    "RAG: injected {} chunks for dispatch",
                    rag_result.chunk_count
                );

                // Prepend RAG context as first system message
                rendered.messages.insert(0, PromptMessage {
                    role: Some("system".to_string()),
                    content: rag_result.context_text,
                });

                // Store retrieved block_ids in response metadata
                // (Flow can read these from prompt_response to accumulate)
            }
            Ok(_) => {
                log::debug!("RAG: no relevant chunks found");
            }
            Err(e) => {
                // RAG failure is non-fatal -- log and continue without RAG context
                log::warn!("RAG retrieval failed, continuing without: {}", e);
            }
        }
    }
}
```

### 5b. Graceful Degradation

RAG failure MUST NOT fail the prompt execution. If:
- `rag_context` is `None` -> no RAG (backward compatible)
- `rag_context.guide_ids` is empty -> no RAG
- `rag_service` is `None` (embedding service not configured) -> no RAG
- pgvector query fails -> log warning, continue without RAG
- Embedding generation fails -> log warning, continue without RAG

This ensures that agents still function even when the embedding service or pgvector data is unavailable.

### 5c. Non-LLM Routes

TTS, summarization, and MLM routes do NOT use RAG. The `rag_context` field is silently ignored for those lineages. This is enforced by the early-return routing in `handle_prompt` (the lineage-based dispatch happens before the RAG injection point).

---

## 6. Flow Changes (dispatch_runner.rs)

### 6a. Building RagContext

Flow assembles `RagContext` from data already available in `execute_dispatch()`. No new database queries are required.

```rust
// In execute_dispatch(), when building the metadata for each task:

// Build RAG context from dispatch/stage/step context
let rag_context = build_rag_context(
    &dispatch,
    stage,
    step_dto,
    task_entity,
    &accumulated_data,
);

let metadata = json!({
    "conduit_url": conduit_url,
    "prompt_data": prompt_data,
    "attempt_id": attempt_id,
    "playbook_task_id": task.playbook_task_id,
    "rag_context": rag_context,  // NEW
});
```

### 6b. `build_rag_context` Function

```rust
/// Build RAG context from dispatch execution state.
///
/// Returns None if the task or guide context does not warrant RAG retrieval.
fn build_rag_context(
    dispatch: &Dispatch,
    stage: &DispatchStage,
    step: &DispatchStep,
    task_entity: &Task,
    accumulated_data: &serde_json::Map<String, Value>,
) -> Option<serde_json::Value> {
    // 1. Extract guide_ids from task function_input or stage params
    //    The playbook's source guide ID is typically stored in function_input
    //    or in stage params as "guide_id" or "guide_ids"
    let guide_ids = extract_guide_ids(
        &task_entity.function_input,
        &stage.params,
        &step.params,
    );

    if guide_ids.is_empty() {
        return None;
    }

    // 2. Audience tags: default to ["agent", "universal"]
    //    Can be overridden in stage/step params as "rag_audience_tags"
    let audience_tags = extract_audience_tags(&stage.params, &step.params);

    // 3. Locale: from stage/step params or default to "en"
    let locale = extract_locale(&stage.params, &step.params);

    // 4. Exclude block IDs: accumulated from prior RAG retrievals
    //    Stored in accumulated_data under "_rag_seen_block_ids"
    let exclude_block_ids: Vec<Uuid> = accumulated_data
        .get("_rag_seen_block_ids")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // 5. Query hint: from task function_input "rag_query_hint" field
    let query_hint = task_entity.function_input
        .get("rag_query_hint")
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(json!({
        "guide_ids": guide_ids,
        "audience_tags": audience_tags,
        "locale": locale,
        "limit": 10,
        "exclude_block_ids": exclude_block_ids,
        "query_hint": query_hint,
    }))
}

fn extract_guide_ids(
    function_input: &Value,
    stage_params: &Value,
    step_params: &Value,
) -> Vec<Uuid> {
    // Check in order: function_input > step_params > stage_params
    for source in [function_input, step_params, stage_params] {
        if let Some(ids) = source.get("rag_guide_ids").and_then(|v| v.as_array()) {
            let parsed: Vec<Uuid> = ids.iter()
                .filter_map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                .collect();
            if !parsed.is_empty() {
                return parsed;
            }
        }
        if let Some(id_str) = source.get("guide_id").and_then(|v| v.as_str()) {
            if let Ok(id) = Uuid::parse_str(id_str) {
                return vec![id];
            }
        }
    }
    vec![]
}

fn extract_audience_tags(stage_params: &Value, step_params: &Value) -> Vec<String> {
    for source in [step_params, stage_params] {
        if let Some(tags) = source.get("rag_audience_tags").and_then(|v| v.as_array()) {
            let parsed: Vec<String> = tags.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }
    vec!["agent".to_string(), "universal".to_string()]
}

fn extract_locale(stage_params: &Value, step_params: &Value) -> String {
    for source in [step_params, stage_params] {
        if let Some(locale) = source.get("rag_locale").and_then(|v| v.as_str()) {
            return locale.to_string();
        }
    }
    "en".to_string()
}
```

### 6c. Accumulating Retrieved Block IDs

After each task execution, Flow reads back the retrieved block IDs and merges them into `accumulated_data`:

```rust
// After successful task execution in execute_dispatch():
// (This is a future enhancement -- initially, duplicate chunks are
// filtered server-side via exclude_block_ids which Flow populates
// from the prompt_response or from task output signals.)

// For Phase 4, the simplest approach: Nushell scripts that use
// `prompt send` do not need to do anything special. The exclude_block_ids
// starts empty and can be populated by guide authors in their
// stage/step params if they know which blocks are shared across steps.
//
// Full automatic accumulation (reading back from Conduit response)
// is deferred to Phase 4b or Phase 5.
```

### 6d. Passing RagContext to Nushell

The `RagContext` is passed through the `DAPPLE_METADATA` environment variable that Nushell scripts already read. The `prompt send` command needs a small change to forward it to Conduit:

```rust
// In conduit_client.rs PromptRequest (Flow-side client):
pub struct PromptRequest {
    pub prompt_id: Uuid,
    pub attempt_id: Uuid,
    pub playbook_task_id: Uuid,
    pub context_data: Option<serde_json::Value>,
    pub rag_context: Option<serde_json::Value>,  // NEW
}

// In prompt_send.rs DappleMetadata:
pub struct DappleMetadata {
    // ... existing fields ...
    #[serde(default)]
    pub rag_context: Option<serde_json::Value>,  // NEW
}

// In prompt_send.rs execute_prompt_with_context:
// Forward rag_context from metadata to the PromptRequest
let request = PromptRequest {
    prompt_id: prompt_info.id,
    attempt_id: metadata.attempt_id,
    playbook_task_id: metadata.playbook_task_id,
    context_data,
    rag_context: metadata.rag_context.clone(),  // NEW
};
```

---

## 7. SQL Query Enhancement

### 7a. Excluding Block IDs

The existing `search_guide_content_embeddings_by_guide_ids.sql` query does not support excluding specific block IDs. Phase 4 needs a variant that does.

**File:** `assets/sql/guide_content_embeddings/search_guide_content_embeddings_by_guide_ids_excluding.sql`

```sql
-- Similarity search scoped to specific guides, excluding already-seen blocks.
-- $1: query embedding (vector(768))
-- $2: guide_ids (UUID[])
-- $3: tag keys (TEXT[])
-- $4: locale tag (TEXT)
-- $5: limit (INT)
-- $6: exclude_block_ids (UUID[]) -- blocks already retrieved in prior steps
SELECT
    e.id,
    e.guide_id,
    e.chapter_id,
    e.block_id,
    dt.key AS directive_type_key,
    e.chunk_text,
    e.metadata,
    1 - (e.embedding <=> $1::vector(768)) AS similarity
FROM rev.guide_content_embeddings e
JOIN rev.guide_block_directive_types dt ON dt.id = e.directive_type_id
JOIN rev.guide_embedding_tags et ON et.embedding_id = e.id
JOIN rev.guide_embedding_locales el ON el.embedding_id = e.id
JOIN rev.guide_tags gt ON gt.id = et.tag_id
JOIN rev.fluent_message_locales fml ON fml.id = el.locale_id
WHERE e.guide_id = ANY($2::UUID[])
AND gt.key = ANY($3::TEXT[])
AND fml.tag = $4
AND (CARDINALITY($6::UUID[]) = 0 OR e.block_id != ALL($6::UUID[]))
ORDER BY e.embedding <=> $1::vector(768)
LIMIT $5;
```

### 7b. Corresponding Query Function

```rust
// In dapple-data/src/query/guide_content_embeddings.rs

/// Search guide content embeddings scoped to specific guides,
/// excluding blocks already retrieved in prior steps.
pub async fn search_guide_content_embeddings_by_guide_ids_excluding(
    pool: &DbPool,
    query_embedding: &[f32],
    guide_ids: &[Uuid],
    tag_keys: &[String],
    locale_tag: &str,
    limit: i32,
    exclude_block_ids: &[Uuid],
) -> Result<Vec<GuideEmbeddingSearchResult>, DataError> {
    let vector = Vector::from(query_embedding.to_vec());

    let rows = sqlx::query_as::<_, GuideEmbeddingSearchResult>(include_str!(
        "../../assets/sql/guide_content_embeddings/search_guide_content_embeddings_by_guide_ids_excluding.sql"
    ))
    .bind(&vector)
    .bind(guide_ids)
    .bind(tag_keys)
    .bind(locale_tag)
    .bind(limit)
    .bind(exclude_block_ids)
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}
```

---

## 8. Implementation Guidance

### 8a. Module Organization

**dapple-prompt-conduit** (Conduit service):
- `src/service/rag.rs` -- New `RagService` with `retrieve()` and `format_rag_context()`
- `src/service/mod.rs` -- Add `pub mod rag;`, add `rag_service: Option<Arc<RagService>>` to `AppState`
- `src/handler/prompt.rs` -- Add `rag_context: Option<RagContext>` to `PromptRequest`, add RAG injection between template rendering and LLM execution

**dapple-prompt-flow** (Flow engine):
- `src/dispatch_runner.rs` -- Add `build_rag_context()` and helpers, include `rag_context` in message metadata
- `src/conduit_client.rs` -- Add `rag_context: Option<serde_json::Value>` to `PromptRequest`
- `src/commands/prompt_send.rs` -- Add `rag_context` to `DappleMetadata`, forward to `PromptRequest`

**dapple-data** (data layer):
- `assets/sql/guide_content_embeddings/search_guide_content_embeddings_by_guide_ids_excluding.sql` -- New SQL file
- `src/query/guide_content_embeddings.rs` -- Add `search_guide_content_embeddings_by_guide_ids_excluding()`

### 8b. Key Implementation Considerations

**Backward Compatibility**: The `rag_context` field is `Option` with `#[serde(default)]` on `PromptRequest`. Existing callers that omit it will see `None`, and no RAG retrieval occurs. This is fully backward compatible.

**Performance**: RAG retrieval adds one embedding generation (~10ms for ModernBERT) and one pgvector query (~5-20ms) per prompt request. This is negligible compared to LLM inference latency (1-30 seconds). The embedding service is already warm in Conduit.

**Token Budget**: The formatted RAG context is injected as a system message. With 10 chunks averaging 200 tokens each, that is ~2000 tokens of context. This is well within the 8192 max_tokens configured for Claude. Guide authors should be aware that RAG context consumes part of the token budget.

**Logging**: The `build_rag_context` output should be logged to `dispatch_task_attempt_events` alongside existing context snapshots (the `started_context` JSON already includes `dispatch_intent`, `stage_params`, `step_params`).

### 8c. Deferred to Implementation Agent

- Exact Rust function signatures (follow existing patterns in `dispatch_runner.rs`)
- Error types for `RagService` (reuse existing `ConduitError` variants)
- `format_rag_context` string formatting details (the format shown above is a starting point)
- Test data generation for RAG integration tests
- Whether to use `search_guide_content_embeddings_by_guide_ids_excluding` or filter client-side

---

## 9. Testing Strategy

### 9a. Unit Tests

**RagService tests** (in `dapple-prompt-conduit`):
- `test_retrieve_empty_guide_ids` -- Returns empty result
- `test_retrieve_with_exclude_ids` -- Filters out excluded blocks
- `test_format_rag_context_empty` -- Returns empty string
- `test_format_rag_context_multiple_chunks` -- Formats correctly

**build_rag_context tests** (in `dapple-prompt-flow`):
- `test_build_rag_context_no_guide_ids` -- Returns None
- `test_build_rag_context_from_function_input` -- Extracts guide_ids correctly
- `test_build_rag_context_from_stage_params` -- Fallback to stage params
- `test_extract_audience_tags_default` -- Returns ["agent", "universal"]
- `test_extract_audience_tags_override` -- Uses step params override

### 9b. Integration Tests

**Conduit integration** (requires database with embeddings):
- POST `/prompt` with `rag_context` containing valid guide_ids
- Verify the response includes RAG context in the system message
- POST `/prompt` without `rag_context` -- backward compatible, no RAG

**Flow integration** (lifecycle tests):
- Create a guide with embeddings (reuse Phase 2 pipeline)
- Create a dispatch that references the guide
- Execute the dispatch and verify RAG context was passed to Conduit
- Verify accumulated `_rag_seen_block_ids` grows across steps

### 9c. Graceful Degradation Tests

- POST `/prompt` with `rag_context` when embedding service is not configured -- should succeed without RAG
- POST `/prompt` with `rag_context` containing non-existent guide_ids -- should succeed without RAG chunks
- POST `/prompt` with `rag_context` when pgvector has no data -- should succeed with empty RAG context

---

## 10. Error Handling

### Principle: RAG Failure Is Not Prompt Failure

RAG retrieval is an enhancement. If any step in the RAG pipeline fails, the prompt should still execute without RAG context. Specific failure modes:

| Failure | Behavior |
|---------|----------|
| `rag_context` is `None` | No RAG, proceed normally |
| `guide_ids` is empty | No RAG, proceed normally |
| `rag_service` is `None` | Log debug, no RAG, proceed normally |
| Embedding generation fails | Log warning, no RAG, proceed normally |
| pgvector query fails | Log warning, no RAG, proceed normally |
| pgvector returns 0 results | Log debug, no RAG, proceed normally |
| All results excluded by `exclude_block_ids` | Log debug, no RAG, proceed normally |

All failure paths are logged at `warn` or `debug` level. No errors are returned to the client for RAG failures.

---

## 11. Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RAG_ENABLED` | `true` | Master switch for RAG in Conduit. When `false`, `rag_context` is ignored even if present. |
| `RAG_DEFAULT_LIMIT` | `10` | Default number of chunks to retrieve if `limit` not specified in `rag_context` |
| `RAG_MIN_SIMILARITY` | `0.3` | Minimum cosine similarity threshold. Chunks below this are discarded. |

These are read by `RagService` at construction time (or from a `RagConfig` struct like other services).

---

## 12. Data Flow Diagram

```
Flow (dispatch_runner)                    Conduit (prompt handler)
========================                  =========================

1. Build prompt_data
2. Build rag_context {
     guide_ids: [...],
     audience_tags: [...],
     locale: "en",
     exclude_block_ids: [...]
   }
3. Set metadata = {
     conduit_url,
     prompt_data,
     rag_context,        ------>
     attempt_id,
     playbook_task_id
   }
4. Execute Nushell script
   -> `prompt send "name"`
   -> HTTP POST /prompt {
        prompt_id,
        attempt_id,
        playbook_task_id,
        context_data,
        rag_context          ------>      5. Resolve model config
      }                                   6. Fetch attempt context
                                          7. Resolve visible slots
                                          8. Render prompt template
                                          9. RAG retrieval:
                                             a. Embed prompt text
                                             b. pgvector search
                                             c. Filter exclusions
                                             d. Format context
                                          10. Inject RAG as system msg
                                          11. Inject context_data as user msg
                                          12. Execute LLM (streaming)
                                <------   13. Stream tokens back
5. Accumulate task output                 14. Store prompt_response
6. Update _rag_seen_block_ids
7. Next task...
```

---

## 13. Open Questions

### Q1: Guide ID Discovery

How does Flow know which guide(s) to search? Current options:
- **Option A (recommended)**: Guide authors include `rag_guide_ids` in their task's `function_input` when authoring the guide. This is explicit and already supported via the `function_input` JSONB field.
- **Option B**: Derive from the playbook's source guide. This requires a query from `playbook` -> `guide` which is not currently in the dispatch DTO. Could be added to `Dispatch` in a future enhancement.
- **Option C**: Derive from the journey. Journeys reference playbooks which reference guides. Similar FK chain issue.

Recommendation: Start with Option A. Guide authors know which guides are relevant to their tasks. Option B can be added later as an automatic fallback.

### Q2: Token Budget Management

Should Conduit enforce a maximum token budget for RAG context? For example, limit RAG context to 2000 tokens regardless of the number of chunks. This would require:
- Estimating tokens per chunk (chunk_text.len() / 4 as rough estimate)
- Truncating chunks or reducing the number of chunks

Recommendation: Defer. Let guide authors control via `limit` in `rag_context`. If token budget becomes an issue in practice, add a `max_tokens` field to `RagContext`.

### Q3: RAG Context Ordering

Should retrieved chunks be ordered by similarity score (most relevant first) or by guide structure (chapter/section order)? The current SQL orders by similarity. Guide structure ordering would use the `depth` and `position` metadata fields.

Recommendation: Keep similarity ordering for Phase 4. Structural ordering can be explored later once we have real usage data.

### Q4: Cross-Guide Search

The original plan scoped RAG to specific guides via `guide_ids`. Should Phase 4 support searching across all guides? This would be useful for agents that need broad context but could return noisy results.

Recommendation: No. Keep it scoped to explicit guide_ids. Broad search is the human search use case (Phase 3). Agent RAG should be precise.

---

## 14. Future: Phase 5 Upgrade Path

If the stateless pgvector approach proves insufficient (large accumulated context sets, high-frequency re-queries of the same chunks), Phase 5 would introduce Lance:

1. Add `lancedb` dependency to `dapple-prompt-conduit`
2. Add `DashMap<Uuid, LanceDb>` to `AppState` (keyed by dispatch_id)
3. On first RAG request for a dispatch: create Lance instance, populate from pgvector
4. On subsequent requests: query Lance instead of pgvector, add new results
5. On dispatch completion: drop Lance instance
6. The `RagContext` and `PromptRequest` structures remain unchanged -- only the retrieval backend changes

This is a clean upgrade path because the `RagService.retrieve()` method encapsulates the retrieval strategy. The rest of the pipeline (Flow context building, prompt injection, graceful degradation) stays the same.
