# Model Taxonomy

> **Audience:** Backend Developers · Platform Engineers

This document describes the technical implementation of the model taxonomy in the conduit.

## Overview

The model taxonomy replaces the legacy `llm_engine_makers` / `llm_engines` tables with a clearer structure:

| Old Term | New Term | Purpose |
|----------|----------|---------|
| `llm_engine_makers` | `lineages` | Prompting paradigm |
| `llm_engines` | `model_releases` | Specific model at provider |
| `playbook_step_engine_configs` | (removed) | Replaced by InferenceProfiles |

## ModelConfig

The `ModelConfig` struct contains everything needed to execute a prompt:

```rust
pub struct ModelConfig {
    /// Prompting paradigm: "claude-3-5", "vits", "flan-t5"
    pub lineage_key: String,
    /// Hosting endpoint: "anthropic", "self-hosted"
    pub provider_key: String,
    /// Capability level: "sonnet", "opus", "ljspeech"
    pub tier: String,
    /// Version snapshot: "20241022", "v0.1"
    pub release: Option<String>,
    /// Fine-tune ID if using a variant
    pub variant_id: Option<Uuid>,
    /// Selection priority (lower = preferred)
    pub priority: i32,
    // Sampling parameters...
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<usize>,
    pub max_tokens: Option<usize>,
}
```

## ModelService

The `ModelService` resolves model configuration for prompt execution:

```rust
impl ModelService {
    /// Resolve model for a playbook task
    pub async fn resolve_model_for_task(
        &self,
        playbook_task_id: i64,
    ) -> Result<ModelConfig, ConduitError>;

    /// Resolve model from InferenceProfiles (full implementation)
    pub async fn resolve_model_for_dispatch(
        &self,
        journey_id: i32,
        prompt_path_object_id: Uuid,
    ) -> Result<ModelConfig, ConduitError>;
}
```

## Release Chain Resolution

The release chain provides fallback for template resolution:

```rust
pub fn build_release_chain(
    lineage_key: &str,
    tier: &str,
    release: Option<&str>,
    provider_key: &str,
) -> Vec<String>;
```

**Example output (Claude 3.5):**
```
["@anthropic:claude-3-5/sonnet:20241022",
 "@anthropic:claude-3-5/sonnet",
 "claude-3-5/sonnet:20241022",
 "claude-3-5/sonnet",
 "claude-3-5"]
```

**Example output (Claude 4.6):**
```
["@anthropic:claude-4-6/sonnet:20250929",
 "@anthropic:claude-4-6/sonnet",
 "claude-4-6/sonnet:20250929",
 "claude-4-6/sonnet",
 "claude-4-6"]
```

**Resolution priority:**
1. Provider-specific exact match (`@provider:lineage/tier:release`)
2. Provider-specific tier match (`@provider:lineage/tier`)
3. Shared exact match (`lineage/tier:release`)
4. Shared tier match (`lineage/tier`)
5. Lineage-wide templates (`lineage`)

## Backend Routing

The `LlmBackend` trait uses `ModelConfig` to determine support:

```rust
#[async_trait]
pub trait LlmBackend: Send + Sync {
    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        model: &ModelConfig,
    ) -> Result<TokenStream, ConduitError>;

    fn supports(&self, model: &ModelConfig) -> bool;
}
```

### Anthropic Backend

```rust
impl LlmBackend for AnthropicBackend {
    fn supports(&self, model: &ModelConfig) -> bool {
        model.provider_key == "anthropic"
            || model.lineage_key.starts_with("claude")
    }
}
```

The Anthropic backend uses lineage-aware model mapping to disambiguate tiers like
"sonnet" that exist across multiple lineages:

```rust
fn map_model(&self, lineage_key: &str, tier: &str) -> &'static str {
    match (lineage_key, tier) {
        ("claude-4-6", "sonnet") => "claude-sonnet-4-5-20250929",
        ("claude-3-5", "sonnet") | (_, "sonnet-3-5") => CLAUDE_3_5_SONNET,
        (_, "opus") | (_, "opus-4") => CLAUDE_4_OPUS,
        (_, "sonnet-4") => CLAUDE_4_SONNET,
        // Bare "sonnet" without lineage context defaults to 3.5
        (_, "sonnet") => CLAUDE_3_5_SONNET,
        // ...
    }
}
```

This two-key approach (`lineage_key` + `tier`) ensures that `"sonnet"` under
`claude-4-6` maps to `claude-sonnet-4-5-20250929`, while `"sonnet"` under
`claude-3-5` maps to `claude-3-5-sonnet-20241022`.

### Mistral Backend

```rust
impl LlmBackend for MistralBackend {
    fn supports(&self, model: &ModelConfig) -> bool {
        model.lineage_key == "mistral"
            || model.provider_key == "self-hosted"
    }
}
```

### Stub Backend

```rust
impl LlmBackend for StubBackend {
    fn supports(&self, model: &ModelConfig) -> bool {
        model.lineage_key == "stub"
    }
}
```

## Specialized Handlers

The prompt handler routes to specialized services based on `lineage_key`:

```rust
// In handle_prompt()
if model.lineage_key == "vits" {
    return handle_tts_prompt(state, req, model, ...).await;
}
if model.lineage_key == "flan-t5" {
    return handle_summarization_prompt(state, req, model, ...).await;
}
if model.lineage_key == "modernbert" {
    return handle_mlm_prompt(state, req, model, ...).await;
}
// Default: LLM prompt handling
```

## Database Schema

### New Tables

```sql
-- Providers (API/hosting endpoints)
CREATE TABLE rev.providers (
    id SMALLINT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    display_name TEXT,
    api_config JSONB
);

-- Lineages (prompting paradigms)
CREATE TABLE rev.lineages (
    id SMALLINT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT
);

-- Model Releases (specific models at providers)
CREATE TABLE rev.model_releases (
    id SERIAL PRIMARY KEY,
    object_id UUID NOT NULL UNIQUE,
    lineage_id SMALLINT REFERENCES rev.lineages(id),
    provider_id SMALLINT REFERENCES rev.providers(id),
    tier TEXT NOT NULL,
    release TEXT,
    UNIQUE (lineage_id, provider_id, tier, release)
);

-- Model Variants (fine-tunes)
CREATE TABLE rev.model_variants (
    id SERIAL PRIMARY KEY,
    object_id UUID NOT NULL UNIQUE,
    base_release_id INT REFERENCES rev.model_releases(id),
    key TEXT NOT NULL,
    description TEXT
);
```

### InferenceProfile Tables

```sql
-- Journey-level model configuration
CREATE TABLE rev.inference_profiles (
    id SERIAL PRIMARY KEY,
    object_id UUID NOT NULL UNIQUE,
    versioned_object_id INT REFERENCES rev.versioned_objects(id),
    name TEXT NOT NULL,
    description TEXT
);

-- Maps prompts to models
CREATE TABLE rev.inference_profile_configs (
    id SERIAL PRIMARY KEY,
    inference_profile_id INT REFERENCES rev.inference_profiles(id),
    prompt_path_object_id UUID NOT NULL,
    model_release_id INT REFERENCES rev.model_releases(id),
    model_variant_id INT REFERENCES rev.model_variants(id),
    priority INT NOT NULL DEFAULT 0,
    CONSTRAINT chk_one_model CHECK (
        (model_release_id IS NOT NULL)::int +
        (model_variant_id IS NOT NULL)::int = 1
    )
);

-- Junction: Journeys have InferenceProfiles
CREATE TABLE rev.journey_inference_profiles (
    id SERIAL PRIMARY KEY,
    journey_id INT REFERENCES rev.journeys(id),
    inference_profile_id INT REFERENCES rev.inference_profiles(id),
    ordinal INT NOT NULL DEFAULT 0
);
```

## Adding a New Lineage

To add support for a new model family:

1. **Insert the lineage:**
   ```sql
   INSERT INTO rev.lineages (key, description)
   VALUES ('new-lineage', 'Description of prompting style');
   ```

2. **Add model releases:**
   ```sql
   INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
   SELECT l.id, p.id, 'tier-name', 'version'
   FROM rev.lineages l, rev.providers p
   WHERE l.key = 'new-lineage' AND p.key = 'provider-key';
   ```

3. **Update backend routing** (if using an existing backend):
   - Add the new lineage to `map_model()` in the relevant backend (e.g., `anthropic.rs`)
   - The `map_model(lineage_key, tier)` function uses both keys to disambiguate tiers

4. **Create a backend** (if needed):
   - Implement `LlmBackend` trait
   - Add to `LlmService` backends list

5. **Add handler routing** (if specialized):
   ```rust
   if model.lineage_key == "new-lineage" {
       return handle_new_lineage_prompt(...).await;
   }
   ```

## Related Documentation

- [Foundation: Model Taxonomy](../../_foundation/concepts/model-taxonomy.md) - Conceptual overview
- [LLM Backends](./llm-backends.md) - Backend implementation details
- [Template Rendering](./template-rendering.md) - Release chain in templates
