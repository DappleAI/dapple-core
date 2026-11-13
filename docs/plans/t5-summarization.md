# T5 Dialogue Summarization Integration

**Parent Issue:** TBD

## Objective

Add T5-based dialogue summarization to dapple-prompt-conduit using Candle for local inference. This enables summarizing conversations, debrief sessions, and other dialogue-based content.

## Target Model

**Primary:** `philschmid/flan-t5-base-samsum`
- Base: FLAN-T5 (instruction-tuned)
- Fine-tuned on: SAMSum dialogue dataset
- Size: ~250M parameters
- Best for: Conversation/dialogue summarization

**Future Options (for A/B testing):**
- `google/flan-t5-base` — General instruction-following
- `google/flan-t5-large` — Larger capacity (780M params)
- `chanifrusydi/t5-dialogue-summarization` — Alternative dialogue model

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   dapple-prompt-conduit                     │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────┐     ┌─────────────────────────────┐   │
│  │ SummarizationSvc│────▶│    T5SummarizationBackend   │   │
│  │                 │     │                             │   │
│  │ summarize(text) │     │  T5ForConditionalGeneration │   │
│  │                 │     │  (Candle + candle-transformers)│   │
│  └─────────────────┘     └─────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Tasks

### 1. Create Summarization Service Module

Create `src/service/summarization/` following the pattern of `embedding/`:

```
src/service/summarization/
├── mod.rs           # SummarizationBackend trait, SummarizationService, Config
├── t5.rs            # T5SummarizationBackend implementation
└── stub.rs          # StubSummarizationBackend for testing
```

### 2. Define SummarizationBackend Trait

```rust
// src/service/summarization/mod.rs
#[async_trait]
pub trait SummarizationBackend: Send + Sync {
    /// Summarize the input text
    async fn summarize(&self, text: &str) -> Result<String, ConduitError>;

    /// Summarize with custom parameters
    async fn summarize_with_params(
        &self,
        text: &str,
        params: &SummarizationParams,
    ) -> Result<String, ConduitError>;

    /// Get the backend name for logging
    fn name(&self) -> &str;

    /// Get the model ID
    fn model_id(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct SummarizationParams {
    /// Maximum output length in tokens
    pub max_length: usize,
    /// Minimum output length in tokens
    pub min_length: usize,
    /// Temperature for sampling (0.0 = greedy)
    pub temperature: f64,
    /// Optional prompt prefix (e.g., "Summarize this dialogue: ")
    pub prompt_prefix: Option<String>,
}

impl Default for SummarizationParams {
    fn default() -> Self {
        Self {
            max_length: 150,
            min_length: 30,
            temperature: 0.0,  // Greedy decoding for deterministic summaries
            prompt_prefix: None,
        }
    }
}
```

### 3. Implement T5SummarizationBackend

Key components from Candle T5 example:

```rust
// src/service/summarization/t5.rs
use candle_core::{Device, Tensor};
use candle_transformers::models::t5::{T5ForConditionalGeneration, Config};
use tokenizers::Tokenizer;

pub struct T5SummarizationBackend {
    model: T5ForConditionalGeneration,
    tokenizer: Tokenizer,
    device: Device,
    config: T5Config,
}

impl T5SummarizationBackend {
    pub async fn new(config: &SummarizationConfig) -> Result<Self, ConduitError> {
        // 1. Set up device (CPU/CUDA/Metal)
        let device = Self::select_device()?;

        // 2. Download model from HuggingFace Hub
        let api = hf_hub::api::tokio::Api::new()?;
        let repo = api.model(config.model_id.clone());

        // 3. Load tokenizer
        let tokenizer_path = repo.get("tokenizer.json").await?;
        let tokenizer = Tokenizer::from_file(tokenizer_path)?;

        // 4. Load model config
        let config_path = repo.get("config.json").await?;
        let model_config: Config = serde_json::from_str(&std::fs::read_to_string(config_path)?)?;

        // 5. Load weights (safetensors)
        let weights_path = repo.get("model.safetensors").await?;
        let vb = candle_nn::VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)?;

        // 6. Build model
        let model = T5ForConditionalGeneration::load(vb, &model_config)?;

        Ok(Self { model, tokenizer, device, config })
    }

    fn generate(&self, input_text: &str, params: &SummarizationParams) -> Result<String, ConduitError> {
        // 1. Tokenize input
        let input = self.tokenizer.encode(input_text, true)?;
        let input_ids = Tensor::new(input.get_ids(), &self.device)?;

        // 2. Encode (run encoder once)
        let encoder_output = self.model.encode(&input_ids)?;

        // 3. Decode loop
        let mut decoder_input_ids = vec![self.config.decoder_start_token_id];

        for _ in 0..params.max_length {
            let decoder_tensor = Tensor::new(&decoder_input_ids[..], &self.device)?;
            let logits = self.model.decode(&decoder_tensor, &encoder_output)?;

            // Get next token (greedy or sampled)
            let next_token = if params.temperature == 0.0 {
                logits.argmax(D::Minus1)?.to_scalar::<u32>()?
            } else {
                // Apply temperature and sample
                self.sample_token(&logits, params.temperature)?
            };

            // Check for EOS
            if next_token == self.config.eos_token_id {
                break;
            }

            decoder_input_ids.push(next_token);
        }

        // 4. Decode tokens to text
        let summary = self.tokenizer.decode(&decoder_input_ids, true)?;
        Ok(summary)
    }
}
```

### 4. Configuration

```rust
// src/service/summarization/mod.rs
#[derive(Debug, Clone)]
pub struct SummarizationConfig {
    /// HuggingFace model ID
    pub model_id: String,
    /// Whether to fail startup if model loading fails
    pub required: bool,
    /// Optional HuggingFace token for private models
    pub hf_token: Option<String>,
    /// Default generation parameters
    pub default_params: SummarizationParams,
}

impl Default for SummarizationConfig {
    fn default() -> Self {
        Self {
            model_id: "philschmid/flan-t5-base-samsum".to_string(),
            required: false,
            hf_token: None,
            default_params: SummarizationParams::default(),
        }
    }
}

impl SummarizationConfig {
    pub fn from_env() -> Option<Self> {
        let enabled = std::env::var("SUMMARIZATION_ENABLED")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        if !enabled {
            return None;
        }

        Some(Self {
            model_id: std::env::var("SUMMARIZATION_MODEL_ID")
                .unwrap_or_else(|_| "philschmid/flan-t5-base-samsum".to_string()),
            required: std::env::var("SUMMARIZATION_REQUIRED")
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
            hf_token: std::env::var("HF_TOKEN").ok(),
            default_params: SummarizationParams {
                max_length: std::env::var("SUMMARIZATION_MAX_LENGTH")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(150),
                min_length: std::env::var("SUMMARIZATION_MIN_LENGTH")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30),
                temperature: std::env::var("SUMMARIZATION_TEMPERATURE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0),
                prompt_prefix: std::env::var("SUMMARIZATION_PROMPT_PREFIX").ok(),
            },
        })
    }
}
```

### 5. Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SUMMARIZATION_ENABLED` | `false` | Enable summarization service |
| `SUMMARIZATION_MODEL_ID` | `philschmid/flan-t5-base-samsum` | HuggingFace model ID |
| `SUMMARIZATION_REQUIRED` | `false` | Fail startup if model can't load |
| `SUMMARIZATION_MAX_LENGTH` | `150` | Max output tokens |
| `SUMMARIZATION_MIN_LENGTH` | `30` | Min output tokens |
| `SUMMARIZATION_TEMPERATURE` | `0.0` | Sampling temperature (0 = greedy) |
| `SUMMARIZATION_PROMPT_PREFIX` | (none) | Optional prefix for input |
| `HF_TOKEN` | (none) | HuggingFace token (shared with embedding) |

### 6. Integration with AppState

```rust
// src/main.rs or src/state.rs
pub struct AppState {
    // ... existing fields
    pub summarization: Option<SummarizationService>,
}

// In initialization
let summarization = if let Some(config) = SummarizationConfig::from_env() {
    match SummarizationService::with_t5(&config).await {
        Ok(svc) => {
            log::info!("Summarization service initialized with {}", config.model_id);
            Some(svc)
        }
        Err(e) if config.required => {
            return Err(e);
        }
        Err(e) => {
            log::warn!("Summarization service unavailable: {}", e);
            None
        }
    }
} else {
    None
};
```

### 7. Add API Endpoint (Optional)

```rust
// src/handler/summarize.rs
pub async fn summarize(
    State(state): State<AppState>,
    Json(req): Json<SummarizeRequest>,
) -> Result<Json<SummarizeResponse>, ApiError> {
    let svc = state.summarization
        .as_ref()
        .ok_or(ApiError::ServiceUnavailable("Summarization not enabled"))?;

    let summary = svc.summarize(&req.text).await?;

    Ok(Json(SummarizeResponse { summary }))
}
```

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `src/service/summarization/mod.rs` | Create | Trait, service, config |
| `src/service/summarization/t5.rs` | Create | T5 backend implementation |
| `src/service/summarization/stub.rs` | Create | Stub for testing |
| `src/service/mod.rs` | Modify | Export summarization module |
| `src/error.rs` | Modify | Add SummarizationError variant |
| `src/main.rs` | Modify | Initialize service, add to AppState |
| `src/handler/summarize.rs` | Create | API endpoint (optional) |
| `src/route/mod.rs` | Modify | Register endpoint (optional) |

## Testing

### Unit Tests

```rust
#[tokio::test]
async fn test_stub_summarization() {
    let svc = SummarizationService::new_stub();
    let result = svc.summarize("Hello, how are you?").await.unwrap();
    assert!(!result.is_empty());
}
```

### Integration Tests

```rust
#[tokio::test]
#[ignore] // Requires model download
async fn test_t5_summarization() {
    let config = SummarizationConfig::default();
    let svc = SummarizationService::with_t5(&config).await.unwrap();

    let dialogue = r#"
        Alice: Hey, did you finish the report?
        Bob: Yes, I sent it to the client this morning.
        Alice: Great! What was their feedback?
        Bob: They loved it and approved the budget.
    "#;

    let summary = svc.summarize(dialogue).await.unwrap();
    println!("Summary: {}", summary);

    assert!(summary.len() < dialogue.len());
    assert!(summary.contains("report") || summary.contains("client") || summary.contains("budget"));
}
```

## Acceptance Criteria

- [ ] `SummarizationBackend` trait defined
- [ ] `T5SummarizationBackend` loads `flan-t5-base-samsum` from HuggingFace
- [ ] `StubSummarizationBackend` works for testing
- [ ] Configuration via environment variables
- [ ] Service initializes gracefully when disabled
- [ ] Integration test passes with real model
- [ ] GPU acceleration works when `cuda` or `metal` feature enabled

## Future Enhancements

- **A/B Testing:** Support multiple model backends, select via config or request parameter
- **Streaming:** Return summary tokens as they're generated
- **Batch Processing:** Summarize multiple texts efficiently
- **Custom Models:** Support user-provided fine-tuned models
- **Quantization:** Use `quantized_t5` for reduced memory footprint

## References

- [Candle T5 Example](https://github.com/huggingface/candle/tree/main/candle-examples/examples/t5)
- [candle-transformers T5 docs](https://docs.rs/candle-transformers/latest/candle_transformers/models/t5/)
- [philschmid/flan-t5-base-samsum](https://huggingface.co/philschmid/flan-t5-base-samsum)
- [SAMSum Dataset](https://huggingface.co/datasets/samsum)
