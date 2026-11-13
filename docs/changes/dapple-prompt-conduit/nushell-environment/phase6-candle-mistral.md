# Nushell Environment Setup - Phase 6: Candle Mistral Inference

This document covers integrating local LLM inference via Candle with Mistral models, enabling fully offline prompt execution without external API dependencies.

## Why Local Mistral Inference

1. **Privacy** — Sensitive prompts never leave the server
2. **Cost** — No per-token API charges after hardware investment
3. **Latency** — Eliminate network round-trips for local deployments
4. **Availability** — No dependence on external service uptime
5. **Control** — Full control over model versions and behavior

## Target Models

| Model | Parameters | VRAM (FP16) | VRAM (Q4) | Context | Notes |
|-------|------------|-------------|-----------|---------|-------|
| **Mistral 7B Instruct v0.3** | 7B | ~14GB | ~4GB | 32K | Primary target |
| Mistral 7B Base v0.3 | 7B | ~14GB | ~4GB | 32K | For fine-tuning |
| Mixtral 8x7B Instruct | 47B (12B active) | ~90GB | ~26GB | 32K | Higher quality, needs multi-GPU |

Primary target: [mistralai/Mistral-7B-Instruct-v0.3](https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3)

---

## Phase 6 Overview

| Component | Description |
|-----------|-------------|
| **6A** | Candle Mistral model loader |
| **6B** | Text generation with KV cache |
| **6C** | LlmBackend implementation with streaming |
| **6D** | Quantization support (GGUF) |

---

## Phase 6A: Candle Mistral Model Loader

### Add Dependencies

Update `dapple-prompt-conduit/Cargo.toml`:

```toml
[dependencies]
# ... existing deps (candle-core, candle-nn, candle-transformers from Phase 5) ...

# Quantization support
candle-quantized = "0.8"

# Sampling
rand = "0.8"
```

### Model Loading

Create `dapple-prompt-conduit/src/service/llm/mistral.rs`:

```rust
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::mistral::{Config, Model as MistralModel};
use hf_hub::{api::sync::Api, Repo, RepoType};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tokenizers::Tokenizer;

use crate::error::ConduitError;

/// Configuration for Mistral model
#[derive(Debug, Clone)]
pub struct MistralConfig {
    pub model_id: String,
    pub revision: Option<String>,
    pub device: DeviceConfig,
    pub dtype: DTypeConfig,
    pub max_context_length: usize,
}

#[derive(Debug, Clone)]
pub enum DeviceConfig {
    Cpu,
    Cuda(usize),
    Metal,
}

#[derive(Debug, Clone)]
pub enum DTypeConfig {
    F32,
    F16,
    BF16,
}

impl Default for MistralConfig {
    fn default() -> Self {
        Self {
            model_id: "mistralai/Mistral-7B-Instruct-v0.3".to_string(),
            revision: None,
            device: DeviceConfig::Cpu,
            dtype: DTypeConfig::F16,
            max_context_length: 8192, // Conservative default
        }
    }
}

impl MistralConfig {
    pub fn from_env() -> Self {
        Self {
            model_id: std::env::var("MISTRAL_MODEL_ID")
                .unwrap_or_else(|_| "mistralai/Mistral-7B-Instruct-v0.3".to_string()),
            revision: std::env::var("MISTRAL_REVISION").ok(),
            device: DeviceConfig::from_env(),
            dtype: DTypeConfig::from_env(),
            max_context_length: std::env::var("MISTRAL_MAX_CONTEXT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8192),
        }
    }
}

impl DeviceConfig {
    pub fn from_env() -> Self {
        match std::env::var("MISTRAL_DEVICE").as_deref() {
            Ok("cuda") | Ok("cuda:0") => Self::Cuda(0),
            Ok(s) if s.starts_with("cuda:") => {
                let idx = s[5..].parse().unwrap_or(0);
                Self::Cuda(idx)
            }
            Ok("metal") => Self::Metal,
            _ => {
                // Auto-detect
                if cfg!(feature = "cuda") {
                    Self::Cuda(0)
                } else if cfg!(feature = "metal") {
                    Self::Metal
                } else {
                    Self::Cpu
                }
            }
        }
    }

    pub fn to_device(&self) -> Result<Device, ConduitError> {
        match self {
            Self::Cpu => Ok(Device::Cpu),
            Self::Cuda(ordinal) => Device::new_cuda(*ordinal)
                .map_err(|e| ConduitError::LlmError(format!("CUDA init failed: {}", e))),
            Self::Metal => Device::new_metal(0)
                .map_err(|e| ConduitError::LlmError(format!("Metal init failed: {}", e))),
        }
    }
}

impl DTypeConfig {
    pub fn from_env() -> Self {
        match std::env::var("MISTRAL_DTYPE").as_deref() {
            Ok("f32") => Self::F32,
            Ok("f16") => Self::F16,
            Ok("bf16") => Self::BF16,
            _ => Self::F16, // Good balance of speed/precision
        }
    }

    pub fn to_dtype(&self) -> DType {
        match self {
            Self::F32 => DType::F32,
            Self::F16 => DType::F16,
            Self::BF16 => DType::BF16,
        }
    }
}

/// Download model files from HuggingFace Hub
pub fn download_mistral_model(config: &MistralConfig) -> Result<PathBuf, ConduitError> {
    log::info!("Downloading Mistral model: {}", config.model_id);

    let api = Api::new()
        .map_err(|e| ConduitError::LlmError(format!("HF Hub API init failed: {}", e)))?;

    let repo = match &config.revision {
        Some(rev) => api.repo(Repo::with_revision(
            config.model_id.clone(),
            RepoType::Model,
            rev.clone(),
        )),
        None => api.repo(Repo::new(config.model_id.clone(), RepoType::Model)),
    };

    // Download required files
    let files = [
        "config.json",
        "tokenizer.json",
        "tokenizer_config.json",
    ];

    for file in &files {
        repo.get(file)
            .map_err(|e| ConduitError::LlmError(format!("Failed to download {}: {}", file, e)))?;
    }

    // Model weights - try safetensors first, then pytorch
    let model_path = repo
        .get("model.safetensors")
        .or_else(|_| repo.get("pytorch_model.bin"))
        .or_else(|_| {
            // Sharded model
            repo.get("model.safetensors.index.json")
        })
        .map_err(|e| ConduitError::LlmError(format!("Failed to download model weights: {}", e)))?;

    Ok(model_path.parent().unwrap().to_path_buf())
}
```

---

## Phase 6B: Text Generation with KV Cache

### Generation State

```rust
/// Manages generation state including KV cache
pub struct GenerationState {
    /// Past key-value cache for efficient generation
    kv_cache: Vec<(Tensor, Tensor)>,
    /// Current position in sequence
    position: usize,
    /// Generated tokens so far
    generated_tokens: Vec<u32>,
}

impl GenerationState {
    pub fn new() -> Self {
        Self {
            kv_cache: Vec::new(),
            position: 0,
            generated_tokens: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.kv_cache.clear();
        self.position = 0;
        self.generated_tokens.clear();
    }
}
```

### Sampling

```rust
use rand::distributions::{Distribution, WeightedIndex};
use rand::SeedableRng;

/// Sampling parameters
#[derive(Debug, Clone)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: usize,
    pub max_tokens: usize,
    pub stop_sequences: Vec<String>,
    pub seed: Option<u64>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            max_tokens: 2048,
            stop_sequences: vec!["</s>".to_string(), "[/INST]".to_string()],
            seed: None,
        }
    }
}

/// Sample next token from logits
fn sample_token(
    logits: &Tensor,
    params: &SamplingParams,
    rng: &mut impl rand::Rng,
) -> Result<u32, ConduitError> {
    let logits = logits.to_vec1::<f32>()
        .map_err(|e| ConduitError::LlmError(e.to_string()))?;

    // Apply temperature
    let scaled: Vec<f32> = if params.temperature > 0.0 {
        logits.iter().map(|&x| x / params.temperature).collect()
    } else {
        // Greedy: just return argmax
        let (idx, _) = logits
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .unwrap();
        return Ok(idx as u32);
    };

    // Apply top-k
    let mut indexed: Vec<(usize, f32)> = scaled.into_iter().enumerate().collect();
    indexed.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap());
    indexed.truncate(params.top_k);

    // Apply top-p (nucleus sampling)
    let max_logit = indexed[0].1;
    let probs: Vec<f32> = indexed
        .iter()
        .map(|(_, logit)| (logit - max_logit).exp())
        .collect();
    let sum: f32 = probs.iter().sum();
    let probs: Vec<f32> = probs.iter().map(|p| p / sum).collect();

    let mut cumsum = 0.0;
    let mut nucleus_size = probs.len();
    for (i, p) in probs.iter().enumerate() {
        cumsum += p;
        if cumsum >= params.top_p {
            nucleus_size = i + 1;
            break;
        }
    }

    let probs = &probs[..nucleus_size];
    let indexed = &indexed[..nucleus_size];

    // Sample
    let dist = WeightedIndex::new(probs)
        .map_err(|e| ConduitError::LlmError(format!("Sampling failed: {}", e)))?;
    let sampled_idx = dist.sample(rng);

    Ok(indexed[sampled_idx].0 as u32)
}
```

### Generation Loop

```rust
/// Generator that produces tokens one at a time
pub struct MistralGenerator {
    model: MistralModel,
    tokenizer: Tokenizer,
    device: Device,
    config: MistralConfig,
    state: GenerationState,
}

impl MistralGenerator {
    pub fn new(config: MistralConfig) -> Result<Self, ConduitError> {
        let device = config.device.to_device()?;
        let model_path = download_mistral_model(&config)?;

        // Load tokenizer
        let tokenizer_path = model_path.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| ConduitError::LlmError(format!("Tokenizer load failed: {}", e)))?;

        // Load model config
        let config_path = model_path.join("config.json");
        let model_config: Config = serde_json::from_str(
            &std::fs::read_to_string(&config_path)
                .map_err(|e| ConduitError::LlmError(e.to_string()))?,
        )
        .map_err(|e| ConduitError::LlmError(e.to_string()))?;

        // Load model weights
        let weights_path = model_path.join("model.safetensors");
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(
                &[weights_path],
                config.dtype.to_dtype(),
                &device,
            )
            .map_err(|e| ConduitError::LlmError(e.to_string()))?
        };

        let model = MistralModel::new(&model_config, vb)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?;

        log::info!("Mistral model loaded on {:?}", config.device);

        Ok(Self {
            model,
            tokenizer,
            device,
            config,
            state: GenerationState::new(),
        })
    }

    /// Format prompt using Mistral instruct template
    fn format_prompt(&self, user_message: &str, system_prompt: Option<&str>) -> String {
        match system_prompt {
            Some(sys) => format!("[INST] {}\n\n{} [/INST]", sys, user_message),
            None => format!("[INST] {} [/INST]", user_message),
        }
    }

    /// Encode text to token IDs
    fn encode(&self, text: &str) -> Result<Vec<u32>, ConduitError> {
        let encoding = self.tokenizer.encode(text, true)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?;
        Ok(encoding.get_ids().to_vec())
    }

    /// Decode token ID to text
    fn decode(&self, token_id: u32) -> Result<String, ConduitError> {
        self.tokenizer.decode(&[token_id], false)
            .map_err(|e| ConduitError::LlmError(e.to_string()))
    }

    /// Generate next token
    fn next_token(
        &mut self,
        input_ids: &Tensor,
        params: &SamplingParams,
        rng: &mut impl rand::Rng,
    ) -> Result<u32, ConduitError> {
        let logits = self.model.forward(input_ids, self.state.position, &mut self.state.kv_cache)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?;

        // Get logits for last position
        let last_logits = logits
            .i((.., logits.dim(1)? - 1, ..))?
            .squeeze(1)?;

        let token = sample_token(&last_logits, params, rng)?;
        self.state.position += input_ids.dim(1)?;
        self.state.generated_tokens.push(token);

        Ok(token)
    }

    /// Check if generation should stop
    fn should_stop(&self, params: &SamplingParams, decoded: &str) -> bool {
        // Check max tokens
        if self.state.generated_tokens.len() >= params.max_tokens {
            return true;
        }

        // Check stop sequences
        for stop in &params.stop_sequences {
            if decoded.ends_with(stop) {
                return true;
            }
        }

        // Check EOS token
        if let Some(&last) = self.state.generated_tokens.last() {
            if last == 2 { // Mistral EOS token
                return true;
            }
        }

        false
    }
}
```

---

## Phase 6C: LlmBackend Implementation

### Streaming Backend

```rust
use async_trait::async_trait;
use futures::stream::{self, Stream, StreamExt};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::service::llm::{LlmBackend, TokenStream};
use crate::service::engine::EngineConfig;
use crate::service::template::RenderedPrompt;

/// Mistral backend using local Candle inference
pub struct MistralBackend {
    generator: Arc<Mutex<MistralGenerator>>,
    config: MistralConfig,
}

impl MistralBackend {
    pub fn new(config: MistralConfig) -> Result<Self, ConduitError> {
        let generator = MistralGenerator::new(config.clone())?;
        Ok(Self {
            generator: Arc::new(Mutex::new(generator)),
            config,
        })
    }

    /// Lazy initialization - create generator on first use
    pub fn new_lazy(config: MistralConfig) -> Self {
        Self {
            generator: Arc::new(Mutex::new(MistralGenerator::new_uninit())),
            config,
        }
    }
}

#[async_trait]
impl LlmBackend for MistralBackend {
    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        engine: &EngineConfig,
    ) -> Result<TokenStream, ConduitError> {
        let generator = self.generator.clone();

        // Extract params from engine config
        let params = SamplingParams {
            temperature: engine.temperature.unwrap_or(0.7),
            top_p: engine.top_p.unwrap_or(0.9),
            max_tokens: engine.max_tokens.unwrap_or(2048),
            ..Default::default()
        };

        // Encode prompt
        let prompt_text = prompt.to_text()?;
        let gen = generator.lock().await;
        let formatted = gen.format_prompt(&prompt_text, prompt.system_prompt.as_deref());
        let input_ids = gen.encode(&formatted)?;
        drop(gen);

        // Create async stream that generates tokens
        let stream = stream::unfold(
            (generator, input_ids, params, String::new()),
            move |(gen, input_ids, params, mut decoded)| async move {
                let mut gen_guard = gen.lock().await;

                // First iteration: process full input
                // Subsequent: process single token
                let input_tensor = if gen_guard.state.position == 0 {
                    Tensor::new(&input_ids[..], &gen_guard.device).ok()?
                        .unsqueeze(0).ok()?
                } else {
                    let last_token = *gen_guard.state.generated_tokens.last()?;
                    Tensor::new(&[last_token], &gen_guard.device).ok()?
                        .unsqueeze(0).ok()?
                };

                let mut rng = rand::rngs::StdRng::from_entropy();
                let token = gen_guard.next_token(&input_tensor, &params, &mut rng).ok()?;
                let token_text = gen_guard.decode(token).ok()?;
                decoded.push_str(&token_text);

                if gen_guard.should_stop(&params, &decoded) {
                    // Clean up
                    gen_guard.state.clear();
                    return None;
                }

                drop(gen_guard);
                Some((Ok(token_text), (gen, input_ids, params, decoded)))
            },
        );

        Ok(Box::pin(stream))
    }

    fn supports(&self, engine: &EngineConfig) -> bool {
        engine.maker_key == "mistral" || engine.maker_key == "local"
    }
}
```

### Register Backend

Update `dapple-prompt-conduit/src/service/llm/mod.rs`:

```rust
mod anthropic;
mod mistral;
mod stub;

pub use anthropic::AnthropicBackend;
pub use mistral::{MistralBackend, MistralConfig};
pub use stub::StubBackend;

// ... in LlmService::new() ...

impl LlmService {
    pub fn new(config: &LlmConfig) -> Self {
        let mut backends: Vec<Box<dyn LlmBackend>> = Vec::new();

        // Add Anthropic backend if API key is available
        if let Some(api_key) = &config.anthropic_api_key {
            log::info!("Anthropic backend configured");
            backends.push(Box::new(AnthropicBackend::new(api_key.clone())));
        }

        // Add Mistral backend if configured
        if config.mistral_enabled {
            log::info!("Mistral local backend configured");
            let mistral_config = MistralConfig::from_env();
            match MistralBackend::new(mistral_config) {
                Ok(backend) => backends.push(Box::new(backend)),
                Err(e) => log::warn!("Failed to initialize Mistral backend: {}", e),
            }
        }

        let fallback = Box::new(StubBackend::new());
        Self { backends, fallback }
    }
}
```

---

## Phase 6D: Quantization Support (GGUF)

For running on consumer hardware, support quantized models via GGUF format.

### GGUF Loader

```rust
use candle_quantized::gguf_file::GgufFile;
use candle_quantized::ggml_model::ModelWeights;

pub struct MistralQuantizedBackend {
    weights: ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
    config: MistralConfig,
}

impl MistralQuantizedBackend {
    pub fn from_gguf(gguf_path: &Path, config: MistralConfig) -> Result<Self, ConduitError> {
        let device = config.device.to_device()?;

        // Load GGUF file
        let mut file = std::fs::File::open(gguf_path)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?;
        let gguf = GgufFile::read(&mut file)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?;

        let weights = ModelWeights::from_gguf(gguf, &device)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?;

        // Tokenizer from HF Hub (GGUF doesn't include it)
        let model_path = download_mistral_model(&config)?;
        let tokenizer_path = model_path.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?;

        log::info!("Loaded quantized Mistral model from GGUF");

        Ok(Self {
            weights,
            tokenizer,
            device,
            config,
        })
    }
}
```

### Quantization Levels

| Format | Bits | Size (7B) | Quality | Speed |
|--------|------|-----------|---------|-------|
| F16 | 16 | ~14GB | Best | Baseline |
| Q8_0 | 8 | ~7GB | Excellent | 1.5x |
| Q6_K | 6 | ~5.5GB | Very Good | 1.8x |
| Q5_K_M | 5 | ~4.5GB | Good | 2x |
| Q4_K_M | 4 | ~4GB | Acceptable | 2.2x |
| Q4_0 | 4 | ~3.8GB | Lower | 2.5x |

Recommended: **Q5_K_M** for best quality/size tradeoff on consumer hardware.

---

## Implementation Order

1. **Phase 6A: Model Loader**
   - Add Candle quantized dependency
   - Implement model downloading
   - Device configuration
   - Basic model loading

2. **Phase 6B: Text Generation**
   - Implement KV cache management
   - Add sampling (temperature, top-p, top-k)
   - Generation loop with stop conditions
   - Mistral instruct template formatting

3. **Phase 6C: LlmBackend Integration**
   - Implement `LlmBackend` trait
   - Token streaming via async generator
   - Register in LlmService
   - Engine config routing (`maker_key = "mistral"`)

4. **Phase 6D: Quantization**
   - GGUF loader
   - Support common quant formats
   - Auto-detect quantized vs full models

---

## Files to Create/Modify

### New Files
- `dapple-prompt-conduit/src/service/llm/mistral.rs`
- `dapple-prompt-conduit/src/service/llm/mistral_quantized.rs` (optional)

### Modified Files
- `dapple-prompt-conduit/Cargo.toml` (add candle-quantized, rand)
- `dapple-prompt-conduit/src/service/llm/mod.rs` (register backend)
- `dapple-prompt-conduit/src/config.rs` (Mistral config)

---

## Configuration

### Environment Variables

```bash
# Mistral model config
MISTRAL_ENABLED=true
MISTRAL_MODEL_ID=mistralai/Mistral-7B-Instruct-v0.3
MISTRAL_DEVICE=cuda:0            # cpu, cuda:0, metal
MISTRAL_DTYPE=f16                # f32, f16, bf16
MISTRAL_MAX_CONTEXT=8192

# Quantized model (alternative to MODEL_ID)
MISTRAL_GGUF_PATH=/models/mistral-7b-instruct-v0.3.Q5_K_M.gguf

# HuggingFace (for gated models)
HF_TOKEN=hf_...
```

### EngineConfig for Mistral

```sql
INSERT INTO rev.engines (maker_key, model_key, display_name, context_length)
VALUES
    ('mistral', 'mistral-7b-instruct', 'Mistral 7B Instruct (Local)', 32768),
    ('local', 'mistral-7b-q5', 'Mistral 7B Q5 (Local)', 32768);
```

---

## Testing

### Unit Tests
- [ ] Tokenization matches HuggingFace reference
- [ ] KV cache shapes are correct
- [ ] Sampling produces valid token IDs
- [ ] Stop sequences detected correctly

### Integration Tests
- [ ] Model downloads successfully
- [ ] Generation produces coherent text
- [ ] Streaming yields tokens incrementally
- [ ] Quantized models load and run

### Quality Tests
- [ ] Compare outputs with vLLM/llama.cpp reference
- [ ] Measure perplexity on held-out data
- [ ] Verify instruction following

---

## Hardware Requirements

| Configuration | VRAM | RAM | Notes |
|---------------|------|-----|-------|
| **F16 on GPU** | 14GB+ | 16GB | RTX 4090, A100, etc. |
| **Q4 on GPU** | 6GB+ | 16GB | RTX 3060, etc. |
| **Q4 on CPU** | — | 16GB+ | Slow but works |
| **Metal (M2+)** | Unified | 16GB+ | Good performance on Apple Silicon |

---

## Performance Targets

| Metric | CPU (Q4) | GPU (F16) | GPU (Q4) | Metal (Q4) |
|--------|----------|-----------|----------|------------|
| First token | ~5s | ~200ms | ~150ms | ~300ms |
| Tokens/sec | ~2-5 | ~30-50 | ~50-80 | ~20-30 |

---

## Relationship to Other Backends

```
┌─────────────────────────────────────────────────────────────┐
│                      LlmService                             │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │  Anthropic  │  │   Mistral   │  │        Stub         │ │
│  │  Backend    │  │   Backend   │  │       Backend       │ │
│  │ (API calls) │  │  (Candle)   │  │     (testing)       │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
│        │                │                    │              │
│        ▼                ▼                    ▼              │
│   maker_key:       maker_key:          (fallback)          │
│   "anthropic"      "mistral"/"local"                       │
└─────────────────────────────────────────────────────────────┘
```

---

## Future Considerations (Not in Scope)

1. **Speculative decoding** — Use smaller draft model for speed
2. **Continuous batching** — Batch multiple requests for GPU efficiency
3. **Context caching** — Reuse KV cache across similar prompts
4. **Fine-tuned models** — Load LoRA adapters for specialized tasks
5. **Mixtral support** — Mixture of Experts requires different handling
