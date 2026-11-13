# Madlib Nushell Command (Masked Language Modeling)

**Depends on:** Existing ModernBERT infrastructure in conduit

## Objective

Add a `madlib` Nushell command to dapple-prompt-flow that fills in `[MASK]` tokens using ModernBERT's masked language modeling capability. This enables playbook flows to generate contextually appropriate words for templated text.

## Command Signature

```nushell
# Fill single mask
"The user wants to [MASK] their data" | madlib
# Output: "The user wants to export their data"

# Fill multiple masks
"I need to [MASK] the [MASK] before tomorrow" | madlib
# Output: "I need to finish the report before tomorrow"

# With options
"The [MASK] is very [MASK]" | madlib --top-k 5
# Returns top 5 predictions per mask
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    dapple-prompt-flow                       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────┐                                        │
│  │  MadlibCommand  │──┐                                     │
│  │  (nu Command)   │  │                                     │
│  └─────────────────┘  │                                     │
│                       ▼                                     │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                  ConduitClient                          ││
│  │                                                         ││
│  │  POST /madlib { text, top_k }                           ││
│  └─────────────────────────────────────────────────────────┘│
│                       │                                     │
└───────────────────────┼─────────────────────────────────────┘
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                   dapple-prompt-conduit                     │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────┐     ┌─────────────────────────────┐   │
│  │ /madlib         │────▶│      MlmService             │   │
│  │ handler         │     │                             │   │
│  └─────────────────┘     │  ModernBertMlmBackend       │   │
│                          │  (MLM head, not embeddings) │   │
│                          └─────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Tasks

### 1. Add MLM Service to Conduit

The existing `ModernBertBackend` is for embeddings (mean pooling). MLM needs:
- The MLM prediction head (vocab logits per token)
- Per-token output at `[MASK]` positions
- Top-k sampling for predictions

```rust
// dapple-prompt-conduit/src/service/mlm/mod.rs

#[async_trait]
pub trait MlmBackend: Send + Sync {
    /// Fill [MASK] tokens in input text
    async fn fill_masks(&self, text: &str, top_k: usize) -> Result<MlmResult, ConduitError>;

    /// Get the mask token for this model
    fn mask_token(&self) -> &str;

    /// Get the backend name
    fn name(&self) -> &str;
}

#[derive(Debug, Clone, Serialize)]
pub struct MlmResult {
    /// The filled text (with best prediction for each mask)
    pub filled_text: String,
    /// Predictions for each mask position
    pub predictions: Vec<MaskPrediction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaskPrediction {
    /// Position in original text
    pub position: usize,
    /// Top-k predictions with scores
    pub candidates: Vec<TokenPrediction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenPrediction {
    pub token: String,
    pub score: f32,
}
```

### 2. Implement ModernBertMlmBackend

```rust
// dapple-prompt-conduit/src/service/mlm/modernbert.rs

use candle_transformers::models::modernbert::{Config, ModernBertForMaskedLM};

pub struct ModernBertMlmBackend {
    model_id: String,
    hf_token: Option<String>,
    state: OnceCell<MlmState>,
}

struct MlmState {
    model: ModernBertForMaskedLM,  // Note: MLM variant
    tokenizer: Tokenizer,
    device: Device,
    mask_token_id: u32,
}

impl ModernBertMlmBackend {
    async fn fill_masks_impl(
        &self,
        text: &str,
        top_k: usize,
    ) -> Result<MlmResult, ConduitError> {
        let state = self.get_state().await?;

        // Tokenize input
        let encoding = state.tokenizer.encode(text, true)?;
        let input_ids = encoding.get_ids();

        // Find mask positions
        let mask_positions: Vec<usize> = input_ids
            .iter()
            .enumerate()
            .filter(|(_, &id)| id == state.mask_token_id)
            .map(|(i, _)| i)
            .collect();

        if mask_positions.is_empty() {
            return Ok(MlmResult {
                filled_text: text.to_string(),
                predictions: vec![],
            });
        }

        // Create tensors
        let input_tensor = Tensor::new(input_ids, &state.device)?;
        let attention_mask = Tensor::ones_like(&input_tensor)?;

        // Run MLM forward pass
        let logits = state.model.forward(&input_tensor.unsqueeze(0)?, &attention_mask.unsqueeze(0)?)?;

        // Extract predictions for each mask position
        let mut predictions = Vec::new();
        let mut filled_ids = input_ids.to_vec();

        for &pos in &mask_positions {
            // Get logits for this position
            let pos_logits = logits.get(0)?.get(pos)?;

            // Get top-k predictions
            let (top_scores, top_indices) = pos_logits.topk(top_k)?;

            let candidates: Vec<TokenPrediction> = (0..top_k)
                .map(|i| {
                    let token_id = top_indices.get(i)?.to_scalar::<u32>()?;
                    let score = top_scores.get(i)?.to_scalar::<f32>()?;
                    let token = state.tokenizer.decode(&[token_id], false)?;
                    Ok(TokenPrediction { token, score })
                })
                .collect::<Result<Vec<_>, ConduitError>>()?;

            // Use best prediction for filled text
            if let Some(best) = candidates.first() {
                let best_id = top_indices.get(0)?.to_scalar::<u32>()?;
                filled_ids[pos] = best_id;
            }

            predictions.push(MaskPrediction {
                position: pos,
                candidates,
            });
        }

        // Decode filled text
        let filled_text = state.tokenizer.decode(&filled_ids, true)?;

        Ok(MlmResult {
            filled_text,
            predictions,
        })
    }
}
```

### 3. Add HTTP Endpoint

```rust
// dapple-prompt-conduit/src/handler/madlib.rs

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MadlibRequest {
    pub text: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize { 1 }

#[derive(Debug, Serialize, JsonSchema)]
pub struct MadlibResponse {
    pub filled_text: String,
    pub predictions: Vec<MaskPrediction>,
}

pub async fn madlib(
    State(state): State<AppState>,
    Json(req): Json<MadlibRequest>,
) -> Result<Json<MadlibResponse>, ApiError> {
    let svc = state.mlm
        .as_ref()
        .ok_or(ApiError::ServiceUnavailable("MLM not enabled"))?;

    let result = svc.fill_masks(&req.text, req.top_k).await?;

    Ok(Json(MadlibResponse {
        filled_text: result.filled_text,
        predictions: result.predictions,
    }))
}
```

### 4. Add ConduitClient Method

```rust
// dapple-prompt-flow/src/conduit_client.rs

#[derive(Debug, Serialize)]
pub struct MadlibRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct MadlibResponse {
    pub filled_text: String,
    pub predictions: Vec<MaskPrediction>,
}

#[derive(Debug, Deserialize)]
pub struct MaskPrediction {
    pub position: usize,
    pub candidates: Vec<TokenPrediction>,
}

#[derive(Debug, Deserialize)]
pub struct TokenPrediction {
    pub token: String,
    pub score: f32,
}

impl ConduitClient {
    pub async fn madlib(&self, request: MadlibRequest) -> Result<MadlibResponse, ConduitClientError> {
        let url = format!("{}/madlib", self.base_url);

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
            Err(ConduitClientError::MadlibError {
                code: status.to_string(),
                message: body,
            })
        }
    }
}
```

### 5. Create MadlibCommand

```rust
// dapple-prompt-flow/src/commands/madlib.rs

use crate::conduit_client::{ConduitClient, MadlibRequest};
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct MadlibCommand;

impl Command for MadlibCommand {
    fn name(&self) -> &str {
        "madlib"
    }

    fn signature(&self) -> Signature {
        Signature::build("madlib")
            .input_output_types(vec![
                (Type::String, Type::String),
            ])
            .named(
                "top-k",
                SyntaxShape::Int,
                "Number of predictions per mask (default: 1)",
                Some('k'),
            )
            .switch(
                "verbose",
                "Return full predictions record instead of just filled text",
                Some('v'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Fill [MASK] tokens in text using ModernBERT masked language modeling."
    }

    fn extra_description(&self) -> &str {
        r#"Takes text with [MASK] tokens and returns the text with masks filled
using contextually appropriate words predicted by ModernBERT.

The command calls the conduit MLM service for local inference.

Environment:
- CONDUIT_URL: URL of the conduit service (default: http://localhost:8081)

Examples:
  "The user wants to [MASK] their data" | madlib
  → "The user wants to export their data"

  "I [MASK] programming in [MASK]" | madlib --top-k 3 --verbose
  → Returns predictions record with top 3 candidates per mask
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["madlib", "mask", "fill", "predict", "bert", "mlm"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Fill a single mask",
                example: r#""The user wants to [MASK] their data" | madlib"#,
                result: Some(Value::test_string("The user wants to export their data")),
            },
            Example {
                description: "Fill multiple masks",
                example: r#""I need to [MASK] the [MASK] today" | madlib"#,
                result: None,
            },
            Example {
                description: "Get top-3 predictions",
                example: r#""The [MASK] is blue" | madlib --top-k 3 --verbose"#,
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
        let top_k: Option<i64> = call.get_flag(engine_state, stack, "top-k")?;
        let verbose: bool = call.has_flag(engine_state, stack, "verbose")?;

        // Convert input to string
        let input_value = input.into_value(span)?;
        let text = input_value.as_str().map_err(|_| ShellError::GenericError {
            error: "Input must be a string".into(),
            msg: "Expected string input".into(),
            span: Some(span),
            help: Some("Pipe a string containing [MASK] tokens".into()),
            inner: vec![],
        })?.to_string();

        // Get conduit URL from environment
        let conduit_url = get_conduit_url(engine_state, stack);

        // Build request
        let request = MadlibRequest {
            text,
            top_k: top_k.map(|v| v as usize),
        };

        // Execute via conduit
        let client = ConduitClient::new(&conduit_url);
        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { client.madlib(request).await })
        });

        match response {
            Ok(resp) => {
                if verbose {
                    // Return full record with predictions
                    let mut record = Record::new();
                    record.push("filled_text", Value::string(&resp.filled_text, span));

                    let predictions: Vec<Value> = resp.predictions
                        .iter()
                        .map(|p| {
                            let mut pred_record = Record::new();
                            pred_record.push("position", Value::int(p.position as i64, span));

                            let candidates: Vec<Value> = p.candidates
                                .iter()
                                .map(|c| {
                                    let mut cand = Record::new();
                                    cand.push("token", Value::string(&c.token, span));
                                    cand.push("score", Value::float(c.score as f64, span));
                                    Value::record(cand, span)
                                })
                                .collect();

                            pred_record.push("candidates", Value::list(candidates, span));
                            Value::record(pred_record, span)
                        })
                        .collect();

                    record.push("predictions", Value::list(predictions, span));
                    Ok(PipelineData::Value(Value::record(record, span), None))
                } else {
                    // Return just the filled text
                    Ok(PipelineData::Value(Value::string(resp.filled_text, span), None))
                }
            }
            Err(e) => Err(ShellError::GenericError {
                error: "Madlib failed".into(),
                msg: e.to_string(),
                span: Some(span),
                help: Some("Check that conduit is running and MLM is enabled".into()),
                inner: vec![],
            }),
        }
    }
}
```

### 6. Register Command

```rust
// dapple-prompt-flow/src/commands/mod.rs

pub mod madlib;  // Add this

pub fn register_dapple_commands(engine_state: &mut EngineState) {
    // ... existing commands ...

    // Add madlib command
    working_set.add_decl(Box::new(madlib::MadlibCommand));

    engine_state.merge_delta(working_set.render());
}
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MLM_ENABLED` | `false` | Enable MLM service |
| `MLM_MODEL_ID` | `answerdotai/ModernBERT-base` | HuggingFace model ID |
| `HF_TOKEN` | (none) | HuggingFace token (shared) |

### AppState Integration

```rust
pub struct AppState {
    // ... existing fields
    pub mlm: Option<MlmService>,
}
```

## Files to Create/Modify

| Crate | File | Action | Description |
|-------|------|--------|-------------|
| conduit | `src/service/mlm/mod.rs` | Create | MlmBackend trait, MlmService |
| conduit | `src/service/mlm/modernbert.rs` | Create | ModernBertMlmBackend |
| conduit | `src/service/mlm/stub.rs` | Create | Stub for testing |
| conduit | `src/service/mod.rs` | Modify | Export mlm module |
| conduit | `src/handler/madlib.rs` | Create | HTTP endpoint |
| conduit | `src/route/mod.rs` | Modify | Register `/madlib` route |
| flow | `src/conduit_client.rs` | Modify | Add `madlib` method |
| flow | `src/commands/madlib.rs` | Create | Nu command |
| flow | `src/commands/mod.rs` | Modify | Register command |

## Testing

### Unit Test

```rust
#[test]
fn test_madlib_command_signature() {
    let cmd = MadlibCommand;
    assert_eq!(cmd.name(), "madlib");

    let sig = cmd.signature();
    assert!(sig.get_flags().iter().any(|f| f.long == "top-k"));
    assert!(sig.get_flags().iter().any(|f| f.long == "verbose"));
}
```

### Integration Test

```rust
#[tokio::test]
#[ignore] // Requires running conduit with MLM enabled
async fn test_madlib_via_conduit() {
    let client = ConduitClient::new("http://localhost:8081");

    let request = MadlibRequest {
        text: "The capital of France is [MASK].".into(),
        top_k: Some(3),
    };

    let response = client.madlib(request).await.unwrap();
    assert!(response.filled_text.contains("Paris"));
    assert_eq!(response.predictions.len(), 1);
    assert_eq!(response.predictions[0].candidates.len(), 3);
}
```

### Manual Test (Nushell)

```nushell
# Start conduit with MLM enabled
MLM_ENABLED=true cargo run -p dapple-prompt-conduit

# Test basic fill
"The user wants to [MASK] their data" | madlib

# Test with verbose output
"I [MASK] programming" | madlib --verbose

# Test top-k
"The [MASK] is very [MASK]" | madlib --top-k 5 --verbose
```

## Acceptance Criteria

- [ ] `MlmBackend` trait defined with `fill_masks` method
- [ ] `ModernBertMlmBackend` loads model and performs MLM inference
- [ ] `POST /madlib` endpoint exists in conduit
- [ ] `ConduitClient::madlib` method works
- [ ] `madlib` command registered in Nushell
- [ ] Default mode returns filled text string
- [ ] `--verbose` flag returns full predictions record
- [ ] `--top-k` controls number of candidates
- [ ] Graceful error when no `[MASK]` tokens in input

## Use Cases

### 1. Dynamic Template Filling

```nushell
# Fill in contextual blanks
let template = "Dear [MASK], Thank you for your [MASK]."
$template | madlib
```

### 2. Generate Variations

```nushell
# Get multiple options
"The [MASK] solution would be to [MASK] the data" | madlib --top-k 5 --verbose | get predictions
```

### 3. Context-Aware Completion

```nushell
# Fill based on surrounding context
let context = $"Based on the user's request to ($action), the next step is to [MASK]."
$context | madlib
```

## Notes

- ModernBERT uses `[MASK]` as its mask token (same as BERT)
- MLM works best with 1-3 masks; too many masks degrades quality
- Predictions are contextual - surrounding words influence the fill
- This is different from embeddings - we use the MLM head, not mean pooling
