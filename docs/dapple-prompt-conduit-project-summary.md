# dapple-prompt-conduit: Project Summary

## Executive Summary

We built a new service called `dapple-prompt-conduit` that serves as the AI gateway for the Dapple platform. This service manages all interactions with Large Language Models (LLMs), providing a unified interface that supports multiple AI providers, local inference capabilities, and intelligent context management.

The project was delivered across 7 phases over the course of development, establishing a production-ready foundation for AI-powered workflows.

---

## What We Built

### The Problem We Solved

Before this project, the platform had no standardized way to:
- Send prompts to AI models
- Switch between AI providers without code changes
- Run AI locally for privacy-sensitive workloads
- Select relevant context intelligently
- Track and audit AI interactions

### The Solution

`dapple-prompt-conduit` is a standalone Rust service that acts as a "conduit" (channel) between our workflow engine and AI providers. It handles:

1. **Prompt Construction** — Building prompts from templates and context data
2. **Provider Routing** — Sending prompts to the right AI backend
3. **Response Streaming** — Delivering AI responses in real-time
4. **Interaction Logging** — Recording all AI usage for auditing and analytics

---

## Key Capabilities Delivered

### 1. Multi-Provider LLM Support

The system uses a pluggable backend architecture that routes prompts to different AI providers based on configuration.

**Supported Backends:**

| Backend | Type | Use Case |
|---------|------|----------|
| **Anthropic Claude** | Cloud API | High-quality responses, complex reasoning tasks |
| **Local Mistral** | On-premises | Privacy-sensitive data, cost optimization |
| **Stub** | Testing | Development and automated testing |

**How It Works:**

Each "engine" in the system has a `maker_key` that determines which backend handles the request:
- `maker_key: "anthropic"` → Routes to Claude API
- `maker_key: "mistral"` or `"local"` → Routes to local Mistral
- `maker_key: "stub"` → Returns echo response for testing

**Adding New Providers:**

The architecture is extensible. Adding a new provider (e.g., OpenAI, Cohere, Llama) requires:
1. Creating a new backend file
2. Implementing the `LlmBackend` trait (two methods)
3. Registering the backend in the service

**Business Value:**
- No vendor lock-in — switch providers without code changes
- Cost optimization — route different workloads to appropriate providers
- Flexibility — use cloud for some tasks, local for others
- Future-proofing — easy to add new providers as the market evolves

---

### 2. Local AI Inference

We can now run AI models directly on our own servers without calling external APIs. This is a significant capability for enterprise deployments.

**Technology Stack:**
- **Candle** — A Rust-native machine learning framework from HuggingFace
- **Mistral 7B Instruct** — An open-source LLM comparable to GPT-3.5
- **HuggingFace Hub** — Model distribution and versioning

**Hardware Support:**

| Platform | Configuration | Performance |
|----------|--------------|-------------|
| CPU | Any modern x86_64 | ~2-5 tokens/second |
| NVIDIA GPU | CUDA-capable (RTX 3060+) | ~50-80 tokens/second |
| Apple Silicon | M1/M2/M3 Mac | ~20-30 tokens/second |

**How It Works:**

1. On first request, the model downloads from HuggingFace (~14GB for full precision)
2. Model loads into memory (GPU VRAM or system RAM)
3. Prompts are tokenized and processed through the model
4. Tokens stream back one at a time
5. KV-cache optimization speeds up subsequent tokens in the same conversation

**Configuration:**

```bash
# Enable local inference
MISTRAL_ENABLED=true

# Choose hardware (auto-detected if not specified)
MISTRAL_DEVICE=cuda:0  # or "cpu" or "metal"

# Model selection (defaults to Mistral 7B Instruct v0.3)
MISTRAL_MODEL_ID=mistralai/Mistral-7B-Instruct-v0.3

# For gated models that require authentication
HF_TOKEN=hf_xxxxxxxxxxxxx
```

**Business Value:**
- **Privacy** — Sensitive data never leaves your infrastructure
- **Cost** — No per-token API charges (just hardware/electricity)
- **Availability** — Works offline, no dependency on external services
- **Latency** — Potentially lower latency for on-premises deployments
- **Compliance** — Meets data residency requirements

---

### 3. Semantic Context Selection

The system uses a text understanding model (ModernBERT) to intelligently select which context to include in prompts, rather than including everything or using keyword matching.

**The Problem:**

LLMs have limited context windows (how much text they can process at once). Including too much context:
- Wastes tokens (increases cost)
- Can confuse the model
- Slows down processing

Including too little context:
- Leads to poor responses
- Misses relevant information

**The Solution:**

ModernBERT converts text into numerical "embeddings" — lists of numbers that represent meaning. Similar texts have similar embeddings. We use this to:

1. Convert the user's query to an embedding
2. Convert all available context items to embeddings
3. Find context items most similar to the query (cosine similarity)
4. Include only the most relevant context in the prompt

**Technical Details:**

- **Model:** ModernBERT (8192 token context, state-of-the-art encoder)
- **Dimensions:** Configurable (128 to 768 dimensions)
- **Similarity:** Cosine similarity for ranking
- **Execution:** Runs entirely locally via Candle

**Example:**

If the user asks "How do I reset my password?" and we have 50 context documents:
- Old approach: Include all 50 documents (expensive, noisy)
- Keyword approach: Include documents containing "password" or "reset" (might miss relevant security policies)
- Semantic approach: Include the 5 documents most semantically similar to password reset questions (efficient, relevant)

**Business Value:**
- **Better responses** — AI gets the most relevant context
- **Lower costs** — Fewer tokens per request
- **Faster processing** — Smaller prompts process faster
- **No external calls** — Embedding computation runs locally

---

### 4. Template-Based Prompt Rendering

Prompts are defined as Handlebars templates with dynamic slots for context data, separating prompt logic from content.

**The Problem:**

Hard-coded prompts are:
- Difficult to modify without developer involvement
- Impossible to A/B test
- Not reusable across different contexts
- Hard to version and audit

**The Solution:**

Prompts are stored as templates with placeholders:

```handlebars
{{#if system_prompt}}
You are {{system_prompt.role}}. {{system_prompt.instructions}}
{{/if}}

Context:
{{#each context_items}}
- {{this.title}}: {{this.content}}
{{/each}}

User Question: {{user_input}}
```

At runtime, the template engine:
1. Fetches the template for the current task
2. Gathers context data from various sources
3. Renders the final prompt by filling in placeholders
4. Sends the complete prompt to the LLM

**Template Features:**

| Feature | Syntax | Purpose |
|---------|--------|---------|
| Variables | `{{variable}}` | Insert dynamic content |
| Conditionals | `{{#if condition}}...{{/if}}` | Include content conditionally |
| Loops | `{{#each items}}...{{/each}}` | Iterate over lists |
| Helpers | `{{formatDate date}}` | Transform data |

**Business Value:**
- **Non-technical editing** — Product/content teams can modify prompts
- **A/B testing** — Test different prompt versions easily
- **Versioning** — Track prompt changes over time
- **Reusability** — Share prompt patterns across tasks
- **Separation of concerns** — Prompt content separate from code

---

### 5. Streaming Responses

AI responses stream token-by-token back to the client, providing real-time feedback as the model generates text.

**The Problem:**

LLMs can take 10-60 seconds to generate a full response. Without streaming:
- Users stare at a loading spinner
- No way to know if the request is working
- Poor perceived performance

**The Solution:**

The conduit service uses Server-Sent Events (SSE) to stream tokens as they're generated:

```
User sends prompt
     ↓
Conduit receives request
     ↓
LLM starts generating
     ↓
Token 1 → Client displays "The"
Token 2 → Client displays "The answer"
Token 3 → Client displays "The answer is"
...
Final token → Client displays complete response
```

**Technical Implementation:**

- Uses async Rust streams for efficient token handling
- Works with all backends (Anthropic API streams natively, Mistral streams locally)
- Supports backpressure (client can slow down if overwhelmed)
- Handles disconnections gracefully

**Business Value:**
- **Better UX** — Users see responses forming in real-time
- **Perceived speed** — Feels faster even if total time is the same
- **Early termination** — Users can cancel if the response is going in the wrong direction
- **Engagement** — More interactive, chat-like experience

---

### 6. Response Storage and Auditing

All AI interactions are logged to the database, providing a complete audit trail and enabling analytics.

**What We Store:**

| Field | Description |
|-------|-------------|
| `prompt_id` | Which prompt template was used |
| `attempt_id` | Which workflow execution triggered this |
| `engine_config` | Which AI provider/model was used |
| `rendered_prompt` | The exact prompt sent to the LLM |
| `response_content` | The complete AI response |
| `token_counts` | Input/output token counts |
| `timing` | How long generation took |
| `created_at` | Timestamp |

**How It Works:**

After the LLM finishes generating (all tokens streamed), the conduit service:
1. Assembles the complete response
2. Calculates token counts
3. Inserts a record into `prompt_responses` table
4. Links to the workflow execution context

**Business Value:**
- **Audit trail** — Full record of all AI interactions
- **Debugging** — See exactly what was sent/received
- **Analytics** — Track usage, costs, performance
- **Compliance** — Meet regulatory requirements for AI logging
- **Improvement** — Analyze responses to improve prompts

---

## Architecture Overview

### System Context

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Dapple Platform                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────────┐  │
│  │   Web App    │    │  dapple-     │    │   dapple-prompt-     │  │
│  │   (Next.js)  │───▶│  prompt-api  │───▶│       flow           │  │
│  │              │    │   (REST)     │    │  (Workflow Engine)   │  │
│  └──────────────┘    └──────────────┘    └──────────┬───────────┘  │
│                                                      │              │
│                                                      ▼              │
│                                          ┌──────────────────────┐  │
│                                          │  dapple-prompt-      │  │
│                                          │     conduit          │  │
│                                          │  (AI Gateway)        │  │
│                                          └──────────┬───────────┘  │
│                                                      │              │
│                      ┌───────────────────────────────┼──────────┐  │
│                      │                               │          │  │
│                      ▼                               ▼          ▼  │
│              ┌──────────────┐              ┌─────────────┐  ┌────┐ │
│              │   Anthropic  │              │   Mistral   │  │Stub│ │
│              │   Claude API │              │   (Local)   │  │    │ │
│              └──────────────┘              └─────────────┘  └────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Conduit Service Internals

```
┌─────────────────────────────────────────────────────────────────────┐
│                      dapple-prompt-conduit                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────────┐                                                │
│  │  HTTP Handler   │  ← Receives prompt execution requests          │
│  └────────┬────────┘                                                │
│           │                                                         │
│           ▼                                                         │
│  ┌─────────────────┐    ┌─────────────────┐                        │
│  │ Engine Service  │───▶│ Context Service │                        │
│  │ (resolve config)│    │ (fetch data)    │                        │
│  └────────┬────────┘    └────────┬────────┘                        │
│           │                      │                                  │
│           ▼                      ▼                                  │
│  ┌─────────────────┐    ┌─────────────────┐                        │
│  │ Template Service│◀───│ Overlay Service │                        │
│  │  (Handlebars)   │    │ (slot selection)│                        │
│  └────────┬────────┘    └────────┬────────┘                        │
│           │                      │                                  │
│           │              ┌───────┴────────┐                        │
│           │              │ Embedding Svc  │                        │
│           │              │ (ModernBERT)   │                        │
│           │              └────────────────┘                        │
│           ▼                                                         │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                       LLM Service                            │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │   │
│  │  │  Anthropic  │  │   Mistral   │  │        Stub         │  │   │
│  │  │  Backend    │  │   Backend   │  │       Backend       │  │   │
│  │  └─────────────┘  └─────────────┘  └─────────────────────┘  │   │
│  └─────────────────────────────────────────────────────────────┘   │
│           │                                                         │
│           ▼                                                         │
│  ┌─────────────────┐                                                │
│  │ Response Store  │  → Logs to database                           │
│  └─────────────────┘                                                │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Data Flow

```
1. Request arrives
   │
   ▼
2. Resolve engine config (which LLM to use)
   │
   ▼
3. Fetch context data (from database)
   │
   ▼
4. Compute embeddings for semantic ranking (optional)
   │
   ▼
5. Select relevant context slots
   │
   ▼
6. Render prompt template with context
   │
   ▼
7. Send to appropriate LLM backend
   │
   ▼
8. Stream tokens back to client
   │
   ▼
9. Store complete response in database
```

---

## Implementation Phases

### Phase 1: Nushell Environment Setup
**Objective:** Establish the basic project structure and Nushell integration points.

**Delivered:**
- Created `dapple-prompt-conduit` crate
- Set up Cargo.toml with initial dependencies
- Established connection patterns with workflow engine
- Basic error handling framework

---

### Phase 2: Handlebars Template Service
**Objective:** Build the prompt template rendering system.

**Delivered:**
- Handlebars template engine integration
- Template loading from database
- Variable substitution and helpers
- Multi-message prompt support (system, user, assistant)
- Template validation

---

### Phase 3: Anthropic Claude Integration (rig-core)
**Objective:** Connect to Anthropic's Claude API for cloud-based inference.

**Delivered:**
- `LlmBackend` trait definition
- `AnthropicBackend` implementation using rig-core library
- API key configuration
- Streaming response handling
- Error handling for API failures

---

### Phase 4: End-to-End Prompt Flow
**Objective:** Complete the full request lifecycle from workflow to response.

**Delivered:**
- Context service for fetching execution data
- Engine resolution (which LLM for which task)
- Response storage in `prompt_responses` table
- HTTP handler with SSE streaming
- Integration with dapple-prompt-flow

---

### Phase 5: ModernBERT Embeddings (Candle)
**Objective:** Add local embedding computation for semantic context selection.

**Delivered:**
- `EmbeddingBackend` trait
- `ModernBertBackend` using Candle ML
- Lazy model loading (load on first use)
- Cosine similarity computation
- Overlay service for slot ranking
- Hardware acceleration (CUDA, Metal)

---

### Phase 6: Local Mistral Inference (Candle)
**Objective:** Enable fully local LLM inference without external APIs.

**Delivered:**
- `MistralBackend` implementation
- HuggingFace Hub model downloading
- Tokenization and generation loop
- KV-cache for efficient generation
- Sampling (temperature, top-p, top-k)
- Stop sequence detection
- Hardware acceleration (CUDA, Metal)

---

### Phase 7: Polish and Configuration
**Objective:** Clean up configuration and wire sampling parameters.

**Delivered:**
- Fixed Cargo.toml edition (2024 → 2021)
- Extended `EngineConfig` with sampling parameters
- `SamplingParams::from_engine_config()` for per-engine configuration
- Updated all backends to use engine config

---

## Configuration Reference

### Environment Variables

| Variable | Description | Default | Required |
|----------|-------------|---------|----------|
| `DATABASE_URL` | PostgreSQL connection string | — | Yes |
| `ANTHROPIC_API_KEY` | Anthropic API key for Claude | — | For Anthropic |
| `MISTRAL_ENABLED` | Enable local Mistral (`true`/`false`) | `false` | No |
| `MISTRAL_MODEL_ID` | HuggingFace model ID | `mistralai/Mistral-7B-Instruct-v0.3` | No |
| `MISTRAL_DEVICE` | Inference device | Auto-detected | No |
| `MISTRAL_DTYPE` | Data type (`f16`, `bf16`, `f32`) | `f16` | No |
| `MISTRAL_MAX_CONTEXT` | Maximum context length | `8192` | No |
| `HF_TOKEN` | HuggingFace API token | — | For gated models |

### Device Options

| Value | Hardware | Notes |
|-------|----------|-------|
| `cpu` | CPU only | Slowest, works everywhere |
| `cuda:0` | NVIDIA GPU #0 | Fastest, requires CUDA |
| `cuda:1` | NVIDIA GPU #1 | For multi-GPU systems |
| `metal` | Apple GPU | For M1/M2/M3 Macs |

### Sampling Parameters

These can be configured per-engine in the database:

| Parameter | Description | Default | Range |
|-----------|-------------|---------|-------|
| `temperature` | Randomness of output | 0.7 | 0.0 - 2.0 |
| `top_p` | Nucleus sampling threshold | 0.9 | 0.0 - 1.0 |
| `top_k` | Top-k sampling | 40 | 1 - vocab size |
| `max_tokens` | Maximum response length | 2048 | 1 - context limit |

**Parameter Effects:**
- **Temperature 0** = Deterministic (always same output)
- **Temperature 0.7** = Balanced creativity
- **Temperature 1.5+** = Very creative/random
- **Top-p 0.9** = Consider tokens comprising 90% of probability mass
- **Top-k 40** = Consider only top 40 most likely tokens

---

## Deferred Work

The following items were documented during development but deferred for future consideration:

### Embedding Optimizations (docs/musings/phase5-followup.md)

| Item | Description | Benefit |
|------|-------------|---------|
| **Handler Integration** | Wire semantic ranking into prompt handler | Enable feature end-to-end |
| **LRU Cache** | Cache embedding results | Avoid recomputing for repeated text |
| **Batch Embedding** | Process multiple texts at once | GPU efficiency |
| **pgvector** | PostgreSQL vector storage | Scale to millions of embeddings |

### NVIDIA NIM Integration (docs/musings/nvidia/)

| Item | Description | Benefit |
|------|-------------|---------|
| **NIM Backend** | NVIDIA Inference Microservices | Enterprise GPU infrastructure |
| **OpenAI-Compatible API** | NIM uses OpenAI API format | Easy integration |
| **Multi-GPU** | Distribute across GPUs | Higher throughput |

### Quantization Support

| Item | Description | Benefit |
|------|-------------|---------|
| **GGUF Loader** | Load quantized models | Run on 4-8GB VRAM |
| **Q4/Q5/Q8 Support** | Various compression levels | Trade quality for size |

---

## Hardware Requirements

### For Local Mistral Inference

| Configuration | VRAM/RAM | Performance | Cost |
|---------------|----------|-------------|------|
| **NVIDIA RTX 4090** | 24GB VRAM | ~50-80 tok/s | ~$1,600 |
| **NVIDIA RTX 3090** | 24GB VRAM | ~40-60 tok/s | ~$900 used |
| **NVIDIA RTX 3060** | 12GB VRAM | ~30-40 tok/s (Q4) | ~$300 |
| **Apple M2 Pro** | 16GB unified | ~20-30 tok/s | ~$2,000 |
| **CPU Only** | 16GB+ RAM | ~2-5 tok/s | Existing |

### For ModernBERT Embeddings

| Configuration | Requirement | Notes |
|---------------|-------------|-------|
| **Minimum** | 4GB RAM | CPU inference |
| **Recommended** | 8GB RAM or 4GB VRAM | Faster embedding |

---

## Security Considerations

### Data Privacy

- **Local inference** ensures sensitive data never leaves your infrastructure
- **No external calls** for embedding computation
- **Response logging** is within your database (you control retention)

### API Keys

- Anthropic API key stored in environment variable
- HuggingFace token for gated model access
- No keys stored in code or database

### Model Security

- Models downloaded from HuggingFace (trusted source)
- Checksums verified during download
- No arbitrary code execution from models

---

## What This Enables

### Immediate Benefits

1. **Privacy-First AI** — Run models locally for sensitive data
2. **Cost Optimization** — Mix cloud and local based on needs
3. **Provider Flexibility** — Swap providers without code changes
4. **Full Observability** — Complete logging of all AI interactions
5. **Better Responses** — Smarter context selection improves quality

### Future Possibilities

1. **Fine-Tuned Models** — Load custom LoRA adapters for specialized tasks
2. **Multi-Model Pipelines** — Chain different models for complex workflows
3. **Hybrid Deployment** — Cloud burst when local capacity exceeded
4. **Cost Analytics** — Track and optimize AI spending per task/user
5. **A/B Testing** — Compare model/prompt performance at scale

---

## Glossary

| Term | Definition |
|------|------------|
| **LLM** | Large Language Model — AI systems like ChatGPT, Claude, or Mistral that generate human-like text based on input prompts |
| **Prompt** | The input text sent to an LLM, including instructions and context |
| **Token** | A word or word-piece that LLMs process; typically ~4 characters; API pricing is per-token |
| **Context Window** | Maximum amount of text an LLM can process at once (e.g., 8K, 32K, 128K tokens) |
| **Inference** | Running an AI model to get predictions/responses (as opposed to training) |
| **Streaming** | Sending data piece-by-piece instead of all at once; enables real-time display |
| **Candle** | A Rust library for running ML models locally, created by HuggingFace; like PyTorch but in Rust |
| **Mistral** | An open-source LLM from Mistral AI; comparable to GPT-3.5; can run locally |
| **Claude** | Anthropic's LLM; known for safety and long context; cloud API only |
| **ModernBERT** | A 2024 text encoder model; converts text to numerical embeddings; 8192 token context |
| **Embeddings** | Numerical representations of text (lists of numbers) that capture semantic meaning; similar texts have similar embeddings |
| **Semantic** | Related to meaning — "semantic search" finds content by meaning, not just keywords |
| **Cosine Similarity** | A math formula that measures how similar two embedding vectors are (1.0 = identical, 0.0 = unrelated) |
| **Quantization** | Compressing AI models to use less memory by reducing number precision (e.g., 16-bit → 4-bit) |
| **GGUF** | A file format for quantized models; enables running large models on consumer hardware |
| **Handlebars** | A template language using `{{variable}}` syntax for dynamic content substitution |
| **rig-core** | A Rust library for calling LLM APIs (Anthropic, OpenAI, etc.) with a unified interface |
| **CUDA** | NVIDIA's GPU programming platform; required for GPU-accelerated inference on NVIDIA cards |
| **Metal** | Apple's GPU programming platform; enables GPU acceleration on M1/M2/M3 Macs |
| **KV Cache** | Key-Value cache; stores intermediate computations during text generation to speed up subsequent tokens |
| **Sampling** | The process of selecting the next token during generation; parameters like temperature control randomness |
| **Temperature** | A sampling parameter; 0 = deterministic, 1 = balanced, >1 = more random/creative |
| **Top-p** | Nucleus sampling; only consider tokens whose cumulative probability exceeds this threshold |
| **Top-k** | Only consider the k most likely next tokens when sampling |
| **Backend** | The system component that handles requests; here: which AI provider processes a prompt |
| **Conduit** | A channel or pipe; our service acts as a conduit between workflows and AI providers |
| **SSE** | Server-Sent Events; a web protocol for streaming data from server to client |
| **Nushell** | A modern shell scripting language used in our workflow engine |
| **Slot** | A placeholder in a prompt template that gets filled with dynamic data |
| **Context** | Background information provided to an AI to inform its response |
| **HuggingFace** | A platform/company that hosts AI models, datasets, and tools; the "GitHub of ML" |
| **pgvector** | A PostgreSQL extension for storing and searching embedding vectors efficiently |
| **NIM** | NVIDIA Inference Microservices; enterprise platform for hosting AI models on NVIDIA hardware |
| **LoRA** | Low-Rank Adaptation; a technique for fine-tuning LLMs efficiently with minimal additional parameters |
| **Trait** | A Rust language feature similar to interfaces; defines shared behavior between types |
| **Crate** | A Rust package/library; our monorepo contains multiple crates |

---

## Document Locations

All implementation details are preserved in the codebase:

| Document | Location | Purpose |
|----------|----------|---------|
| Phase 1-7 Plans | `docs/changes/dapple-prompt-conduit/nushell-environment/` | Detailed implementation specs |
| Embedding Followup | `docs/musings/dapple-prompt-conduit/nushell-environment/phase5-followup.md` | Deferred optimizations |
| NVIDIA NIM | `docs/musings/nvidia/phase6-nim-integration.md` | Deferred enterprise integration |
| Development Guide | `CLAUDE.md` | Comprehensive patterns and conventions |
| Agent Instructions | `AGENTS.md` | AI coding agent guidelines |

---

## Summary

The `dapple-prompt-conduit` project successfully delivered a production-ready AI gateway that provides:

- **Multi-provider support** with easy extensibility
- **Local inference capabilities** for privacy and cost control
- **Intelligent context selection** for better AI responses
- **Complete observability** through response logging
- **Real-time streaming** for better user experience

The foundation is now in place to power AI-driven workflows across the Dapple platform, with clear paths for future enhancement documented in the musings directory.
