# Agent Instructions — conduit-llm

Internal local LLM inference sub-service. Only dapple-prompt-conduit talks to this service.

## Architecture

conduit-llm hosts the MistralRS GGUF backend in its own pod to isolate its ~4-5GB memory footprint from the rest of the conduit orchestration layer. The Anthropic API backend stays in conduit (it's just HTTP calls with near-zero memory).

```
conduit (orchestrator, port 8081)
    |
    v
conduit-llm (local LLM, port 8084)
```

## Authentication

All endpoints except `/api/health_check` require an `X-Internal-Key` header matching `CONDUIT_INTERNAL_KEY`. When the env var is unset (dev mode), all requests are allowed.

## Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/health_check` | GET | Health probe (no auth) |
| `/generate` | POST | Streaming LLM generation (SSE) |

### POST /generate

**Request:**
```json
{
  "messages": [
    {"role": "system", "content": "You are helpful."},
    {"role": "user", "content": "Hello!"}
  ],
  "temperature": 0.7,
  "top_p": 0.9,
  "top_k": 50,
  "max_tokens": 1024
}
```

**Response (SSE):**
```
data: {"type":"token","content":"Hello"}
data: {"type":"token","content":" there"}
data: {"type":"done"}
```

**Error event:**
```
data: {"type":"error","message":"mistral.rs model error: ..."}
```

## Memory Profile

| Component | Memory | Notes |
|-----------|--------|-------|
| MistralRS (Mistral-7B Q4_K_M) | ~4-5GB | GGUF quantized, instant load |
| Runtime + tokio | ~100MB | |
| **Total** | **~4-5GB** | K8s limit: 8Gi |

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `LLM_HOST` | `0.0.0.0` | Server bind host |
| `LLM_PORT` | `8084` | Server bind port |
| `CONDUIT_INTERNAL_KEY` | (none) | Shared auth key |
| `MISTRALRS_MODEL_ID` | `bartowski/Mistral-7B-Instruct-v0.3-GGUF` | GGUF repo |
| `MISTRALRS_GGUF_FILE` | `Mistral-7B-Instruct-v0.3-Q4_K_M.gguf` | GGUF filename |
| `MISTRALRS_TOK_MODEL_ID` | `mistralai/Mistral-7B-Instruct-v0.3` | Tokenizer source |
| `HF_TOKEN` | - | HuggingFace token for gated models |

## Key Files

```
src/main.rs         # Service initialization and warmup
src/lib.rs          # Router with auth middleware
src/auth.rs         # X-Internal-Key validation middleware
src/config.rs       # Environment configuration + MistralRsConfig
src/error.rs        # LlmServiceError
src/backend.rs      # MistralRsBackend (GGUF loading, streaming generation)
src/handler/        # HTTP endpoint handlers
```
