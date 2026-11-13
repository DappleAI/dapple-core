# Nushell Environment Setup - Phase 5: Candle + HuggingFace with ModernBERT

This document focuses on integrating local embedding models via Candle, specifically targeting ModernBERT for high-quality embeddings without external API dependencies.

## Why ModernBERT

[ModernBERT](https://huggingface.co/blog/modernbert) is a refresh of the BERT encoder architecture with modern improvements:
- **8192 token context** via Rotary Positional Embeddings (vs BERT's 512)
- **Unpadding** for efficient batch processing with mixed-length sequences
- **GeGLU layers** replacing MLP for improved performance
- **Matryoshka dimensions** (256-1024) for memory/performance tradeoff

Target model: [lightonai/modernbert-embed-large](https://huggingface.co/lightonai/modernbert-embed-large)

## Use Cases in dapple-prompt

1. **Semantic search over value_slots** - Find relevant context for prompts
2. **RAG context selection** - Rank visible_slots by relevance to prompt
3. **Response similarity** - Compare LLM responses for consistency
4. **Prompt deduplication** - Detect similar prompts for caching

---

## Phase 5 Overview

| Component | Description |
|-----------|-------------|
| **5A** | Candle integration in dapple-prompt-conduit |
| **5B** | ModernBERT embedding backend |
| **5C** | Embedding storage and indexing |
| **5D** | Semantic context selection for prompts |

---

## Phase 5A: Candle Integration

### Add Dependencies

Update `dapple-prompt-conduit/Cargo.toml`:

```toml
[dependencies]
# ... existing deps ...

# Candle for local ML inference
candle-core = "0.8"
candle-nn = "0.8"
candle-transformers = "0.8"

# HuggingFace Hub for model loading
hf-hub = "0.3"

# Tokenizer
tokenizers = "0.21"

# Optional: CUDA support (feature flag)
# candle-core = { version = "0.8", features = ["cuda"] }

[features]
default = []
cuda = ["candle-core/cuda"]
metal = ["candle-core/metal"]
```

### Model Loading Service

Create `dapple-prompt-conduit/src/service/embedding/mod.rs`:

```rust
use candle_core::{Device, Tensor};
use hf_hub::{api::sync::Api, Repo, RepoType};
use std::path::PathBuf;

pub mod modernbert;

/// Configuration for embedding models
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub model_id: String,
    pub revision: Option<String>,
    pub device: DeviceConfig,
    pub dimension: usize,  // Matryoshka dimension (256, 512, 768, 1024)
}

#[derive(Debug, Clone)]
pub enum DeviceConfig {
    Cpu,
    Cuda(usize),  // GPU index
    Metal,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model_id: "lightonai/modernbert-embed-large".to_string(),
            revision: None,
            device: DeviceConfig::Cpu,
            dimension: 768,
        }
    }
}

/// Trait for embedding models
pub trait EmbeddingModel: Send + Sync {
    /// Generate embeddings for a batch of texts
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError>;

    /// Get the embedding dimension
    fn dimension(&self) -> usize;

    /// Get the model identifier
    fn model_id(&self) -> &str;
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("model loading failed: {0}")]
    ModelLoad(String),
    #[error("tokenization failed: {0}")]
    Tokenization(String),
    #[error("inference failed: {0}")]
    Inference(String),
    #[error("device error: {0}")]
    Device(String),
}

/// Download model files from HuggingFace Hub
pub fn download_model(config: &EmbeddingConfig) -> Result<PathBuf, EmbeddingError> {
    let api = Api::new().map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?;

    let repo = match &config.revision {
        Some(rev) => api.repo(Repo::with_revision(
            config.model_id.clone(),
            RepoType::Model,
            rev.clone(),
        )),
        None => api.repo(Repo::new(config.model_id.clone(), RepoType::Model)),
    };

    // Download required files
    let model_path = repo
        .get("model.safetensors")
        .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?;

    Ok(model_path.parent().unwrap().to_path_buf())
}

/// Get the appropriate device based on config
pub fn get_device(config: &DeviceConfig) -> Result<Device, EmbeddingError> {
    match config {
        DeviceConfig::Cpu => Ok(Device::Cpu),
        DeviceConfig::Cuda(ordinal) => {
            Device::new_cuda(*ordinal).map_err(|e| EmbeddingError::Device(e.to_string()))
        }
        DeviceConfig::Metal => {
            Device::new_metal(0).map_err(|e| EmbeddingError::Device(e.to_string()))
        }
    }
}
```

---

## Phase 5B: ModernBERT Backend

Create `dapple-prompt-conduit/src/service/embedding/modernbert.rs`:

```rust
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::modernbert::{Config, ModernBertModel};
use tokenizers::Tokenizer;
use std::sync::Arc;

use super::{EmbeddingConfig, EmbeddingError, EmbeddingModel};

pub struct ModernBertEmbedding {
    model: ModernBertModel,
    tokenizer: Tokenizer,
    device: Device,
    config: EmbeddingConfig,
}

impl ModernBertEmbedding {
    pub fn new(config: EmbeddingConfig) -> Result<Self, EmbeddingError> {
        let device = super::get_device(&config.device)?;
        let model_path = super::download_model(&config)?;

        // Load tokenizer
        let tokenizer_path = model_path.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| EmbeddingError::Tokenization(e.to_string()))?;

        // Load model config
        let config_path = model_path.join("config.json");
        let model_config: Config = serde_json::from_str(
            &std::fs::read_to_string(&config_path)
                .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?,
        )
        .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?;

        // Load model weights
        let weights_path = model_path.join("model.safetensors");
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)
                .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?
        };

        let model = ModernBertModel::new(&model_config, vb)
            .map_err(|e| EmbeddingError::ModelLoad(e.to_string()))?;

        Ok(Self {
            model,
            tokenizer,
            device,
            config,
        })
    }

    fn mean_pooling(&self, hidden_states: &Tensor, attention_mask: &Tensor) -> Result<Tensor, EmbeddingError> {
        // Expand attention mask to match hidden states dimensions
        let mask = attention_mask
            .unsqueeze(2)?
            .expand(hidden_states.shape())?
            .to_dtype(hidden_states.dtype())?;

        // Sum hidden states weighted by mask
        let sum_hidden = (hidden_states * &mask)?.sum(1)?;
        let sum_mask = mask.sum(1)?.clamp(1e-9, f64::MAX)?;

        (sum_hidden / sum_mask).map_err(|e| EmbeddingError::Inference(e.to_string()))
    }

    fn truncate_to_dimension(&self, embeddings: Tensor) -> Result<Tensor, EmbeddingError> {
        // Matryoshka: truncate to desired dimension
        let dim = self.config.dimension;
        embeddings
            .narrow(1, 0, dim)
            .map_err(|e| EmbeddingError::Inference(e.to_string()))
    }

    fn normalize(&self, embeddings: Tensor) -> Result<Tensor, EmbeddingError> {
        // L2 normalize
        let norm = embeddings
            .sqr()?
            .sum_keepdim(1)?
            .sqrt()?
            .clamp(1e-12, f64::MAX)?;

        (embeddings / norm).map_err(|e| EmbeddingError::Inference(e.to_string()))
    }
}

impl EmbeddingModel for ModernBertEmbedding {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // Tokenize
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| EmbeddingError::Tokenization(e.to_string()))?;

        // Build input tensors
        let max_len = encodings.iter().map(|e| e.len()).max().unwrap_or(0);

        let mut input_ids_vec = Vec::new();
        let mut attention_mask_vec = Vec::new();

        for encoding in &encodings {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();

            // Pad to max_len
            let mut padded_ids = ids.to_vec();
            let mut padded_mask = mask.to_vec();
            padded_ids.resize(max_len, 0);
            padded_mask.resize(max_len, 0);

            input_ids_vec.extend(padded_ids);
            attention_mask_vec.extend(padded_mask);
        }

        let batch_size = texts.len();
        let input_ids = Tensor::from_vec(
            input_ids_vec.iter().map(|&x| x as i64).collect(),
            (batch_size, max_len),
            &self.device,
        )
        .map_err(|e| EmbeddingError::Inference(e.to_string()))?;

        let attention_mask = Tensor::from_vec(
            attention_mask_vec.iter().map(|&x| x as i64).collect(),
            (batch_size, max_len),
            &self.device,
        )
        .map_err(|e| EmbeddingError::Inference(e.to_string()))?;

        // Forward pass
        let hidden_states = self
            .model
            .forward(&input_ids, &attention_mask)
            .map_err(|e| EmbeddingError::Inference(e.to_string()))?;

        // Pool, truncate, normalize
        let pooled = self.mean_pooling(&hidden_states, &attention_mask)?;
        let truncated = self.truncate_to_dimension(pooled)?;
        let normalized = self.normalize(truncated)?;

        // Convert to Vec<Vec<f32>>
        let embeddings: Vec<Vec<f32>> = normalized
            .to_vec2()
            .map_err(|e| EmbeddingError::Inference(e.to_string()))?;

        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        self.config.dimension
    }

    fn model_id(&self) -> &str {
        &self.config.model_id
    }
}
```

---

## Phase 5C: Embedding Storage

### Database Schema

Migration: `20260120000001_embeddings.sql`

```sql
-- Store embeddings for value slots (RAG context)
CREATE TABLE rev.value_slot_embeddings (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    value_slot_id   BIGINT NOT NULL REFERENCES rev.value_slots,
    model_id        TEXT NOT NULL,
    dimension       INTEGER NOT NULL,
    embedding       vector NOT NULL,  -- pgvector extension
    created_at      TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT value_slot_embeddings_unique UNIQUE (value_slot_id, model_id)
);

ALTER TABLE rev.value_slot_embeddings OWNER TO dapple;

-- Index for similarity search
CREATE INDEX value_slot_embeddings_idx ON rev.value_slot_embeddings
    USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);

-- Store embeddings for prompt responses (deduplication, similarity)
CREATE TABLE rev.prompt_response_embeddings (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    prompt_response_id  BIGINT NOT NULL REFERENCES rev.prompt_responses,
    model_id            TEXT NOT NULL,
    dimension           INTEGER NOT NULL,
    embedding           vector NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT prompt_response_embeddings_unique UNIQUE (prompt_response_id, model_id)
);

ALTER TABLE rev.prompt_response_embeddings OWNER TO dapple;

CREATE INDEX prompt_response_embeddings_idx ON rev.prompt_response_embeddings
    USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);
```

> **Note:** Requires pgvector extension: `CREATE EXTENSION IF NOT EXISTS vector;`

### Embedding Service

Create `dapple-prompt-conduit/src/service/embedding_service.rs`:

```rust
use std::sync::Arc;
use uuid::Uuid;

use super::embedding::{EmbeddingConfig, EmbeddingModel, ModernBertEmbedding};
use crate::error::ConduitError;

pub struct EmbeddingService {
    model: Arc<dyn EmbeddingModel>,
    pool: Arc<sqlx::PgPool>,
}

impl EmbeddingService {
    pub async fn new(config: EmbeddingConfig, pool: Arc<sqlx::PgPool>) -> Result<Self, ConduitError> {
        log::info!("Loading embedding model: {}", config.model_id);

        let model = ModernBertEmbedding::new(config)
            .map_err(|e| ConduitError::LlmError(format!("Failed to load embedding model: {}", e)))?;

        log::info!("Embedding model loaded, dimension={}", model.dimension());

        Ok(Self {
            model: Arc::new(model),
            pool,
        })
    }

    /// Embed texts and return vectors
    pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ConduitError> {
        self.model
            .embed(texts)
            .map_err(|e| ConduitError::LlmError(e.to_string()))
    }

    /// Embed and store for a value slot
    pub async fn embed_value_slot(&self, value_slot_id: i64, content: &str) -> Result<(), ConduitError> {
        let embeddings = self.embed(&[content.to_string()])?;
        let embedding = &embeddings[0];

        sqlx::query!(
            r#"
            INSERT INTO rev.value_slot_embeddings (value_slot_id, model_id, dimension, embedding)
            VALUES ($1, $2, $3, $4::vector)
            ON CONFLICT (value_slot_id, model_id)
            DO UPDATE SET embedding = EXCLUDED.embedding, created_at = CURRENT_TIMESTAMP
            "#,
            value_slot_id,
            self.model.model_id(),
            self.model.dimension() as i32,
            embedding as &[f32],
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| ConduitError::Database(e.to_string()))?;

        Ok(())
    }

    /// Find similar value slots
    pub async fn find_similar_value_slots(
        &self,
        query: &str,
        limit: i32,
    ) -> Result<Vec<(i64, f32)>, ConduitError> {
        let embeddings = self.embed(&[query.to_string()])?;
        let query_embedding = &embeddings[0];

        let results = sqlx::query!(
            r#"
            SELECT value_slot_id, 1 - (embedding <=> $1::vector) as similarity
            FROM rev.value_slot_embeddings
            WHERE model_id = $2
            ORDER BY embedding <=> $1::vector
            LIMIT $3
            "#,
            query_embedding as &[f32],
            self.model.model_id(),
            limit,
        )
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| ConduitError::Database(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|r| (r.value_slot_id, r.similarity.unwrap_or(0.0) as f32))
            .collect())
    }
}
```

---

## Phase 5D: Semantic Context Selection

Enhance the overlay service to use embeddings for smarter context selection.

### Update OverlayService

In `service/overlay.rs`:

```rust
impl OverlayService {
    /// Resolve visible slots with semantic ranking
    pub async fn resolve_visible_slots_ranked(
        &self,
        playbook_task_id: i64,
        query: &str,
        max_slots: usize,
    ) -> Result<Vec<VisibleValueSlot>, ConduitError> {
        // Get all visible slots
        let all_slots = self.resolve_visible_slots(playbook_task_id).await?;

        if all_slots.len() <= max_slots {
            return Ok(all_slots);
        }

        // Rank by semantic similarity
        let slot_ids: Vec<i64> = all_slots.iter().map(|s| s.id).collect();
        let ranked = self
            .embedding_service
            .rank_by_similarity(query, &slot_ids)
            .await?;

        // Take top N
        let top_ids: std::collections::HashSet<i64> = ranked
            .into_iter()
            .take(max_slots)
            .map(|(id, _)| id)
            .collect();

        Ok(all_slots
            .into_iter()
            .filter(|s| top_ids.contains(&s.id))
            .collect())
    }
}
```

### Update Handler

In `handler/prompt.rs`, optionally use semantic ranking:

```rust
// 3. Resolve visible value slots with optional semantic ranking
let visible_slots = if state.config.use_semantic_ranking {
    let query = &attempt_context.context_data.to_string();
    state.overlay_service
        .resolve_visible_slots_ranked(req.playbook_task_id, query, 20)
        .await?
} else {
    state.overlay_service
        .resolve_visible_slots(req.playbook_task_id)
        .await?
};
```

---

## Implementation Order

1. **Phase 5A: Candle Integration**
   - Add dependencies
   - Create model loading infrastructure
   - Define `EmbeddingModel` trait

2. **Phase 5B: ModernBERT Backend**
   - Implement ModernBERT loader
   - Add tokenization
   - Implement mean pooling + normalization
   - Support Matryoshka dimensions

3. **Phase 5C: Embedding Storage**
   - Add pgvector extension
   - Create embedding tables
   - Implement EmbeddingService
   - Add similarity search queries

4. **Phase 5D: Semantic Context Selection**
   - Update OverlayService with ranking
   - Add config flag for semantic ranking
   - Update handler to use ranked slots

---

## Files to Create/Modify

### New Files
- `dapple-prompt-conduit/src/service/embedding/mod.rs`
- `dapple-prompt-conduit/src/service/embedding/modernbert.rs`
- `dapple-prompt-conduit/src/service/embedding_service.rs`
- `dapple-data/migrations/20260120000001_embeddings.sql`
- `dapple-data/assets/sql/embeddings/insert_value_slot_embedding.sql`
- `dapple-data/assets/sql/embeddings/find_similar_value_slots.sql`

### Modified Files
- `dapple-prompt-conduit/Cargo.toml` (add candle, hf-hub, tokenizers)
- `dapple-prompt-conduit/src/service/mod.rs` (add embedding service)
- `dapple-prompt-conduit/src/service/overlay.rs` (semantic ranking)
- `dapple-prompt-conduit/src/handler/prompt.rs` (use ranked slots)
- `dapple-prompt-conduit/src/config.rs` (embedding config)
- `dapple-prompt-conduit/src/main.rs` (initialize embedding service)

---

## Configuration

### Environment Variables

```bash
# Embedding model config
EMBEDDING_MODEL_ID=lightonai/modernbert-embed-large
EMBEDDING_DIMENSION=768           # Matryoshka: 256, 512, 768, 1024
EMBEDDING_DEVICE=cpu              # cpu, cuda:0, metal

# Feature flags
USE_SEMANTIC_RANKING=true
SEMANTIC_RANKING_MAX_SLOTS=20

# HuggingFace (optional, for private models)
HF_TOKEN=hf_...
```

### Config Struct

```rust
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub model_id: String,
    pub dimension: usize,
    pub device: DeviceConfig,
    pub use_semantic_ranking: bool,
    pub max_ranked_slots: usize,
}

impl EmbeddingConfig {
    pub fn from_env() -> Self {
        Self {
            model_id: std::env::var("EMBEDDING_MODEL_ID")
                .unwrap_or_else(|_| "lightonai/modernbert-embed-large".to_string()),
            dimension: std::env::var("EMBEDDING_DIMENSION")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(768),
            device: DeviceConfig::from_env(),
            use_semantic_ranking: std::env::var("USE_SEMANTIC_RANKING")
                .map(|s| s == "true")
                .unwrap_or(false),
            max_ranked_slots: std::env::var("SEMANTIC_RANKING_MAX_SLOTS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20),
        }
    }
}
```

---

## Testing

### Unit Tests
- [ ] ModernBERT tokenization
- [ ] Embedding dimension matches config
- [ ] Mean pooling produces correct shape
- [ ] L2 normalization (vectors have unit length)

### Integration Tests
- [ ] Model downloads from HuggingFace Hub
- [ ] Embedding stored in pgvector
- [ ] Similarity search returns ranked results
- [ ] Semantic ranking filters slots correctly

### Performance Tests
- [ ] Batch embedding throughput (texts/sec)
- [ ] Single embedding latency
- [ ] pgvector query performance with 10k+ embeddings

---

## Hardware Considerations

| Device | Batch Size | Throughput | Notes |
|--------|------------|------------|-------|
| CPU | 8 | ~10 texts/sec | Development/small scale |
| CUDA (RTX 3080) | 32 | ~200 texts/sec | Production GPU |
| Metal (M2) | 16 | ~80 texts/sec | macOS development |

For production, consider pre-computing embeddings for value_slots on insert rather than at query time.

---

## Deferred to Phase 6

1. **Embedding caching** - Redis cache for frequently used embeddings
2. **Async embedding pipeline** - Background job for bulk embedding
3. **Multiple embedding models** - Support different models per use case
4. **Quantized models** - INT8 models for faster inference
5. **Embedding versioning** - Track model versions, re-embed on upgrade
