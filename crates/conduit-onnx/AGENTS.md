# Agent Instructions — conduit-onnx

Internal ONNX inference sub-service. Only dapple-prompt-conduit talks to this service.

## Architecture

conduit-onnx is a headless inference service — no orchestration logic, no database access, just ONNX model loading and inference endpoints. It exists to isolate ONNX model memory from the LLM process in conduit.

```
conduit (orchestrator, port 8081)
    |
    v
conduit-onnx (ONNX inference, port 8083)
```

## Authentication

All endpoints except `/api/health_check` require an `X-Internal-Key` header matching `CONDUIT_INTERNAL_KEY`. When the env var is unset (dev mode), all requests are allowed.

## Endpoints

| Endpoint | Method | Service | Purpose |
|----------|--------|---------|---------|
| `/api/health_check` | GET | - | Health probe (no auth) |
| `/embed` | POST | EmbeddingService | Generate embeddings (batched, max 32 per inference call) |
| `/synthesize` | POST | TtsService | Text-to-speech via VITS ONNX |
| `/summarize` | POST | SummarizationService | Text summarization via T5 ONNX |
| `/madlib` | POST | MlmService | Masked language model fill via ModernBERT ONNX |

## Memory Profile

| Component | Memory | Notes |
|-----------|--------|-------|
| ONNX Runtime arena | ~1GB | Shared across all models |
| Embedding (ModernBERT-Large) | ~350MB | 768-dim, Matryoshka truncation |
| TTS (VITS-LJSpeech) | ~200MB | Single speaker, English |
| Summarization (FLAN-T5-Base) | ~400MB | Encoder + decoder |
| MLM (ModernBERT-Base) | ~400MB | Masked language model |
| **Total** | **~2-3GB** | K8s limit: 4Gi |

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `ONNX_HOST` | `0.0.0.0` | Server bind host |
| `ONNX_PORT` | `8083` | Server bind port |
| `CONDUIT_INTERNAL_KEY` | (none) | Shared auth key (unset = dev mode) |
| `ONNX_MODEL_PATH` | `/models` | ONNX model directory |
| `ONNX_EXECUTION_PROVIDER` | (auto) | Override: CPUExecutionProvider, ROCMExecutionProvider, etc. |
| `ORT_DYLIB_PATH` | - | Path to libonnxruntime.so |
| `EMBEDDING_ENABLED` | `false` | Enable embedding service |
| `EMBEDDING_MODEL_ID` | `lightonai/modernbert-embed-large` | HuggingFace model ID |
| `EMBEDDING_DIMENSION` | `768` | Vector dimension |
| `EMBEDDING_REQUIRED` | `false` | Panic on init failure |
| `TTS_ENABLED` | `false` | Enable TTS service |
| `TTS_MODEL` | `vits-ljspeech` | VITS model name |
| `TTS_MAX_TEXT_LENGTH` | `500` | Max input characters |
| `TTS_REQUIRED` | `false` | Panic on init failure |
| `SUMMARIZATION_ENABLED` | `false` | Enable summarization |
| `SUMMARIZATION_REQUIRED` | `false` | Panic on init failure |
| `MLM_ENABLED` | `false` | Enable MLM service |
| `MLM_REQUIRED` | `false` | Panic on init failure |

## Key Files

```
src/main.rs           # Service initialization and warmup
src/lib.rs            # Router with auth middleware
src/auth.rs           # X-Internal-Key validation middleware
src/config.rs         # Environment configuration
src/error.rs          # OnnxServiceError (ONNX-only error variants)
src/service/mod.rs    # AppState (embedding, tts, summarization, mlm)
src/service/onnx/     # Shared ONNX session builder
src/service/embedding/ # ModernBERT embedding backend
src/service/tts/      # VITS TTS backend
src/service/summarization/ # T5 summarization backend
src/service/mlm/      # ModernBERT MLM backend
src/handler/          # HTTP endpoint handlers
```

## This service does NOT have

- Database access (no `dapple-data` dependency)
- Search/pgvector queries (search stays in conduit)
- LLM backends (Anthropic, MistralRS stay in conduit)
- RAG, overlay, template, context services (all in conduit)
- Any external-facing endpoints (ClusterIP only, no Ingress)
