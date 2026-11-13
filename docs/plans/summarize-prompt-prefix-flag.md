# Summarize Command: Custom Prompt Prefix Flag

**SHA-1**: (current HEAD)
**Status**: Ready
**Created**: 2026-01-23
**Author**: Architecture Agent

## Summary

Add a `--prompt` flag to the `summarize` Nushell command that allows users to provide custom T5 prompt prefixes instead of the hardcoded `"summarize:"`. This enables use cases like `"compress:"`, `"normalize this activity log:"`, or other task-specific prefixes that T5 models understand.

## Motivation

The T5 model family uses text-to-text prompts where the prefix instructs the model what task to perform. Currently, the summarization service hardcodes `"summarize: "` as the prefix in `T5SummarizationBackend::prepare_input()`:

```rust
// Current hardcoded behavior
fn prepare_input(&self, input: &SummarizationInput) -> String {
    let text = input.to_text();
    format!("summarize: {}", text)  // <-- Always "summarize:"
}
```

Users need flexibility to:
1. Use alternative prefixes like `"compress:"` for shorter outputs
2. Provide domain-specific instructions like `"normalize this activity log:"`
3. Experiment with different prompt formulations for their specific use cases

## Current State

### File: `dapple-prompt-conduit/src/handler/summarize.rs`

The HTTP handler accepts `text`, `max_length`, and `min_length` but no prompt customization:

```rust
#[derive(Debug, Deserialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub min_length: Option<usize>,
    // No prompt_prefix field
}
```

### File: `dapple-prompt-conduit/src/service/summarization/mod.rs`

The `SummarizationInput` enum has two variants but no prompt metadata:

```rust
#[derive(Debug, Clone)]
pub enum SummarizationInput {
    Text(String),
    Dialogue(Vec<DialogueTurn>),
}
```

The `SummarizationService::summarize_text()` method has no way to pass a custom prefix:

```rust
pub async fn summarize_text(&self, text: &str) -> Result<String, ConduitError> {
    let inputs = vec![SummarizationInput::Text(text.to_string())];
    let mut summaries = self.summarize(&inputs).await?;
    // ...
}
```

### File: `dapple-prompt-conduit/src/service/summarization/t5.rs`

The `prepare_input` method hardcodes the prefix:

```rust
fn prepare_input(&self, input: &SummarizationInput) -> String {
    let text = input.to_text();
    format!("summarize: {}", text)  // Hardcoded
}
```

### File: `dapple-prompt-flow/src/conduit_client.rs`

The client-side `SummarizeRequest` mirrors the server with no prompt field:

```rust
#[derive(Debug, Serialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    // No prompt_prefix field
}
```

### File: `dapple-prompt-flow/src/commands/summarize.rs`

The Nushell command signature has `--max-length` and `--min-length` flags but no `--prompt`:

```rust
fn signature(&self) -> Signature {
    Signature::build("summarize")
        .input_output_types(vec![
            (Type::String, Type::String),
            (Type::Any, Type::String),
        ])
        .named(
            "max-length",
            SyntaxShape::Int,
            "Maximum summary length in tokens (default: 150)",
            Some('m'),
        )
        .named(
            "min-length",
            SyntaxShape::Int,
            "Minimum summary length in tokens (default: 20)",
            Some('n'),
        )
        .category(Category::Custom("dapple".into()))
}
```

## Desired State

### Command Usage

```nushell
# Default behavior (unchanged)
$dialogue | summarize

# With custom prompt prefix
$activity_log | summarize --prompt "normalize this activity log:"

# Short flag version
$document | summarize -p "compress:"

# Combined with existing flags
$long_text | summarize --prompt "compress:" --max-length 50
```

### Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                    dapple-prompt-flow                        │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐                                        │
│  │ SummarizeCommand│  --prompt "compress:"                  │
│  │ (nu Command)    │──┐                                     │
│  └─────────────────┘  │                                     │
│                       ▼                                     │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                  ConduitClient                       │   │
│  │                                                      │   │
│  │  POST /summarize {                                   │   │
│  │    text: "...",                                      │   │
│  │    prompt_prefix: "compress:",  // NEW               │   │
│  │    max_length: 50                                    │   │
│  │  }                                                   │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                   dapple-prompt-conduit                      │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐     ┌─────────────────────────────┐   │
│  │ /summarize      │────▶│   SummarizationService      │   │
│  │ handler         │     │                             │   │
│  │                 │     │   summarize_text_with_prefix│   │
│  └─────────────────┘     └──────────────┬──────────────┘   │
│                                          │                  │
│                          ┌───────────────▼──────────────┐   │
│                          │   T5SummarizationBackend     │   │
│                          │                              │   │
│                          │   prepare_input() uses       │   │
│                          │   custom prefix or default   │   │
│                          │   "summarize:"               │   │
│                          └──────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Steps

### Step 1: Update `SummarizeRequest` in Handler (conduit)

**File**: `crates/dapple-prompt-conduit/src/handler/summarize.rs`

Add `prompt_prefix` field to the request struct:

```rust
/// Request to summarize text
#[derive(Debug, Deserialize)]
pub struct SummarizeRequest {
    /// The text to summarize
    pub text: String,
    /// Maximum output length in tokens (default: 150)
    #[serde(default)]
    pub max_length: Option<usize>,
    /// Minimum output length in tokens (default: 20)
    #[serde(default)]
    pub min_length: Option<usize>,
    /// Custom prompt prefix for T5 (default: "summarize:")
    /// Examples: "compress:", "normalize this activity log:"
    #[serde(default)]
    pub prompt_prefix: Option<String>,
}
```

Update the handler to pass the prefix:

```rust
pub async fn handle_summarize(
    State(state): State<AppState>,
    Json(req): Json<SummarizeRequest>,
) -> Result<Json<SummarizeResponse>, ConduitError> {
    log::debug!(
        "Summarize request: text_len={}, max_length={:?}, min_length={:?}, prompt_prefix={:?}",
        req.text.len(),
        req.max_length,
        req.min_length,
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

    let input_length = req.text.len();
    let summary = svc.summarize_text_with_prefix(&req.text, req.prompt_prefix.as_deref()).await?;
    let output_length = summary.len();

    // ... rest unchanged
}
```

Add tests for the new field:

```rust
#[test]
fn test_request_with_prompt_prefix() {
    let json = r#"{"text": "Hello world", "prompt_prefix": "compress:"}"#;
    let req: SummarizeRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.text, "Hello world");
    assert_eq!(req.prompt_prefix, Some("compress:".to_string()));
}

#[test]
fn test_request_without_prompt_prefix() {
    let json = r#"{"text": "Hello world"}"#;
    let req: SummarizeRequest = serde_json::from_str(json).unwrap();
    assert!(req.prompt_prefix.is_none());
}
```

### Step 2: Update `SummarizationInput` Enum (conduit)

**File**: `crates/dapple-prompt-conduit/src/service/summarization/mod.rs`

Option A (Recommended): Add prompt prefix to the enum variants:

```rust
/// Input for summarization
#[derive(Debug, Clone)]
pub enum SummarizationInput {
    /// Raw text to summarize with optional custom prefix
    Text {
        content: String,
        prompt_prefix: Option<String>,
    },
    /// Structured dialogue with speaker turns and optional custom prefix
    Dialogue {
        turns: Vec<DialogueTurn>,
        prompt_prefix: Option<String>,
    },
}

impl SummarizationInput {
    /// Create a text input with default prefix
    pub fn text(content: impl Into<String>) -> Self {
        SummarizationInput::Text {
            content: content.into(),
            prompt_prefix: None,
        }
    }

    /// Create a text input with custom prefix
    pub fn text_with_prefix(content: impl Into<String>, prefix: impl Into<String>) -> Self {
        SummarizationInput::Text {
            content: content.into(),
            prompt_prefix: Some(prefix.into()),
        }
    }

    /// Convert input to text format for the model
    pub fn to_text(&self) -> String {
        match self {
            SummarizationInput::Text { content, .. } => content.clone(),
            SummarizationInput::Dialogue { turns, .. } => turns
                .iter()
                .map(|t| format!("{}: {}", t.speaker, t.content))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Get the custom prompt prefix, if any
    pub fn prompt_prefix(&self) -> Option<&str> {
        match self {
            SummarizationInput::Text { prompt_prefix, .. } => prompt_prefix.as_deref(),
            SummarizationInput::Dialogue { prompt_prefix, .. } => prompt_prefix.as_deref(),
        }
    }
}
```

Add new service method:

```rust
impl SummarizationService {
    // ... existing methods ...

    /// Summarize a single text string with optional custom prompt prefix
    pub async fn summarize_text_with_prefix(
        &self,
        text: &str,
        prompt_prefix: Option<&str>,
    ) -> Result<String, ConduitError> {
        let input = match prompt_prefix {
            Some(prefix) => SummarizationInput::text_with_prefix(text, prefix),
            None => SummarizationInput::text(text),
        };
        let inputs = vec![input];
        let mut summaries = self.summarize(&inputs).await?;
        summaries.pop().ok_or_else(|| {
            ConduitError::SummarizationError("No summary returned for text".to_string())
        })
    }
}
```

### Step 3: Update T5 Backend to Use Custom Prefix

**File**: `crates/dapple-prompt-conduit/src/service/summarization/t5.rs`

Update `prepare_input` to respect custom prefix:

```rust
/// Default prompt prefix for summarization
const DEFAULT_PROMPT_PREFIX: &str = "summarize:";

impl T5SummarizationBackend {
    // ... existing methods ...

    /// Prepare input with prompt prefix (custom or default)
    fn prepare_input(&self, input: &SummarizationInput) -> String {
        let text = input.to_text();
        let prefix = input.prompt_prefix().unwrap_or(DEFAULT_PROMPT_PREFIX);

        // Ensure prefix ends with space for proper tokenization
        if prefix.ends_with(' ') {
            format!("{}{}", prefix, text)
        } else {
            format!("{} {}", prefix, text)
        }
    }
}
```

### Step 4: Update Stub Backend (conduit)

**File**: `crates/dapple-prompt-conduit/src/service/summarization/stub.rs`

Update to reflect the new enum structure:

```rust
fn hash_to_summary(&self, input: &SummarizationInput) -> String {
    let text = input.to_text();
    let prefix = input.prompt_prefix().unwrap_or("summarize:");

    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    prefix.hash(&mut hasher);  // Include prefix in hash for determinism
    let hash = hasher.finish();

    let word_count = text.split_whitespace().count();

    let speaker_info = match input {
        SummarizationInput::Dialogue { turns, .. } => {
            let speakers: std::collections::HashSet<_> =
                turns.iter().map(|t| &t.speaker).collect();
            format!(" ({} speakers, {} turns)", speakers.len(), turns.len())
        }
        SummarizationInput::Text { .. } => String::new(),
    };

    // ... rest of method unchanged
}
```

### Step 5: Update Client-Side `SummarizeRequest` (flow)

**File**: `crates/dapple-prompt-flow/src/conduit_client.rs`

Add the field to the client request struct:

```rust
/// Request to summarize text via conduit
#[derive(Debug, Serialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    /// Custom prompt prefix for T5 (default: "summarize:")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_prefix: Option<String>,
}
```

Update debug logging:

```rust
impl ConduitClient {
    pub async fn summarize(
        &self,
        request: SummarizeRequest,
    ) -> Result<SummarizeResponse, ConduitClientError> {
        let url = format!("{}/summarize", self.base_url);

        log::debug!(
            "Summarizing text: url={}, text_len={}, max_length={:?}, min_length={:?}, prompt_prefix={:?}",
            url,
            request.text.len(),
            request.max_length,
            request.min_length,
            request.prompt_prefix
        );

        // ... rest unchanged
    }
}
```

### Step 6: Add `--prompt` Flag to Nushell Command (flow)

**File**: `crates/dapple-prompt-flow/src/commands/summarize.rs`

Update the signature:

```rust
fn signature(&self) -> Signature {
    Signature::build("summarize")
        .input_output_types(vec![
            (Type::String, Type::String),
            (Type::Any, Type::String),
        ])
        .named(
            "max-length",
            SyntaxShape::Int,
            "Maximum summary length in tokens (default: 150)",
            Some('m'),
        )
        .named(
            "min-length",
            SyntaxShape::Int,
            "Minimum summary length in tokens (default: 20)",
            Some('n'),
        )
        .named(
            "prompt",
            SyntaxShape::String,
            "Custom T5 prompt prefix (default: \"summarize:\")",
            Some('p'),
        )
        .category(Category::Custom("dapple".into()))
}
```

Update the description:

```rust
fn extra_description(&self) -> &str {
    r#"Takes text input via pipeline and returns a concise summary.
Optimized for dialogue and conversational content.

The command calls the conduit summarization service, which uses
a T5 model fine-tuned on the SAMSum dialogue dataset.

Custom prompt prefixes allow alternative T5 tasks:
- "compress:" for shorter outputs
- "normalize this activity log:" for domain-specific processing

Environment:
- CONDUIT_URL: URL of the conduit service (default: http://localhost:8081)
- DAPPLE_METADATA: JSON with optional conduit_url override
"#
}
```

Add examples:

```rust
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
            description: "Summarize debrief responses",
            example: r#"$debrief.responses | to text | summarize"#,
            result: None,
        },
        Example {
            description: "Use custom prompt prefix for compression",
            example: r#"$long_text | summarize --prompt "compress:""#,
            result: None,
        },
        Example {
            description: "Normalize activity logs",
            example: r#"$activity_log | summarize --prompt "normalize this activity log:""#,
            result: None,
        },
    ]
}
```

Update the `run` method:

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
    let prompt_prefix: Option<String> = call.get_flag(engine_state, stack, "prompt")?;

    // Convert input to string
    let input_value = input.into_value(span)?;
    let text = value_to_string(&input_value, span)?;

    // Validate input
    if text.is_empty() {
        return Err(ShellError::GenericError {
            error: "Empty input".into(),
            msg: "Cannot summarize empty text".into(),
            span: Some(span),
            help: Some("Provide text to summarize via pipeline".into()),
            inner: vec![],
        });
    }

    // Get conduit URL from environment
    let conduit_url = get_conduit_url(engine_state, stack);

    // Build request
    let request = SummarizeRequest {
        text,
        max_length: max_length.map(|v| v as usize),
        min_length: min_length.map(|v| v as usize),
        prompt_prefix,  // NEW: Pass through custom prefix
    };

    // Execute via conduit
    let client = ConduitClient::new(&conduit_url);
    let response = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async { client.summarize(request).await })
    });

    // ... rest unchanged
}
```

Add search terms:

```rust
fn search_terms(&self) -> Vec<&str> {
    vec!["summarize", "summary", "compress", "dialogue", "t5", "prompt"]
}
```

## Files to Modify

| Crate | File | Action | Description |
|-------|------|--------|-------------|
| conduit | `src/handler/summarize.rs` | Modify | Add `prompt_prefix` to `SummarizeRequest`, update handler |
| conduit | `src/service/summarization/mod.rs` | Modify | Update `SummarizationInput` enum, add `summarize_text_with_prefix` |
| conduit | `src/service/summarization/t5.rs` | Modify | Update `prepare_input` to use custom prefix |
| conduit | `src/service/summarization/stub.rs` | Modify | Update for new `SummarizationInput` structure |
| flow | `src/conduit_client.rs` | Modify | Add `prompt_prefix` to `SummarizeRequest` |
| flow | `src/commands/summarize.rs` | Modify | Add `--prompt` flag, update examples and description |

## Verification

### Unit Tests

**conduit handler tests:**
```bash
cd crates/dapple-prompt-conduit
cargo test handler::summarize::tests
```

**conduit service tests:**
```bash
cd crates/dapple-prompt-conduit
cargo test service::summarization::tests
```

**flow command tests:**
```bash
cd crates/dapple-prompt-flow
cargo test commands::summarize::tests
```

### Integration Test

```rust
#[tokio::test]
#[ignore] // Requires running conduit with summarization enabled
async fn test_summarize_with_custom_prefix() {
    let client = ConduitClient::new("http://localhost:8081");

    let request = SummarizeRequest {
        text: "This is a long document that needs compression...".into(),
        max_length: Some(30),
        min_length: None,
        prompt_prefix: Some("compress:".into()),
    };

    let response = client.summarize(request).await.unwrap();
    assert!(!response.summary.is_empty());
}
```

### Manual Test (Nushell)

```nushell
# Start conduit with summarization enabled
SUMMARIZATION_ENABLED=true cargo run -p dapple-prompt-conduit

# In another terminal, test default behavior (unchanged)
"Alice: Hey! Bob: Hi there!" | summarize

# Test with custom prefix
"Alice: Hey! Bob: Hi there!" | summarize --prompt "compress:"

# Test with short flag
"Some long text here..." | summarize -p "normalize:"

# Test combined flags
"Long document..." | summarize --prompt "compress:" --max-length 50 --min-length 10
```

### Expected Outputs

1. Default behavior unchanged - existing scripts continue to work
2. Custom prefix is visible in conduit debug logs
3. Different prefixes produce different model outputs
4. Help text shows new `--prompt` flag with examples

## Acceptance Criteria

- [ ] `SummarizeRequest` in conduit accepts optional `prompt_prefix`
- [ ] `SummarizationInput` carries optional prefix through to backend
- [ ] `T5SummarizationBackend::prepare_input` uses custom prefix when provided
- [ ] `StubSummarizationBackend` handles new enum structure
- [ ] Client-side `SummarizeRequest` includes `prompt_prefix`
- [ ] `summarize` command accepts `--prompt` / `-p` flag
- [ ] Default behavior unchanged when no prefix specified
- [ ] Help text and examples updated
- [ ] All existing tests pass
- [ ] New tests for prefix functionality pass

## Future Enhancement: `--prompt-name` Flag

A future enhancement could add a `--prompt-name` flag that loads prompt prefixes from the database by name. This would enable:

```nushell
# Instead of hardcoding the prefix
$log | summarize --prompt-name "activity-normalizer"

# Where "activity-normalizer" is stored in the prompts table
```

**Implementation notes for future:**
1. Add `prompt_name: Option<String>` to `SummarizeRequest` (both sides)
2. In conduit handler, resolve name via `PromptService` before calling summarization
3. Priority: explicit `prompt_prefix` > resolved `prompt_name` > default `"summarize:"`
4. This requires the prompt resolution infrastructure from dapple-prompt-api

This is out of scope for the current implementation but documented for future reference.

## Open Questions

1. **Prefix validation**: Should we validate that prefixes end with `:` or ` `? The T5 model is flexible but some consistency might help users.
   - **Recommendation**: No validation. Document that prefixes should typically end with `:` but allow flexibility.

2. **Prefix in response**: Should the response echo back the prefix used? This could help debugging.
   - **Recommendation**: Not for MVP. Can add later if needed via `prompt_prefix_used: Option<String>` in response.

3. **Stub backend behavior**: Should the stub include the prefix in its deterministic output?
   - **Recommendation**: Yes, hash the prefix along with content for deterministic different outputs per prefix.
