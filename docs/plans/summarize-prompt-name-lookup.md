# Summarize Command: Prompt Name Lookup Architecture Plan

**Status:** Draft
**Created:** 2026-01-24
**Last Updated:** 2026-01-24
**Target Version:** 0.2.0
**Dependencies:**
- `docs/plans/summarize-prompt-prefix-flag.md` (implements current `--prompt` literal string flag)
- `task_prompts` infrastructure (existing)

## Executive Summary

This plan transforms the `summarize --prompt` flag from accepting a literal T5 prefix string to accepting a **Prompt name** that is resolved via `DAPPLE_METADATA.prompt_mappings`. This aligns `summarize` with the established pattern used by `prompt send`, where prompts are looked up by their local name bound to the task via `task_prompts`.

The change enables workflow authors to define T5 prefixes as versioned Prompt objects in the database, bind them to tasks, and reference them by name in Nushell scripts. This provides consistency, reusability, and the ability to evolve prompt prefixes without modifying task scripts.

**Key Changes:**
1. **Flow:** `summarize` command looks up `prompt_object_id` from `prompt_mappings` (same as `prompt send`)
2. **Flow:** `SummarizeRequest` uses `prompt_object_id: Option<Uuid>` instead of `prompt_prefix: Option<String>`
3. **Conduit:** Handler accepts `prompt_object_id`, resolves Prompt via overlay, extracts T5 prefix from the resolved prompt

**Out of Scope:**
- How fluent messages become the T5 prefix text (separate "fluency" design concern)
- For now, assume conduit can call some method to get prefix text from the resolved Prompt

## Context and Background

### Current State

The `summarize` command currently accepts a `--prompt` flag that takes a literal string:

```nushell
# Current usage (accepts literal prefix string)
$activity_log | summarize --prompt "normalize this activity log:"
```

This literal string is passed through the entire stack:

1. **Flow:** `SummarizeCommand` extracts the `--prompt` flag as `Option<String>`
2. **Flow:** `ConduitClient::summarize()` sends `prompt_prefix: Option<String>` in the HTTP request
3. **Conduit:** `SummarizationService::summarize_text_with_prefix()` passes the prefix to T5 backend
4. **Conduit:** `T5SummarizationBackend::prepare_input()` prepends the prefix to input text

### Target Pattern (from `prompt send`)

The `prompt send` command demonstrates the desired pattern:

```rust
// dapple-prompt-flow/src/commands/prompt_send.rs

/// Metadata passed via DAPPLE_METADATA environment variable
#[derive(Debug, Deserialize, Serialize)]
struct DappleMetadata {
    attempt_id: i64,
    playbook_task_id: i64,
    conduit_url: Option<String>,
    /// Mapping of prompt names to their object_ids
    /// Built from task_prompts when the engine is configured
    #[serde(default)]
    prompt_mappings: HashMap<String, Uuid>,
}

fn execute_prompt(name: &str, metadata: &DappleMetadata, span: Span) -> Result<String, ShellError> {
    // Look up prompt_object_id from prompt_mappings
    let prompt_object_id = match metadata.prompt_mappings.get(name) {
        Some(id) => *id,
        None => {
            return Err(ShellError::GenericError {
                error: format!("Prompt '{}' not found in prompt_mappings", name),
                msg: "Check that the prompt is bound to this task via task_prompts".into(),
                span: Some(span),
                help: Some(format!(
                    "Available prompts: {:?}",
                    metadata.prompt_mappings.keys().collect::<Vec<_>>()
                )),
                inner: vec![],
            });
        }
    };

    // Send prompt_object_id to conduit
    let request = PromptRequest {
        prompt_object_id,
        attempt_id: metadata.attempt_id,
        playbook_task_id: metadata.playbook_task_id,
    };

    client.execute_prompt(request).await
}
```

### Target Usage

```nushell
# In a Task, with "activity-log-normalizer" bound via task_prompts:
$text | summarize --prompt "activity-log-normalizer"

# The prompt name is looked up in DAPPLE_METADATA.prompt_mappings
# to find the prompt_object_id, which is sent to conduit
```

## Data Flow Design

### New Data Flow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         dapple-prompt-flow                               │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐                                                    │
│  │ SummarizeCommand│  --prompt "activity-log-normalizer"               │
│  │ (nu Command)    │                                                    │
│  └────────┬────────┘                                                    │
│           │                                                             │
│           ▼                                                             │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                     DappleMetadata                               │   │
│  │                                                                  │   │
│  │  prompt_mappings: {                                              │   │
│  │    "activity-log-normalizer" -> UUID("abc-123-..."),             │   │
│  │    "dialogue-compressor" -> UUID("def-456-..."),                 │   │
│  │  }                                                               │   │
│  └────────┬────────────────────────────────────────────────────────┘   │
│           │ lookup by name                                             │
│           ▼                                                             │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                     ConduitClient                                │   │
│  │                                                                  │   │
│  │  POST /summarize {                                               │   │
│  │    text: "...",                                                  │   │
│  │    prompt_object_id: "abc-123-...",  // NEW: UUID instead of str │   │
│  │    playbook_task_id: 42,             // NEW: for overlay         │   │
│  │    max_length: 50                                                │   │
│  │  }                                                               │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                       dapple-prompt-conduit                              │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐                                                    │
│  │ /summarize      │────▶ resolve_prompt_prefix(prompt_object_id,      │
│  │ handler         │        playbook_task_id)                           │
│  └─────────────────┘              │                                     │
│                                   ▼                                     │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                   SummarizationService                           │   │
│  │                                                                  │   │
│  │   summarize_text_with_prefix(&text, Some("normalize this..."))  │   │
│  └────────────────────────────────────┬────────────────────────────┘   │
│                                       │                                 │
│                       ┌───────────────▼──────────────┐                  │
│                       │   T5SummarizationBackend     │                  │
│                       │                              │                  │
│                       │   prepare_input() uses       │                  │
│                       │   resolved prefix            │                  │
│                       └──────────────────────────────┘                  │
└─────────────────────────────────────────────────────────────────────────┘
```

### Backward Compatibility Consideration

The current `--prompt` flag accepts a literal string. After this change, it accepts a **prompt name** that must exist in `prompt_mappings`. This is a **breaking change** for workflows that use literal prefixes.

**Migration Path:**
1. Create Prompt objects with the desired T5 prefixes
2. Bind them to tasks via `task_prompts`
3. Update Nushell scripts to use the prompt name instead of the literal prefix

**Alternative (if needed):** Add a separate `--prompt-prefix` flag for literal strings while `--prompt` does name lookup. This adds complexity but preserves backward compatibility.

**Recommendation:** Proceed with the breaking change. The literal prefix pattern was always intended as a stepping stone to proper prompt management.

## API Contract Changes

### Flow: SummarizeRequest (Client-Side)

**Before:**
```rust
#[derive(Debug, Serialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_prefix: Option<String>,  // Literal T5 prefix
}
```

**After:**
```rust
#[derive(Debug, Serialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    /// Prompt object_id for T5 prefix resolution (resolved from name in flow)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_object_id: Option<Uuid>,
    /// Playbook task ID for overlay resolution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playbook_task_id: Option<i64>,
}
```

### Conduit: SummarizeRequest (Server-Side)

**Before:**
```rust
#[derive(Debug, Deserialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub min_length: Option<usize>,
    #[serde(default)]
    pub prompt_prefix: Option<String>,
}
```

**After:**
```rust
#[derive(Debug, Deserialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub min_length: Option<usize>,
    /// Prompt object_id for T5 prefix lookup (mutually exclusive with prompt_prefix)
    #[serde(default)]
    pub prompt_object_id: Option<Uuid>,
    /// Playbook task ID for overlay-aware prefix resolution
    #[serde(default)]
    pub playbook_task_id: Option<i64>,
    /// Direct prompt prefix (legacy, for backward compatibility if needed)
    #[serde(default)]
    pub prompt_prefix: Option<String>,
}
```

**Resolution Priority in Handler:**
1. If `prompt_prefix` is provided, use it directly (legacy/testing path)
2. If `prompt_object_id` is provided, resolve via database
3. Otherwise, use default `"summarize:"`

## Implementation Guidance

### Phase 1: Update Flow Command

**File:** `dapple-prompt-flow/src/commands/summarize.rs`

1. **Add `prompt_mappings` to `DappleMetadata`:**

```rust
/// Metadata passed via DAPPLE_METADATA environment variable
#[derive(Debug, Deserialize)]
struct DappleMetadata {
    attempt_id: Option<i64>,
    playbook_task_id: Option<i64>,
    conduit_url: Option<String>,
    /// Mapping of prompt names to their object_ids
    #[serde(default)]
    prompt_mappings: HashMap<String, Uuid>,
}
```

2. **Update `run` method to look up prompt name:**

```rust
fn run(
    &self,
    engine_state: &EngineState,
    stack: &mut Stack,
    call: &Call,
    input: PipelineData,
) -> Result<PipelineData, ShellError> {
    let span = call.head;

    // Get optional parameters
    let max_length: Option<i64> = call.get_flag(engine_state, stack, "max-length")?;
    let min_length: Option<i64> = call.get_flag(engine_state, stack, "min-length")?;
    let prompt_name: Option<String> = call.get_flag(engine_state, stack, "prompt")?;

    // Get metadata from environment
    let metadata = get_dapple_metadata(engine_state, stack)?;

    // Resolve prompt_object_id if prompt name is provided
    let prompt_object_id = match prompt_name {
        Some(name) => {
            match metadata.prompt_mappings.get(&name) {
                Some(id) => Some(*id),
                None => {
                    return Err(ShellError::GenericError {
                        error: format!("Prompt '{}' not found in prompt_mappings", name),
                        msg: "Check that the prompt is bound to this task via task_prompts".into(),
                        span: Some(span),
                        help: Some(format!(
                            "Available prompts: {:?}",
                            metadata.prompt_mappings.keys().collect::<Vec<_>>()
                        )),
                        inner: vec![],
                    });
                }
            }
        }
        None => None,
    };

    // Convert input to string
    let input_value = input.into_value(span)?;
    let text = value_to_string(&input_value, span)?;

    // ... validation ...

    // Build request
    let request = SummarizeRequest {
        text,
        max_length: max_length.map(|v| v as usize),
        min_length: min_length.map(|v| v as usize),
        prompt_object_id,
        playbook_task_id: metadata.playbook_task_id,
    };

    // Execute via conduit
    // ...
}
```

3. **Update examples and description:**

```rust
fn extra_description(&self) -> &str {
    r#"Takes text input via pipeline and returns a concise summary.
Optimized for dialogue and conversational content.

The command calls the conduit summarization service, which uses
a T5 model fine-tuned on the SAMSum dialogue dataset.

The --prompt flag accepts a prompt NAME (not a literal string).
The prompt must be bound to the current task via task_prompts.
Conduit resolves the prompt to extract the T5 prefix.

Environment:
- CONDUIT_URL: URL of the conduit service (default: http://localhost:8081)
- DAPPLE_METADATA: JSON with prompt_mappings, conduit_url, playbook_task_id
"#
}

fn examples(&self) -> Vec<Example<'_>> {
    vec![
        Example {
            description: "Summarize a dialogue",
            example: r#""Alice: Hi! Bob: Hello!" | summarize"#,
            result: Some(Value::test_string("Alice and Bob greeted each other.")),
        },
        Example {
            description: "Summarize with length constraint",
            example: r#"$document | summarize --max-length 50"#,
            result: None,
        },
        Example {
            description: "Use a named prompt for activity log normalization",
            example: r#"$activity_log | summarize --prompt "activity-log-normalizer""#,
            result: None,
        },
        Example {
            description: "Use a compression prompt",
            example: r#"$long_text | summarize --prompt "dialogue-compressor" --max-length 50"#,
            result: None,
        },
    ]
}
```

### Phase 2: Update ConduitClient

**File:** `dapple-prompt-flow/src/conduit_client.rs`

```rust
/// Request to summarize text via conduit
#[derive(Debug, Serialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    /// Prompt object_id for T5 prefix resolution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_object_id: Option<Uuid>,
    /// Playbook task ID for overlay resolution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playbook_task_id: Option<i64>,
}
```

Update debug logging:

```rust
log::debug!(
    "Summarizing text: url={}, text_len={}, max_length={:?}, min_length={:?}, prompt_object_id={:?}, playbook_task_id={:?}",
    url,
    request.text.len(),
    request.max_length,
    request.min_length,
    request.prompt_object_id,
    request.playbook_task_id
);
```

### Phase 3: Update Conduit Handler

**File:** `dapple-prompt-conduit/src/handler/summarize.rs`

```rust
use uuid::Uuid;

/// Request to summarize text
#[derive(Debug, Deserialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub min_length: Option<usize>,
    /// Prompt object_id for T5 prefix lookup
    #[serde(default)]
    pub prompt_object_id: Option<Uuid>,
    /// Playbook task ID for overlay-aware resolution
    #[serde(default)]
    pub playbook_task_id: Option<i64>,
    /// Direct prompt prefix (fallback/legacy)
    #[serde(default)]
    pub prompt_prefix: Option<String>,
}

pub async fn handle_summarize(
    State(state): State<AppState>,
    Json(req): Json<SummarizeRequest>,
) -> Result<Json<SummarizeResponse>, ConduitError> {
    log::debug!(
        "Summarize request: text_len={}, max_length={:?}, min_length={:?}, prompt_object_id={:?}, playbook_task_id={:?}, prompt_prefix={:?}",
        req.text.len(),
        req.max_length,
        req.min_length,
        req.prompt_object_id,
        req.playbook_task_id,
        req.prompt_prefix
    );

    let svc = state.summarization_service.as_ref().ok_or_else(|| {
        ConduitError::SummarizationError(
            "Summarization service not enabled. Set SUMMARIZATION_ENABLED=true".to_string(),
        )
    })?;

    if req.text.is_empty() {
        return Err(ConduitError::InvalidRequest(
            "Text cannot be empty".to_string(),
        ));
    }

    // Resolve prompt prefix from object_id or use direct prefix
    let prompt_prefix = resolve_prompt_prefix(
        &state,
        req.prompt_object_id,
        req.playbook_task_id,
        req.prompt_prefix,
    ).await?;

    let input_length = req.text.len();
    let summary = svc
        .summarize_text_with_prefix(&req.text, prompt_prefix.as_deref())
        .await?;
    let output_length = summary.len();

    log::debug!(
        "Summarization complete: input_length={}, output_length={}, prefix={:?}",
        input_length,
        output_length,
        prompt_prefix
    );

    Ok(Json(SummarizeResponse {
        summary,
        input_length,
        output_length,
    }))
}

/// Resolve the T5 prompt prefix from various sources
/// Priority: prompt_prefix > prompt_object_id > default
async fn resolve_prompt_prefix(
    state: &AppState,
    prompt_object_id: Option<Uuid>,
    playbook_task_id: Option<i64>,
    prompt_prefix: Option<String>,
) -> Result<Option<String>, ConduitError> {
    // Direct prefix takes precedence (legacy/testing path)
    if prompt_prefix.is_some() {
        return Ok(prompt_prefix);
    }

    // Resolve from prompt_object_id if provided
    if let Some(object_id) = prompt_object_id {
        let prefix = state.summarization_service
            .as_ref()
            .ok_or_else(|| ConduitError::SummarizationError(
                "Summarization service not configured".to_string()
            ))?
            .resolve_t5_prefix_from_prompt(
                &state.pool,
                object_id,
                playbook_task_id,
            )
            .await?;

        return Ok(Some(prefix));
    }

    // No prefix specified - use default
    Ok(None)
}
```

### Phase 4: Add Prompt Resolution to SummarizationService

**File:** `dapple-prompt-conduit/src/service/summarization/mod.rs`

Add a new method to resolve T5 prefix from a Prompt:

```rust
impl SummarizationService {
    // ... existing methods ...

    /// Resolve T5 prefix text from a Prompt object
    ///
    /// This method fetches the Prompt by object_id and extracts the
    /// T5 prefix from its content. The exact extraction logic depends
    /// on how T5 prompts are stored (see "Fluency Design" notes below).
    ///
    /// For now, we assume the Prompt has a single fluent message whose
    /// body contains the T5 prefix text.
    pub async fn resolve_t5_prefix_from_prompt(
        &self,
        pool: &DbPool,
        prompt_object_id: Uuid,
        playbook_task_id: Option<i64>,
    ) -> Result<String, ConduitError> {
        // TODO: The exact implementation depends on the "fluency" design
        // For now, this is a placeholder that demonstrates the interface

        // Option 1: Simple lookup - Prompt description contains the prefix
        let prefix = select_prompt_description_by_object_id(pool, prompt_object_id)
            .await
            .map_err(|e| ConduitError::Database(e))?
            .ok_or_else(|| ConduitError::NotFound(
                format!("Prompt {} not found", prompt_object_id)
            ))?;

        Ok(prefix)
    }
}
```

**Important Note:** The exact method of extracting T5 prefix text from a Prompt object is intentionally left as a TODO. This is because:

1. Prompts currently use `fluent_messages` associations which are designed for LLM chat templates, not T5 prefixes
2. T5 prefixes are simple strings like `"summarize:"` or `"compress:"`
3. A separate "fluency" design decision is needed to determine how these prefixes are stored

**Possible Storage Approaches (for future design):**
- Use Prompt `description` field to store T5 prefix
- Create a new `t5_prefix` column on prompts
- Use the first fluent message's body as the prefix
- Add a `prompt_kind` enum to distinguish LLM prompts from T5 prompts

### Phase 5: Database Query for Prompt Description

**File:** `dapple-data/src/query/prompts.rs`

Add a simple query to fetch prompt description:

```rust
/// Select prompt description by object_id (for T5 prefix resolution)
pub async fn select_prompt_description_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<Option<String>, DataError> {
    let result = sqlx::query_scalar!(
        r#"
        SELECT pd.description
        FROM rev.prompts p
        JOIN rev.prompt_details pd ON pd.prompt_id = p.id
        WHERE p.object_id = $1
        "#,
        object_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(result.flatten())
}
```

**Note:** This query assumes T5 prefix is stored in `description`. The actual implementation may differ based on the fluency design decision.

## Testing Strategy

### Unit Tests

**Flow command tests (`dapple-prompt-flow/src/commands/summarize.rs`):**

```rust
#[test]
fn test_prompt_name_lookup_success() {
    let mut mappings = HashMap::new();
    mappings.insert(
        "activity-normalizer".to_string(),
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
    );

    let metadata = DappleMetadata {
        attempt_id: Some(1),
        playbook_task_id: Some(42),
        conduit_url: None,
        prompt_mappings: mappings,
    };

    let result = metadata.prompt_mappings.get("activity-normalizer");
    assert!(result.is_some());
}

#[test]
fn test_prompt_name_lookup_not_found() {
    let metadata = DappleMetadata {
        attempt_id: Some(1),
        playbook_task_id: Some(42),
        conduit_url: None,
        prompt_mappings: HashMap::new(),
    };

    let result = metadata.prompt_mappings.get("nonexistent");
    assert!(result.is_none());
}
```

**Conduit handler tests (`dapple-prompt-conduit/src/handler/summarize.rs`):**

```rust
#[test]
fn test_request_with_prompt_object_id() {
    let json = r#"{
        "text": "Hello world",
        "prompt_object_id": "550e8400-e29b-41d4-a716-446655440000",
        "playbook_task_id": 42
    }"#;
    let req: SummarizeRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.text, "Hello world");
    assert!(req.prompt_object_id.is_some());
    assert_eq!(req.playbook_task_id, Some(42));
}

#[test]
fn test_request_with_legacy_prefix() {
    let json = r#"{"text": "Hello world", "prompt_prefix": "compress:"}"#;
    let req: SummarizeRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.prompt_prefix, Some("compress:".to_string()));
    assert!(req.prompt_object_id.is_none());
}

#[test]
fn test_request_without_prompt() {
    let json = r#"{"text": "Hello world"}"#;
    let req: SummarizeRequest = serde_json::from_str(json).unwrap();
    assert!(req.prompt_prefix.is_none());
    assert!(req.prompt_object_id.is_none());
}
```

### Integration Test

```rust
#[tokio::test]
#[ignore] // Requires running conduit with summarization enabled and database
async fn test_summarize_with_prompt_name() {
    // Setup: Create a prompt with T5 prefix in database
    // (This would be done via API in real setup)

    let client = ConduitClient::new("http://localhost:8081");

    let request = SummarizeRequest {
        text: "Activity log: User logged in at 9am, opened dashboard...".into(),
        max_length: Some(50),
        min_length: None,
        prompt_object_id: Some(Uuid::parse_str("...").unwrap()),
        playbook_task_id: Some(42),
    };

    let response = client.summarize(request).await.unwrap();
    assert!(!response.summary.is_empty());
}
```

### Manual Test (Nushell)

```nushell
# 1. Create a T5 prompt in the database (via API)
# POST /api/prompts with description="normalize this activity log:"

# 2. Bind the prompt to a task via task_prompts

# 3. Run the task and test the summarize command
$activity_log | summarize --prompt "activity-log-normalizer"
```

## Files to Modify

| Crate | File | Action | Description |
|-------|------|--------|-------------|
| flow | `src/commands/summarize.rs` | Modify | Add `prompt_mappings` lookup, change `--prompt` semantics |
| flow | `src/conduit_client.rs` | Modify | Replace `prompt_prefix` with `prompt_object_id` + `playbook_task_id` |
| conduit | `src/handler/summarize.rs` | Modify | Add `prompt_object_id` and `playbook_task_id` fields, add resolution logic |
| conduit | `src/service/summarization/mod.rs` | Modify | Add `resolve_t5_prefix_from_prompt` method |
| data | `src/query/prompts.rs` | Modify | Add query for T5 prefix resolution (implementation TBD) |

## Open Questions and Risks

### Q1: How are T5 prefixes stored in Prompt objects?

**Current State:** Prompts have `fluent_messages` associations designed for LLM chat templates.

**Options:**
1. Store T5 prefix in `description` field
2. Add a dedicated `t5_prefix` column
3. Use first fluent message body as prefix
4. Add `prompt_kind` enum to distinguish prompt types

**Recommendation:** Use `description` field for now as a pragmatic solution. This can be refactored when the "fluency" design is finalized.

### Q2: Should we maintain backward compatibility with literal `--prompt` strings?

**Current Approach:** Breaking change - `--prompt` now requires a name lookup.

**Alternative:** Add `--prompt-prefix` for literal strings, keep `--prompt` for name lookup.

**Recommendation:** Proceed with breaking change. The literal string pattern was a temporary solution. Document the migration path clearly.

### Q3: What happens if `playbook_task_id` is not provided?

**Scenario:** Running `summarize` outside of a task context (e.g., testing).

**Recommendation:** Allow `playbook_task_id` to be optional. If not provided, skip overlay resolution and do a simple prompt lookup by `object_id`.

### Q4: Error handling for prompt resolution failures

**Scenarios:**
- Prompt not found
- Prompt has no T5 prefix configured
- Database connection error

**Recommendation:** Return clear error messages that help users diagnose the issue:
- "Prompt 'name' not found in prompt_mappings - check task_prompts binding"
- "Prompt UUID not found in database"
- "Prompt has no T5 prefix configured (description is empty)"

## Acceptance Criteria

- [ ] `summarize --prompt "name"` looks up prompt from `DAPPLE_METADATA.prompt_mappings`
- [ ] Error message lists available prompts when name not found
- [ ] `SummarizeRequest` sends `prompt_object_id` and `playbook_task_id` to conduit
- [ ] Conduit handler resolves T5 prefix from Prompt object
- [ ] Legacy `prompt_prefix` field still works (for testing/backward compat)
- [ ] Help text and examples updated to reflect new semantics
- [ ] Unit tests for prompt name lookup in flow
- [ ] Unit tests for request deserialization in conduit
- [ ] Integration test with actual prompt resolution

## Future Enhancements

1. **Overlay-aware prefix resolution:** Use `playbook_task_id` to apply overlay customizations to T5 prefixes (similar to how LLM prompts use overlays for engine-specific templates).

2. **Fluency design:** Formalize how T5 prefixes are stored in Prompt objects, potentially with localization support via fluent messages.

3. **Caching:** Cache resolved T5 prefixes in conduit to avoid repeated database lookups for the same prompt.

4. **Validation:** Add validation that a Prompt intended for T5 summarization has a proper prefix configured (not empty, reasonable length, etc.).
