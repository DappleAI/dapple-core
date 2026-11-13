# LLM Backends

> **Audience:** Backend Developers · Platform Engineers

The conduit supports multiple LLM backends, selected based on `lineage_key` and `provider_key` in the model configuration.

## Available Backends

| Backend | `lineage_key` / `provider_key` | Description | Requirements |
|---------|-------------------------------|-------------|--------------|
| Anthropic | `claude-3-5`, `claude-4-6` / `anthropic` | Claude models via API | `ANTHROPIC_API_KEY` |
| Mistral | `mistral` / `self-hosted` | Local inference via Candle | `MISTRAL_ENABLED=true` |
| Stub | (any unmatched) | Echo backend for testing | None (always available) |

## Backend Selection

The `LlmService` iterates through registered backends and selects the first one that supports the model configuration:

```rust
// LlmService selects backend by checking supports() method
for backend in &self.backends {
    if backend.supports(&model) {
        return backend.execute_streaming(&prompt, &model).await;
    }
}
// Fall back to stub if no backend matches
self.fallback.execute_streaming(&prompt, &model).await
```

## Anthropic Backend

Uses Claude models via the Anthropic API with rig-core.

### Configuration

```
# Required environment
ANTHROPIC_API_KEY=sk-ant-...
```

### Supported Models

- `claude-sonnet-4-5-*` (Claude 4.6 Sonnet)
- `claude-4-opus-*`
- `claude-4-sonnet-*`
- `claude-3-5-sonnet-*`
- `claude-3-5-haiku-*`

## Mistral Backend

Runs local Mistral inference via Candle (no external API required).

### Configuration

```
# Required environment
MISTRAL_ENABLED=true

# Optional configuration
MISTRAL_MODEL_ID=mistralai/Mistral-7B-Instruct-v0.3
MISTRAL_DEVICE=cpu|cuda:0|metal  # Auto-detected if not set
MISTRAL_DTYPE=f16|bf16|f32       # Default: f16
MISTRAL_MAX_CONTEXT=8192
HF_TOKEN=hf_...                   # For gated models
```

### Device Selection

| Device | Description |
|--------|-------------|
| `cpu` | CPU inference (slowest, always available) |
| `cuda:0` | NVIDIA GPU via CUDA |
| `metal` | Apple Silicon via Metal |

## Stub Backend

Echoes input for testing. Always available as fallback.

```json
{
  "lineage_key": "stub",
  "provider_key": "stub"
}
```

The stub backend is useful for:
- Unit testing without API costs
- Development without credentials
- CI/CD pipelines

## Model Configuration

### ModelConfig Structure

```rust
pub struct ModelConfig {
    pub lineage_key: String,     // "claude-3-5", "vits", "flan-t5"
    pub provider_key: String,    // "anthropic", "self-hosted"
    pub tier: String,            // "sonnet", "ljspeech"
    pub release: Option<String>, // "20241022", "v0.1"
    pub variant_id: Option<Uuid>,
    pub priority: i32,

    // Sampling parameters (optional)
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<usize>,
    pub max_tokens: Option<usize>,
}
```

### Lineage-Aware Routing

The Anthropic backend's `map_model()` function uses both `lineage_key` and `tier` to disambiguate models like "sonnet" that exist across multiple lineages. For example, `("claude-4-6", "sonnet")` maps to `claude-sonnet-4-5-20250929`, while bare `"sonnet"` defaults to `claude-3-5-sonnet-20241022`. See [Model Taxonomy](./model-taxonomy.md#anthropic-backend) for the full mapping table.

### Resolution Hierarchy

1. **InferenceProfile** — Prompt-to-model mapping assigned to the journey
2. **Default** — Falls back to stub if no config found

See [Model Taxonomy](./model-taxonomy.md) for the full resolution chain.

## Adding a New Backend

1. Create `src/service/llm/{backend}.rs`
2. Implement `LlmBackend` trait:
   - `supports(&self, model: &ModelConfig) -> bool`
   - `execute_streaming(&self, prompt: &RenderedPrompt, model: &ModelConfig) -> Result<TokenStream>`
3. Register in `LlmService::new()` in `src/service/llm/mod.rs`
4. Add feature flags to `Cargo.toml` if needed (e.g., `cuda`, `metal`)

## Environment Variables Summary

| Variable | Default | Description |
|----------|---------|-------------|
| `ANTHROPIC_API_KEY` | (none) | Enable Anthropic backend |
| `MISTRAL_ENABLED` | `false` | Enable local Mistral |
| `MISTRAL_MODEL_ID` | `mistralai/Mistral-7B-Instruct-v0.3` | HuggingFace model ID |
| `MISTRAL_DEVICE` | (auto) | Device: `cpu`, `cuda:0`, `metal` |
| `MISTRAL_DTYPE` | `f16` | Data type: `f16`, `bf16`, `f32` |
| `MISTRAL_MAX_CONTEXT` | `8192` | Max context length |
| `HF_TOKEN` | (none) | HuggingFace token for gated models |
