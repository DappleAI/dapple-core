# `guide search` Nushell Command

**Status:** Draft
**Created:** 2026-02-25
**Dependencies:** pgvector search infrastructure (Phases 1-4 of `pg-vector-searching.md` -- all implemented)
**Companion plans:** `pg-vector-searching.md` (backend), `~/src/dapple/dapple-ui/packages/ui/docs/plans/references-page.md` (UI panel)

---

## Context

Stuart is an internal guide that powers the librarian panel in dapple-ui. It's a skill-based guide -- each playbook is a reusable skill (search, browse, focus, recommend) executed via Nushell tasks. Stuart's **search skill** needs to return structured, ranked results to the UI -- guide titles, slugs, chapters, similarity scores -- so the panel can render them as clickable cards.

The existing `prompt send` + RAG path gives the LLM implicit access to retrieved chunks, but doesn't return explicit ranked results the UI can render. The `GET /api/guide-search/` endpoint on prompt-api is fully implemented and returns exactly what the UI needs, but tasks can't make HTTP calls directly -- they go through Conduit.

This plan adds a `guide search` Nushell command to dapple-prompt-flow, following the established command pattern.

---

## What Exists

| Component | Status | Location |
|-----------|--------|----------|
| Search endpoint | Done | `crates/dapple-prompt-api/src/handler/guide_search.rs` |
| Search schema | Done | `crates/dapple-prompt-api/src/schema/guide_search.rs` |
| Embedding client | Done | `crates/dapple-prompt-api/src/service/embedding_client.rs` |
| pgvector queries | Done | `crates/dapple-data/src/query/guide_content_embeddings.rs` |
| Result DTO | Done | `crates/dapple-data/src/model/guide_content_embeddings.rs` |
| Command registration | Done | `crates/dapple-prompt-flow/src/commands/mod.rs` |
| ConduitClient | Done | `crates/dapple-prompt-flow/src/conduit_client.rs` |
| Conduit `/embed` endpoint | Done | `crates/dapple-prompt-conduit/src/handler/embed.rs` |

---

## Routing Decision: Option A -- Via Conduit

### Options Considered

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| **A: Via Conduit** | Add `/search` endpoint to Conduit alongside `/embed` and `/prompt` | Follows existing pattern; auth is automatic; Conduit already has `EmbeddingService` + DB pool | Conduit gains another endpoint |
| B: Direct to prompt-api | Command calls `GET /api/guide-search/` on prompt-api | Simpler endpoint | Breaks pattern (tasks talk only to Conduit); requires separate auth; prompt-api is user-facing, not task-facing |
| C: Via `DAPPLE_METADATA` | Add `search_url` field to metadata | Keeps URL in metadata layer | Still breaks Conduit-only pattern; metadata bloat; requires separate auth |

**Decision: Option A.** All existing Nushell commands (`prompt send`, `summarize`, `madlib`, `synth`) go through Conduit. The search command should follow the same pattern. Conduit already has everything needed: the `EmbeddingService` for generating query embeddings, the database pool for pgvector queries, and the same search query functions from `dapple-data`.

The prompt-api search endpoint remains for direct HTTP clients (e.g., future REST consumers). Conduit's `/search` endpoint serves Nushell tasks within the dispatch pipeline.

---

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                      dapple-prompt-flow                       │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────────┐                                    │
│  │ GuideCommand         │  ("guide" -- parent, help-only)    │
│  └──────────────────────┘                                    │
│                                                              │
│  ┌──────────────────────┐                                    │
│  │ GuideSearch           │──┐  ("guide search")              │
│  │ (nu Command)          │  │                                │
│  └──────────────────────┘  │                                 │
│                             ▼                                │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              ConduitClient                            │   │
│  │                                                       │   │
│  │  POST /search { q, tags, locale, limit, threshold,   │   │
│  │                  guide_ids }                           │   │
│  └──────────────────────────────────────────────────────┘   │
│                             │                                │
└─────────────────────────────┼────────────────────────────────┘
                              ▼
┌──────────────────────────────────────────────────────────────┐
│                   dapple-prompt-conduit                       │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────┐    ┌──────────────────────────────┐   │
│  │ POST /search     │───▶│   search handler             │   │
│  │ handler          │    │                              │   │
│  └──────────────────┘    │  1. Embed query via          │   │
│                          │     EmbeddingService          │   │
│                          │  2. pgvector search via       │   │
│                          │     dapple-data queries       │   │
│                          │  3. Filter by threshold       │   │
│                          │  4. Return ranked results     │   │
│                          └──────────────────────────────┘   │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

---

## 1. Conduit `/search` Endpoint

### Request

```rust
// crates/dapple-prompt-conduit/src/handler/search.rs

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    /// The search query text (required, non-empty).
    pub q: String,

    /// Audience tags to filter by (default: ["universal"]).
    #[serde(default = "default_tags")]
    pub tags: Vec<String>,

    /// Locale for the search (default: "en").
    #[serde(default = "default_locale")]
    pub locale: String,

    /// Maximum number of results (clamped to 1-100, default: 20).
    #[serde(default = "default_limit")]
    pub limit: i32,

    /// Minimum similarity threshold (default: 0.0).
    #[serde(default)]
    pub threshold: f64,

    /// Optional: scope search to specific guide IDs.
    #[serde(default)]
    pub guide_ids: Vec<Uuid>,
}

fn default_tags() -> Vec<String> { vec!["universal".to_string()] }
fn default_locale() -> String { "en".to_string() }
fn default_limit() -> i32 { 20 }
```

### Response

```rust
#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub block_id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub directive_type: String,
    pub chunk_text: String,
    pub metadata: serde_json::Value,
    pub similarity: f64,
}
```

The `metadata` field carries `guide_title`, `chapter_title`, and other enrichment data added during the embedding pipeline. This matches the existing `GuideSearchResult` DTO from `dapple-data`.

### Handler

```rust
pub async fn handle_search(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<SearchErrorResponse>)> {
    // 1. Validate: q must be non-empty
    if req.q.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(SearchErrorResponse {
            error: "Query must not be empty".into(),
        })));
    }

    // 2. Clamp limit
    let limit = req.limit.clamp(1, 100);

    // 3. Embed query text
    let embedding_service = state.embedding_service.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, Json(SearchErrorResponse {
            error: "Embedding service not configured".into(),
        })))?;

    let query_embedding = embedding_service.embed_one(&req.q).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(SearchErrorResponse {
            error: format!("Embedding generation failed: {}", e),
        })))?;

    // 4. pgvector search (scoped or global)
    let results = if req.guide_ids.is_empty() {
        search_guide_content_embeddings(
            &state.pool, &query_embedding, &req.tags, &req.locale, limit,
        ).await
    } else {
        search_guide_content_embeddings_by_guide_ids(
            &state.pool, &query_embedding, &req.guide_ids, &req.tags, &req.locale, limit,
        ).await
    }.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(SearchErrorResponse {
        error: format!("Search query failed: {}", e),
    })))?;

    // 5. Filter by threshold
    let filtered: Vec<SearchResultItem> = results
        .into_iter()
        .filter(|r| r.similarity >= req.threshold)
        .map(|r| SearchResultItem {
            block_id: r.block_id,
            guide_id: r.guide_id,
            chapter_id: r.chapter_id,
            directive_type: r.directive_type_key,
            chunk_text: r.chunk_text,
            metadata: r.metadata,
            similarity: r.similarity,
        })
        .collect();

    let count = filtered.len();
    Ok(Json(SearchResponse { results: filtered, count }))
}
```

### Route Registration

```rust
// crates/dapple-prompt-conduit/src/main.rs

let app = Router::new()
    .route("/api/health_check", get(health_check))
    .route("/prompt", post(handler::handle_prompt))
    .route("/summarize", post(handler::handle_summarize))
    .route("/madlib", post(handler::handle_madlib))
    .route("/synthesize", post(handler::handle_synthesize))
    .route("/embed", post(handler::handle_embed))
    .route("/search", post(handler::handle_search))  // NEW
    .layer(cors)
    .with_state(state);
```

### Authentication

No additional auth needed. Conduit is an internal service -- tasks reach it via `conduit_url` from `DAPPLE_METADATA`, which is set by the dispatch runner. There is no user-facing auth on Conduit endpoints (same as `/embed`, `/prompt`).

---

## 2. ConduitClient Method

```rust
// crates/dapple-prompt-flow/src/conduit_client.rs

#[derive(Debug, Serialize)]
pub struct SearchRequest {
    pub q: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub guide_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub count: usize,
}

#[derive(Debug, Deserialize)]
pub struct SearchResultItem {
    pub block_id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub directive_type: String,
    pub chunk_text: String,
    pub metadata: serde_json::Value,
    pub similarity: f64,
}

impl ConduitClient {
    pub async fn search(
        &self,
        request: SearchRequest,
    ) -> Result<SearchResponse, ConduitClientError> {
        let url = format!("{}/search", self.base_url);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            response.json().await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::PromptError {
                code: status.to_string(),
                message: body,
                retryable: status.is_server_error(),
            })
        }
    }
}
```

---

## 3. Nushell Command: `guide search`

### Parent Command: `guide`

Following the `prompt` / `prompt send` pattern, create a `guide` parent command that shows help text:

```rust
// crates/dapple-prompt-flow/src/commands/guide.rs

#[derive(Clone)]
pub struct GuideCommand;

impl Command for GuideCommand {
    fn name(&self) -> &str {
        "guide"
    }

    fn signature(&self) -> Signature {
        Signature::build("guide")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Commands for working with guides."
    }

    fn extra_description(&self) -> &str {
        "Subcommands:\n  guide search - Search guide content by semantic similarity"
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string(self.extra_description(), call.head),
            None,
        ))
    }
}
```

### Subcommand: `guide search`

```rust
// crates/dapple-prompt-flow/src/commands/guide_search.rs

use crate::conduit_client::{ConduitClient, SearchRequest};
use crate::commands::prompt_send::{get_dapple_metadata, get_conduit_url, value_to_json};
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct GuideSearch;

impl Command for GuideSearch {
    fn name(&self) -> &str {
        "guide search"
    }

    fn signature(&self) -> Signature {
        Signature::build("guide search")
            .input_output_types(vec![
                (Type::String, Type::table()),   // Piped query string
                (Type::Nothing, Type::table()),  // Query via positional arg
            ])
            .required(
                "query",
                SyntaxShape::String,
                "The search query text",
            )
            .named(
                "tags",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Audience tags to filter by (default: [\"universal\"])",
                Some('t'),
            )
            .named(
                "locale",
                SyntaxShape::String,
                "Locale for the search (default: \"en\")",
                Some('l'),
            )
            .named(
                "limit",
                SyntaxShape::Int,
                "Maximum number of results (default: 20, max: 100)",
                Some('n'),
            )
            .named(
                "threshold",
                SyntaxShape::Number,
                "Minimum similarity threshold, 0.0-1.0 (default: 0.0)",
                None,
            )
            .named(
                "guide-ids",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Scope search to specific guide IDs",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Search guide content by semantic similarity using pgvector."
    }

    fn extra_description(&self) -> &str {
        r#"Searches guide content embeddings and returns ranked results.
Each result includes the guide ID, chapter ID, block ID, chunk text,
metadata (guide_title, chapter_title), and similarity score.

Results are returned as a Nushell table that can be filtered,
sorted, and transformed with standard Nushell commands.

Environment:
- DAPPLE_METADATA.conduit_url: URL of the conduit service
- Falls back to DAPPLE_PROMPT_CONDUIT_URL env var
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["search", "guide", "find", "query", "semantic", "vector"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Search for guides about authentication",
                example: r#"guide search "how to authenticate with the API""#,
                result: None,
            },
            Example {
                description: "Search with locale and limit",
                example: r#"guide search "workflow building" --locale pt-br --limit 5"#,
                result: None,
            },
            Example {
                description: "Search with similarity threshold",
                example: r#"guide search "dispatch tasks" --threshold 0.3"#,
                result: None,
            },
            Example {
                description: "Filter results by similarity",
                example: r#"guide search "API integration" | where similarity > 0.4"#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        // Required: query string
        let query: String = call.req(engine_state, stack, 0)?;
        if query.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Search query must not be empty".into(),
                msg: "Provide a non-empty search query".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

        // Optional flags
        let tags: Option<Vec<String>> = call.get_flag(engine_state, stack, "tags")?;
        let locale: Option<String> = call.get_flag(engine_state, stack, "locale")?;
        let limit: Option<i64> = call.get_flag(engine_state, stack, "limit")?;
        let threshold: Option<f64> = call.get_flag(engine_state, stack, "threshold")?;
        let guide_ids: Option<Vec<String>> = call.get_flag(engine_state, stack, "guide-ids")?;

        // Parse guide_ids as UUIDs
        let parsed_guide_ids: Vec<Uuid> = guide_ids
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.parse::<Uuid>().map_err(|e| ShellError::GenericError {
                error: format!("Invalid guide ID '{}': {}", s, e),
                msg: "Guide IDs must be valid UUIDs".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            }))
            .collect::<Result<Vec<_>, _>>()?;

        // Get conduit URL
        let metadata = get_dapple_metadata(engine_state, stack);
        let conduit_url = get_conduit_url(&metadata);

        // Build request
        let request = SearchRequest {
            q: query,
            tags: tags.unwrap_or_default(),
            locale,
            limit: limit.map(|v| v as i32),
            threshold,
            guide_ids: parsed_guide_ids,
        };

        // Execute via conduit
        let client = ConduitClient::new(&conduit_url);
        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { client.search(request).await })
        });

        match response {
            Ok(resp) => {
                let records: Vec<Value> = resp.results
                    .into_iter()
                    .map(|item| search_result_to_value(item, span))
                    .collect();

                Ok(Value::list(records, span).into_pipeline_data())
            }
            Err(e) => Err(search_error_to_shell_error(e, span)),
        }
    }
}

/// Convert a search result item into a Nushell record.
fn search_result_to_value(item: SearchResultItem, span: Span) -> Value {
    // Extract enriched fields from metadata
    let guide_title = item.metadata
        .get("guide_title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let chapter_title = item.metadata
        .get("chapter_title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Value::record(
        Record::from_raw_cols_vals(
            vec![
                "block_id".into(),
                "guide_id".into(),
                "chapter_id".into(),
                "guide_title".into(),
                "chapter_title".into(),
                "directive_type".into(),
                "chunk_text".into(),
                "similarity".into(),
                "metadata".into(),
            ],
            vec![
                Value::string(item.block_id.to_string(), span),
                Value::string(item.guide_id.to_string(), span),
                Value::string(item.chapter_id.to_string(), span),
                Value::string(guide_title, span),
                Value::string(chapter_title, span),
                Value::string(item.directive_type, span),
                Value::string(item.chunk_text, span),
                Value::float(item.similarity, span),
                json_to_nu_value(&item.metadata, span),
            ],
        ),
        span,
    )
}

/// Convert a conduit client error into a Nushell shell error.
fn search_error_to_shell_error(e: ConduitClientError, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("Guide search failed: {}", e),
        msg: "Search request to Conduit failed".into(),
        span: Some(span),
        help: Some(
            "Check that Conduit is running and the embedding service is configured".into(),
        ),
        inner: vec![],
    }
}
```

### Return Format

The command returns a Nushell table. Each row is a record:

| Column | Type | Description |
|--------|------|-------------|
| `block_id` | string (UUID) | The embedding block ID |
| `guide_id` | string (UUID) | The source guide's ID |
| `chapter_id` | string (UUID) | The source chapter's ID |
| `guide_title` | string | Guide title (from metadata) |
| `chapter_title` | string | Chapter title (from metadata) |
| `directive_type` | string | Block type (`prose`, `stage`, `step`, etc.) |
| `chunk_text` | string | The matched text chunk |
| `similarity` | float | Cosine similarity score (0.0 - 1.0) |
| `metadata` | record | Full metadata JSON as Nushell record |

This is a standard Nushell table, so users can pipe it through `where`, `sort-by`, `select`, `first`, etc.:

```nushell
# Top 5 results above 0.3 similarity
guide search "authentication" --limit 10 | where similarity > 0.3 | first 5

# Just guide titles and scores
guide search "workflow" | select guide_title chapter_title similarity

# Group by guide
guide search "dispatch" | group-by guide_title
```

---

## 4. Command Registration

```rust
// crates/dapple-prompt-flow/src/commands/mod.rs

pub mod guide;
pub mod guide_search;

pub fn register_dapple_commands(engine_state: &mut EngineState) {
    let mut working_set = StateWorkingSet::new(engine_state);

    // Existing commands
    working_set.add_decl(Box::new(prompt::PromptCommand));
    working_set.add_decl(Box::new(prompt_send::PromptSend));
    working_set.add_decl(Box::new(prompt_retry::PromptRetry));
    working_set.add_decl(Box::new(prompt_reset::PromptReset));
    working_set.add_decl(Box::new(summarize::SummarizeCommand));
    working_set.add_decl(Box::new(madlib::MadlibCommand));
    working_set.add_decl(Box::new(json_schema::JsonSchemaCommand));
    working_set.add_decl(Box::new(synth::SynthCommand));

    // Guide commands (NEW)
    working_set.add_decl(Box::new(guide::GuideCommand));
    working_set.add_decl(Box::new(guide_search::GuideSearch));

    let _ = engine_state.merge_delta(working_set.render());
}
```

---

## 5. Error Handling

Follow the `prompt_error_to_shell_error` precedent. Search errors are not retryable at the command level (the user can simply re-run the search with different terms).

| Failure Mode | Behavior |
|--------------|----------|
| Empty query | `ShellError::GenericError` before any HTTP call |
| Invalid guide ID format | `ShellError::GenericError` with UUID parse failure message |
| Conduit unreachable | `ShellError::GenericError` with network error |
| Embedding service unavailable (503) | `ShellError::GenericError` with "embedding service not configured" hint |
| pgvector query failure | `ShellError::GenericError` with "search query failed" |
| No results | Returns empty table (not an error) |

---

## 6. Future Sibling Commands

The `guide` parent command is designed for extensibility. Potential siblings:

| Command | Purpose | Priority |
|---------|---------|----------|
| `guide search` | Semantic search via pgvector | **This plan** |
| `guide list` | List available guides (by tags, locale, classification) | Low -- useful but not needed for Stuart |
| `guide get` | Fetch a specific guide's content by slug or ID | Low -- guides are rendered by the UI, not by tasks |

These can be added later by creating new files in `commands/` and registering them. The `guide` parent command already provides the namespace.

---

## Files to Create/Modify

| Crate | File | Action | Description |
|-------|------|--------|-------------|
| conduit | `src/handler/search.rs` | Create | `/search` endpoint handler |
| conduit | `src/handler/mod.rs` | Modify | Export `search` module |
| conduit | `src/main.rs` | Modify | Register `/search` route |
| flow | `src/conduit_client.rs` | Modify | Add `search()` method + request/response types |
| flow | `src/commands/guide.rs` | Create | `guide` parent command |
| flow | `src/commands/guide_search.rs` | Create | `guide search` subcommand |
| flow | `src/commands/mod.rs` | Modify | Register both commands |

---

## Testing Strategy

### Unit Tests (Flow)

```rust
#[test]
fn test_guide_search_command_signature() {
    let cmd = GuideSearch;
    assert_eq!(cmd.name(), "guide search");

    let sig = cmd.signature();
    // Has required positional: query
    assert_eq!(sig.required_positional.len(), 1);
    assert_eq!(sig.required_positional[0].name, "query");

    // Has optional flags
    let flag_names: Vec<&str> = sig.named.iter().map(|f| f.long.as_str()).collect();
    assert!(flag_names.contains(&"tags"));
    assert!(flag_names.contains(&"locale"));
    assert!(flag_names.contains(&"limit"));
    assert!(flag_names.contains(&"threshold"));
    assert!(flag_names.contains(&"guide-ids"));
}

#[test]
fn test_guide_parent_command() {
    let cmd = GuideCommand;
    assert_eq!(cmd.name(), "guide");
}

#[test]
fn test_search_result_to_value() {
    let item = SearchResultItem {
        block_id: Uuid::nil(),
        guide_id: Uuid::nil(),
        chapter_id: Uuid::nil(),
        directive_type: "prose".into(),
        chunk_text: "Example text".into(),
        metadata: serde_json::json!({
            "guide_title": "API Consumer Guide",
            "chapter_title": "Authentication"
        }),
        similarity: 0.85,
    };

    let span = Span::test_data();
    let value = search_result_to_value(item, span);

    // Verify record structure
    if let Value::Record { val, .. } = value {
        assert_eq!(val.get("guide_title").unwrap().as_str().unwrap(), "API Consumer Guide");
        assert_eq!(val.get("similarity").unwrap().as_f64().unwrap(), 0.85);
    } else {
        panic!("Expected record");
    }
}
```

### Integration Tests (Conduit)

```rust
#[tokio::test]
#[ignore] // Requires running Conduit with embedding service
async fn test_search_via_conduit() {
    let client = ConduitClient::new("http://localhost:8081");

    let request = SearchRequest {
        q: "authentication".into(),
        tags: vec!["universal".into()],
        locale: Some("en".into()),
        limit: Some(5),
        threshold: Some(0.0),
        guide_ids: vec![],
    };

    let response = client.search(request).await.unwrap();
    // Results may be empty if no guides are ingested, but the call should succeed
    assert!(response.count <= 5);
}

#[tokio::test]
#[ignore]
async fn test_search_empty_query_rejected() {
    let client = ConduitClient::new("http://localhost:8081");

    let request = SearchRequest {
        q: "".into(),
        tags: vec![],
        locale: None,
        limit: None,
        threshold: None,
        guide_ids: vec![],
    };

    let result = client.search(request).await;
    assert!(result.is_err());
}
```

### Manual Testing (Nushell)

```nushell
# Basic search
guide search "how to authenticate"

# With all flags
guide search "workflow automation" --tags ["product", "universal"] --locale en --limit 10 --threshold 0.3

# Pipe through Nushell table operations
guide search "dispatch" | sort-by -r similarity | first 3

# Use in a task script (Stuart's search skill)
let results = (guide search $user_query --limit 10 --threshold 0.25)
if ($results | length) > 0 {
    $results | select guide_title chapter_title similarity
} else {
    "No results found"
}
```

---

## Acceptance Criteria

- [ ] `POST /search` endpoint exists in Conduit and returns ranked results
- [ ] `ConduitClient::search` method works
- [ ] `guide` parent command registered
- [ ] `guide search` subcommand registered
- [ ] Positional `query` argument works
- [ ] `--tags`, `--locale`, `--limit`, `--threshold`, `--guide-ids` flags work
- [ ] Returns Nushell table with correct column schema
- [ ] Empty query rejected with clear error
- [ ] Graceful error when Conduit unreachable
- [ ] Graceful error when embedding service unavailable
- [ ] Empty result set returns empty table (not an error)
- [ ] Results pipeable through standard Nushell commands (`where`, `sort-by`, `select`)

---

## Stuart Integration (Preview)

Once this command exists, Stuart's search skill playbook will contain a task like:

```nushell
# search.md :: search stage :: execute-search step
let results = (guide search $user_query --limit 10 --threshold 0.25 --tags ["product", "universal"])

if ($results | length) == 0 {
    { type: "no_results", message: "No guides matched your search." }
} else {
    {
        type: "search_results",
        results: ($results | each { |r|
            {
                guide_id: $r.guide_id,
                guide_title: $r.guide_title,
                chapter_title: $r.chapter_title,
                chunk_text: ($r.chunk_text | str substring 0..200),
                similarity: $r.similarity,
            }
        })
    }
}
```

The task output becomes the data source for the UI panel's search results cards (via the dispatch → value slot → form rendering pipeline).
