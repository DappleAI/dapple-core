# Embedding Backends

> **Audience:** Backend Developers · Platform Engineers

The conduit provides embedding services for semantic operations like similarity search and masked language modeling.

## Available Services

| Endpoint | Model | Description |
|----------|-------|-------------|
| `POST /summarize` | T5 | Text summarization |
| `POST /madlib` | ModernBERT MLM | Mask filling |

## ModernBERT Embedding

The embedding service uses ModernBERT via Candle for:

- Semantic similarity
- Text classification
- Masked language modeling (madlib)

### Configuration

```
# Optional configuration
EMBEDDING_MODEL_ID=answerdotai/ModernBERT-base
EMBEDDING_DEVICE=cpu|cuda:0|metal  # Auto-detected if not set
EMBEDDING_DTYPE=f32                 # Default: f32
```

## Summarization Service

The `/summarize` endpoint uses T5 for text summarization:

```bash
curl -X POST http://localhost:8081/summarize \
  -H "Content-Type: application/json" \
  -d '{"text": "Long text to summarize..."}'
```

Response:
```json
{
  "summary": "Summarized text..."
}
```

## Madlib Service

The `/madlib` endpoint fills masked tokens:

```bash
curl -X POST http://localhost:8081/madlib \
  -H "Content-Type: application/json" \
  -d '{"text": "The [MASK] is shining today."}'
```

Response:
```json
{
  "filled": "The sun is shining today.",
  "predictions": [
    {"token": "sun", "score": 0.85},
    {"token": "moon", "score": 0.08},
    {"token": "light", "score": 0.04}
  ]
}
```

## Architecture

```rust
pub trait EmbeddingBackend: Send + Sync {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn supports(&self, model_type: &str) -> bool;
}

pub struct ModernBertBackend {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}
```

## Use Cases

- **Semantic search** — Find similar documents
- **Classification** — Categorize text based on embeddings
- **Slot matching** — Match user input to value slots
- **Content generation** — Fill in missing information
