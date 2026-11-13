# Summarize Nushell Command

**Depends on:** `docs/plans/t5-summarization.md` (T5 service must be implemented first)

## Objective

Add a `summarize` Nushell command to dapple-prompt-flow that calls the conduit summarization service. This enables playbook flows to compress text (dialogue, documents, debrief responses) into concise summaries.

## Command Signature

```nushell
# Pipe text in
$dialogue | summarize

# With optional parameters
$content | summarize --max-length 100 --min-length 20

# Store result
let summary = ($asset_content | summarize)
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    dapple-prompt-flow                       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────┐                                        │
│  │ SummarizeCommand│──┐                                     │
│  │ (nu Command)    │  │                                     │
│  └─────────────────┘  │                                     │
│                       ▼                                     │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                  ConduitClient                       │   │
│  │                                                      │   │
│  │  POST /summarize { text, max_length, min_length }   │   │
│  └─────────────────────────────────────────────────────┘   │
│                       │                                     │
└───────────────────────┼─────────────────────────────────────┘
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                   dapple-prompt-conduit                     │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────┐     ┌─────────────────────────────┐   │
│  │ /summarize      │────▶│   SummarizationService      │   │
│  │ handler         │     │                             │   │
│  └─────────────────┘     │   T5SummarizationBackend    │   │
│                          └─────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Tasks

### 1. Add Summarize Endpoint to Conduit (prerequisite)

This is covered in `docs/plans/t5-summarization.md`, but we need the HTTP endpoint:

```rust
// dapple-prompt-conduit/src/handler/summarize.rs
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub min_length: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SummarizeResponse {
    pub summary: String,
    pub input_length: usize,
    pub output_length: usize,
}

pub async fn summarize(
    State(state): State<AppState>,
    Json(req): Json<SummarizeRequest>,
) -> Result<Json<SummarizeResponse>, ApiError> {
    let svc = state.summarization
        .as_ref()
        .ok_or(ApiError::ServiceUnavailable("Summarization not enabled"))?;

    let params = SummarizationParams {
        max_length: req.max_length.unwrap_or(150),
        min_length: req.min_length.unwrap_or(30),
        ..Default::default()
    };

    let summary = svc.summarize_with_params(&req.text, &params).await?;

    Ok(Json(SummarizeResponse {
        input_length: req.text.len(),
        output_length: summary.len(),
        summary,
    }))
}
```

### 2. Add ConduitClient Method

```rust
// dapple-prompt-flow/src/conduit_client.rs

#[derive(Debug, Serialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SummarizeResponse {
    pub summary: String,
    pub input_length: usize,
    pub output_length: usize,
}

impl ConduitClient {
    pub async fn summarize(&self, request: SummarizeRequest) -> Result<SummarizeResponse, ConduitClientError> {
        let url = format!("{}/summarize", self.base_url);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| ConduitClientError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            response.json().await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::SummarizeError {
                code: status.to_string(),
                message: body,
            })
        }
    }
}
```

### 3. Create SummarizeCommand

```rust
// dapple-prompt-flow/src/commands/summarize.rs

use crate::conduit_client::{ConduitClient, SummarizeRequest};
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct SummarizeCommand;

impl Command for SummarizeCommand {
    fn name(&self) -> &str {
        "summarize"
    }

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
                "Minimum summary length in tokens (default: 30)",
                Some('n'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Summarize text using the T5 dialogue summarization model."
    }

    fn extra_description(&self) -> &str {
        r#"Takes text input via pipeline and returns a concise summary.
Optimized for dialogue and conversational content.

The command calls the conduit summarization service, which uses
a T5 model fine-tuned on the SAMSum dialogue dataset.

Environment:
- CONDUIT_URL: URL of the conduit service (default: http://localhost:8081)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["summarize", "summary", "compress", "dialogue", "t5"]
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
                description: "Summarize debrief responses",
                example: r#"$debrief.responses | to text | summarize"#,
                result: None,
            },
        ]
    }

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

        // Convert input to string
        let input_value = input.into_value(span)?;
        let text = value_to_string(&input_value, span)?;

        // Get conduit URL from environment
        let conduit_url = get_conduit_url(engine_state, stack);

        // Build request
        let request = SummarizeRequest {
            text,
            max_length: max_length.map(|v| v as usize),
            min_length: min_length.map(|v| v as usize),
        };

        // Execute via conduit
        let client = ConduitClient::new(&conduit_url);
        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { client.summarize(request).await })
        });

        match response {
            Ok(resp) => Ok(PipelineData::Value(Value::string(resp.summary, span), None)),
            Err(e) => Err(ShellError::GenericError {
                error: "Summarization failed".into(),
                msg: e.to_string(),
                span: Some(span),
                help: Some("Check that conduit is running and summarization is enabled".into()),
                inner: vec![],
            }),
        }
    }
}

fn value_to_string(value: &Value, span: Span) -> Result<String, ShellError> {
    match value {
        Value::String { val, .. } => Ok(val.clone()),
        Value::List { vals, .. } => {
            // Join list elements with newlines
            let strings: Result<Vec<String>, _> = vals
                .iter()
                .map(|v| value_to_string(v, span))
                .collect();
            Ok(strings?.join("\n"))
        }
        Value::Record { val, .. } => {
            // Convert record to readable text
            let mut parts = Vec::new();
            for (k, v) in val.iter() {
                let v_str = value_to_string(v, span)?;
                parts.push(format!("{}: {}", k, v_str));
            }
            Ok(parts.join("\n"))
        }
        _ => Ok(value.to_expanded_string("", &nu_protocol::Config::default())),
    }
}

fn get_conduit_url(engine_state: &EngineState, stack: &Stack) -> String {
    // Try DAPPLE_METADATA first
    if let Some((_, meta_value)) = stack.get_env_var_insensitive(engine_state, "DAPPLE_METADATA") {
        if let Value::Record { val, .. } = meta_value {
            if let Some(Value::String { val: url, .. }) = val.get("conduit_url") {
                return url.clone();
            }
        }
    }

    // Fall back to CONDUIT_URL env var or default
    std::env::var("CONDUIT_URL").unwrap_or_else(|_| "http://localhost:8081".to_string())
}
```

### 4. Register Command

```rust
// dapple-prompt-flow/src/commands/mod.rs

pub mod summarize;  // Add this

use nu_protocol::engine::{EngineState, StateWorkingSet};

pub fn register_dapple_commands(engine_state: &mut EngineState) {
    let mut working_set = StateWorkingSet::new(engine_state);

    // Existing commands
    working_set.add_decl(Box::new(prompt::PromptCommand));
    working_set.add_decl(Box::new(prompt_send::PromptSend));
    working_set.add_decl(Box::new(prompt_retry::PromptRetry));
    working_set.add_decl(Box::new(prompt_reset::PromptReset));

    // Add summarize command
    working_set.add_decl(Box::new(summarize::SummarizeCommand));

    engine_state.merge_delta(working_set.render());
}
```

## Files to Create/Modify

| Crate | File | Action | Description |
|-------|------|--------|-------------|
| conduit | `src/handler/summarize.rs` | Create | HTTP endpoint |
| conduit | `src/route/mod.rs` | Modify | Register `/summarize` route |
| flow | `src/conduit_client.rs` | Modify | Add `summarize` method |
| flow | `src/commands/summarize.rs` | Create | Nu command implementation |
| flow | `src/commands/mod.rs` | Modify | Register command |

## Testing

### Unit Test (Flow)

```rust
#[test]
fn test_summarize_command_signature() {
    let cmd = SummarizeCommand;
    assert_eq!(cmd.name(), "summarize");

    let sig = cmd.signature();
    assert!(sig.get_flags().iter().any(|f| f.long == "max-length"));
    assert!(sig.get_flags().iter().any(|f| f.long == "min-length"));
}
```

### Integration Test

```rust
#[tokio::test]
#[ignore] // Requires running conduit with summarization enabled
async fn test_summarize_via_conduit() {
    let client = ConduitClient::new("http://localhost:8081");

    let request = SummarizeRequest {
        text: "Alice: Did you finish the report?\nBob: Yes, sent it this morning.".into(),
        max_length: Some(50),
        min_length: None,
    };

    let response = client.summarize(request).await.unwrap();
    assert!(!response.summary.is_empty());
    assert!(response.summary.len() < response.input_length);
}
```

### Manual Test (Nushell)

```nushell
# Start conduit with summarization enabled
SUMMARIZATION_ENABLED=true cargo run -p dapple-prompt-conduit

# In another terminal, test the command
"Alice: Hey, how are you?\nBob: Great, thanks for asking!" | summarize

# With parameters
open transcript.txt | summarize --max-length 100
```

## Acceptance Criteria

- [ ] `POST /summarize` endpoint exists in conduit
- [ ] `ConduitClient::summarize` method works
- [ ] `summarize` command registered in Nushell
- [ ] Pipeline input works (`$text | summarize`)
- [ ] `--max-length` and `--min-length` flags work
- [ ] Graceful error when conduit unavailable
- [ ] Graceful error when summarization disabled

## Use Cases

### 1. Summarize Debrief Responses

```nushell
# After user completes debrief, summarize their responses
let summary = ($debrief.responses | to text | summarize)
$summary | save "data.debrief_summary"
```

### 2. Compress Asset for Context

```nushell
# Instead of sending full document to LLM
let doc = (fetch_asset $asset_id)
let compressed = ($doc | summarize --max-length 200)
{ context: $compressed } | prompt send "analyze"
```

### 3. Summarize Conversation History

```nushell
# Before next step, summarize what happened
let history = ($journey.messages | each { |m| $"($m.role): ($m.content)" } | str join "\n")
let summary = ($history | summarize)
```
