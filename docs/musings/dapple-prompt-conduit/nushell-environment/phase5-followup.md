# Phase 5 Followup: Embedding Optimizations

This document captures enhancement ideas for the embedding system implemented in Phase 5. These are not blockers for current functionality but would improve performance and enable larger-scale use cases.

## Prerequisites

- Phase 5 complete (ModernBERT backend, on-the-fly similarity)
- Semantic ranking working for small datasets
- Understanding of production usage patterns

---

## Followup Items Overview

| Item | Priority | Complexity | Benefit |
|------|----------|------------|---------|
| **F1** | High | Low | Handler integration to enable semantic ranking |
| **F2** | Medium | Low | Embedding caching for repeated texts |
| **F3** | Medium | Medium | Batch embedding for GPU efficiency |
| **F4** | Low | High | pgvector for large-scale vector search |

---

## F1: Handler Integration

### Current State

The handler still uses basic `resolve_visible_slots()`:

```rust
// handler/prompt.rs
let visible_slots = state.overlay_service
    .resolve_visible_slots(req.playbook_task_id)
    .await?;
```

The semantic ranking method exists but isn't called.

### Required Change

```rust
// handler/prompt.rs

// 3. Resolve visible value slots with semantic ranking if enabled
let visible_slots = if state.overlay_service.has_semantic_ranking() {
    // Use context_data as the semantic query
    let context_str = serde_json::to_string(&attempt_context.context_data)
        .unwrap_or_default();

    let ranked = state.overlay_service
        .resolve_visible_slots_with_context(req.playbook_task_id, Some(&context_str))
        .await?;

    log::debug!(
        "Resolved {} visible value slots with semantic ranking for task {}",
        ranked.len(),
        req.playbook_task_id
    );

    // Extract just the slots, preserving ranked order
    ranked.into_iter().map(|r| r.slot).collect()
} else {
    state.overlay_service
        .resolve_visible_slots(req.playbook_task_id)
        .await?
};
```

### Configuration

Semantic ranking is controlled by environment variables (already implemented):

```bash
EMBEDDING_ENABLED=true
EMBEDDING_MODEL_ID=lightonai/modernbert-embed-large
OVERLAY_SEMANTIC_RANKING=true
OVERLAY_SEMANTIC_TOP_K=10
OVERLAY_SIMILARITY_THRESHOLD=0.3
```

### Testing

1. Enable semantic ranking in test environment
2. Create task with multiple value_slots
3. Verify slots are reordered by relevance to context
4. Check logs for similarity scores

---

## F2: Embedding Caching

### Problem

Currently, every call to `resolve_visible_slots_with_context()` re-embeds all slot texts, even if they haven't changed. This is wasteful for repeated queries.

### Solution: In-Memory LRU Cache

Add a cache layer to `EmbeddingService`:

```rust
use lru::LruCache;
use std::sync::Mutex;
use std::num::NonZeroUsize;

pub struct EmbeddingService {
    backend: Arc<dyn EmbeddingBackend>,
    cache: Mutex<LruCache<String, Vec<f32>>>,
}

impl EmbeddingService {
    pub fn new(backend: Arc<dyn EmbeddingBackend>, cache_size: usize) -> Self {
        let cache_size = NonZeroUsize::new(cache_size).unwrap_or(NonZeroUsize::new(1000).unwrap());
        Self {
            backend,
            cache: Mutex::new(LruCache::new(cache_size)),
        }
    }

    pub async fn embed_one_cached(&self, text: &str) -> Result<Vec<f32>, ConduitError> {
        // Check cache first
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(embedding) = cache.get(text) {
                return Ok(embedding.clone());
            }
        }

        // Generate embedding
        let embedding = self.embed_one(text).await?;

        // Store in cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.put(text.to_string(), embedding.clone());
        }

        Ok(embedding)
    }
}
```

### Configuration

```bash
EMBEDDING_CACHE_SIZE=1000  # Number of texts to cache
```

### Dependencies

Add to `Cargo.toml`:

```toml
lru = "0.12"
```

### Cache Invalidation

For now, cache is per-process and cleared on restart. If value_slot content changes, the old embedding remains cached until evicted by LRU. This is acceptable for:
- Read-heavy workloads
- Infrequent content changes

For stronger consistency, consider:
- TTL-based expiration
- Cache invalidation on value_slot update (requires pub/sub or polling)

---

## F3: Batch Embedding for GPU Efficiency

### Problem

Current `resolve_visible_slots_with_context()` embeds slots one at a time:

```rust
for slot in slots {
    let slot_embedding = embedding_service.embed_one(&slot_text).await?;
    // ...
}
```

This is inefficient on GPU where batch processing is much faster.

### Solution: Batch All Slot Embeddings

```rust
pub async fn resolve_visible_slots_with_context(
    &self,
    playbook_task_id: i64,
    context: Option<&str>,
) -> Result<Vec<RankedValueSlot>, ConduitError> {
    let slots = self.resolve_visible_slots(playbook_task_id).await?;

    let context = match context {
        Some(ctx) if self.config.semantic_ranking && self.embedding_service.is_some() => ctx,
        _ => return Ok(slots.into_iter().map(RankedValueSlot::from).collect()),
    };

    let embedding_service = self.embedding_service.as_ref().unwrap();

    // Prepare all texts for batch embedding
    let mut texts: Vec<String> = Vec::with_capacity(slots.len() + 1);
    texts.push(context.to_string());  // First text is the context
    for slot in &slots {
        texts.push(format!("{} {}", slot.name, slot.value_kind));
    }

    // Batch embed all at once
    let embeddings = embedding_service.embed(&texts).await?;

    // First embedding is context, rest are slots
    let context_embedding = &embeddings[0];
    let slot_embeddings = &embeddings[1..];

    // Compute similarities
    let mut ranked_slots: Vec<RankedValueSlot> = slots
        .into_iter()
        .zip(slot_embeddings.iter())
        .map(|(slot, slot_emb)| {
            let similarity = cosine_similarity(context_embedding, slot_emb);
            RankedValueSlot {
                slot,
                similarity: Some(similarity),
            }
        })
        .collect();

    // Sort and filter as before
    ranked_slots.sort_by(|a, b| {
        b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(ranked_slots
        .into_iter()
        .filter(|s| s.similarity.unwrap_or(0.0) >= self.config.similarity_threshold)
        .take(self.config.semantic_top_k)
        .collect())
}
```

### Performance Impact

| Slots | Sequential (CPU) | Batched (CPU) | Batched (GPU) |
|-------|-----------------|---------------|---------------|
| 10 | ~500ms | ~100ms | ~20ms |
| 50 | ~2.5s | ~200ms | ~30ms |
| 100 | ~5s | ~300ms | ~40ms |

*Estimates based on typical ModernBERT inference times*

---

## F4: pgvector for Large-Scale Vector Search

### When to Consider

- Thousands of value_slots per task
- Need for cross-task/cross-journey similarity search
- Response similarity for caching/deduplication
- Building a knowledge base from historical prompts

### Migration

```sql
-- Enable pgvector extension
CREATE EXTENSION IF NOT EXISTS vector;

-- Add embedding column to value_slots
ALTER TABLE rev.value_slots
ADD COLUMN embedding vector(768);

-- Create index for similarity search
CREATE INDEX value_slots_embedding_idx
ON rev.value_slots
USING ivfflat (embedding vector_cosine_ops)
WITH (lists = 100);

-- Separate table for response embeddings
CREATE TABLE rev.prompt_response_embeddings (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    prompt_response_id  BIGINT NOT NULL REFERENCES rev.prompt_responses,
    model_id            TEXT NOT NULL,
    embedding           vector(768) NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT prompt_response_embeddings_unique
        UNIQUE (prompt_response_id, model_id)
);

CREATE INDEX prompt_response_embeddings_idx
ON rev.prompt_response_embeddings
USING ivfflat (embedding vector_cosine_ops)
WITH (lists = 100);
```

### Query Pattern

```sql
-- Find similar value_slots
SELECT id, object_id, name,
       1 - (embedding <=> $1::vector) as similarity
FROM rev.value_slots
WHERE embedding IS NOT NULL
ORDER BY embedding <=> $1::vector
LIMIT $2;
```

### Embedding Population

Background job to populate embeddings:

```rust
pub async fn backfill_value_slot_embeddings(
    pool: &DbPool,
    embedding_service: &EmbeddingService,
    batch_size: usize,
) -> Result<usize, ConduitError> {
    let mut total = 0;

    loop {
        // Get slots without embeddings
        let slots = sqlx::query!(
            r#"
            SELECT id, name, value_kind
            FROM rev.value_slots
            WHERE embedding IS NULL
            LIMIT $1
            "#,
            batch_size as i64
        )
        .fetch_all(pool)
        .await?;

        if slots.is_empty() {
            break;
        }

        // Generate embeddings
        let texts: Vec<String> = slots
            .iter()
            .map(|s| format!("{} {}", s.name, s.value_kind))
            .collect();

        let embeddings = embedding_service.embed(&texts).await?;

        // Update database
        for (slot, embedding) in slots.iter().zip(embeddings.iter()) {
            sqlx::query!(
                "UPDATE rev.value_slots SET embedding = $1::vector WHERE id = $2",
                embedding as &[f32],
                slot.id
            )
            .execute(pool)
            .await?;
        }

        total += slots.len();
        log::info!("Backfilled {} embeddings", total);
    }

    Ok(total)
}
```

### Hybrid Approach

Could combine in-memory and pgvector:
- Small datasets (< 100 slots): In-memory cosine similarity
- Large datasets: pgvector ANN search
- Threshold configurable via `EMBEDDING_PGVECTOR_THRESHOLD=100`

---

## Implementation Order

1. **F1: Handler Integration** - Quick win, enables semantic ranking
2. **F2: Embedding Caching** - Low effort, immediate performance benefit
3. **F3: Batch Embedding** - Medium effort, significant GPU speedup
4. **F4: pgvector** - High effort, only if scale requires it

---

## Files to Modify

### F1: Handler Integration
- `src/handler/prompt.rs`

### F2: Embedding Caching
- `Cargo.toml` (add lru)
- `src/service/embedding/mod.rs`

### F3: Batch Embedding
- `src/service/overlay.rs`

### F4: pgvector
- New migration
- `src/service/embedding/mod.rs` (add pgvector backend)
- `dapple-data` queries for vector operations
- Background job for backfill

---

## Testing Checklist

- [ ] F1: Semantic ranking produces different order than default
- [ ] F2: Cache hit rate logged, repeated queries faster
- [ ] F3: Batch embedding completes in expected time
- [ ] F4: pgvector similarity matches in-memory cosine similarity
