# Nushell Environment Setup - Phase 7: Polish and Configuration

This phase addresses cleanup items and configuration improvements identified during Phase 6 review.

---

## Phase 7 Overview

| Component | Description |
|-----------|-------------|
| **7A** | Fix Cargo.toml edition |
| **7B** | Wire SamplingParams to EngineConfig |

---

## Phase 7A: Fix Cargo.toml Edition

### Problem

`dapple-prompt-conduit/Cargo.toml` specifies `edition = "2024"` which doesn't exist yet. Rust editions are released every 3 years (2015, 2018, 2021). The 2024 edition won't be stable until late 2024/early 2025.

### Fix

```toml
# Before
edition = "2024"

# After
edition = "2021"
```

### Files to Modify
- `crates/dapple-prompt-conduit/Cargo.toml`

---

## Phase 7B: Wire SamplingParams to EngineConfig

### Problem

The Mistral backend currently uses hardcoded `SamplingParams::default()` instead of pulling values from the `EngineConfig` or request parameters.

```rust
// Current (hardcoded)
let params = SamplingParams::default();
```

### Solution

Extend `EngineConfig` to include optional sampling parameters, and use them when available.

### EngineConfig Extension

```rust
// In src/service/engine.rs
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub maker_key: String,
    pub engine_key: String,
    pub engine_version: Option<String>,

    // Sampling parameters (optional, backend uses defaults if None)
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<usize>,
    pub max_tokens: Option<usize>,
}
```

### Mistral Backend Update

```rust
// In src/service/llm/mistral.rs

impl SamplingParams {
    /// Create from EngineConfig, falling back to defaults
    pub fn from_engine_config(engine: &EngineConfig) -> Self {
        Self {
            temperature: engine.temperature.unwrap_or(0.7),
            top_p: engine.top_p.unwrap_or(0.9),
            top_k: engine.top_k.unwrap_or(40),
            max_tokens: engine.max_tokens.unwrap_or(2048),
            ..Default::default()
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
        // Use engine config for sampling params
        let params = SamplingParams::from_engine_config(engine);

        // ... rest of implementation
    }
}
```

### Database Schema (if storing in engines table)

If sampling defaults should be configurable per engine in the database:

```sql
-- Optional: Add columns to rev.engines
ALTER TABLE rev.engines
ADD COLUMN default_temperature REAL,
ADD COLUMN default_top_p REAL,
ADD COLUMN default_top_k INTEGER,
ADD COLUMN default_max_tokens INTEGER;
```

### Files to Modify
- `crates/dapple-prompt-conduit/src/service/engine.rs` — Add optional sampling fields
- `crates/dapple-prompt-conduit/src/service/llm/mistral.rs` — Use engine config for params

### Files to Create (optional)
- Migration for engine sampling defaults (if database-driven)

---

## Implementation Order

1. **7A: Fix edition** — Simple one-line fix, do first
2. **7B: SamplingParams wiring** — Extend EngineConfig, update Mistral backend

---

## Testing

### 7A Tests
- [ ] `cargo build -p dapple-prompt-conduit` succeeds
- [ ] No edition-related warnings

### 7B Tests
- [ ] Unit test: `SamplingParams::from_engine_config` with all fields
- [ ] Unit test: `SamplingParams::from_engine_config` with partial fields (uses defaults)
- [ ] Integration: Verify temperature affects output randomness
