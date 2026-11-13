# Implementation — AIGNE vs dapple-prompt

## What the Paper Says

The paper presents AIGNE (AI-native aGENt Engine) as a reference implementation of the Agentic File System (AFS) and context engineering pipeline. Key characteristics:

**Technology stack:** TypeScript/Node.js. SQLite for persistent context storage. MCP (Model Context Protocol) integration for tool access. File-system metaphor throughout — agents interact with a virtual filesystem where different paths map to different context sources.

**AFS module:** Implements the virtual filesystem with mount points for memory types (scratchpad, episodic, procedural, etc.), tool directories, and governance stores. Standard file operations (read, write, list, delete) provide uniform access.

**Storage:** SQLite for local persistence. The paper emphasizes simplicity and portability — SQLite runs anywhere without a server.

**Exemplar agents:** The paper demonstrates two agent patterns:
1. **Memory-enabled agent** — Uses the AFS to persist context across conversation turns, reading from episodic memory and writing observations back
2. **MCP GitHub assistant** — Integrates external tools (GitHub API) via MCP, with the AFS mediating between tool output and agent context

**Design philosophy:** Accessibility and simplicity. The paper emphasizes that AIGNE should be easy to set up, require no infrastructure, and provide immediate value. The tradeoff is explicit: simplicity over performance, portability over power.

## What dapple-prompt Does

The architectural choices diverge significantly at the implementation level:

### Technology Comparison

| Dimension | AIGNE | dapple-prompt |
|-----------|-------|---------------|
| **Language** | TypeScript/Node.js | Rust |
| **Storage** | SQLite (single file) | PostgreSQL (server, schemas, pgvector) |
| **Query verification** | Runtime | Compile-time (`query_file_as!`, `cargo sqlc compose --verify`) |
| **Scripting** | JavaScript/TypeScript | Nushell (sandboxed, security policies) |
| **Vector search** | External service or plugin | Built-in pgvector extension |
| **LLM integration** | MCP protocol | Trait-based backends (`LlmBackend`, `EmbeddingBackend`) |
| **Serialization** | JSON | JSON (API) + JSONB (storage) + compile-time types (internal) |
| **Deployment** | Single process, zero infrastructure | Kubernetes, multiple services, registry |
| **Localization** | Not addressed | First-class via Fluent `.ftl` files |
| **Concurrency** | Node.js event loop | Tokio async runtime, parallel dispatch steps |

### Metaphor Comparison

**AIGNE abstracts everything as files.** An agent's episodic memory is at `/memory/episodic/`, its scratchpad is at `/memory/scratchpad/`, tool outputs appear at `/tools/github/`, and governance policies live at `/governance/`. The file system metaphor provides universal familiarity — every developer knows `read()` and `write()`.

**dapple-prompt abstracts everything as versioned relational objects.** A guide is a row in `rev.guides` with foreign keys to chapters, blocks, tags, and assets. A playbook is a versioned object in `rev.versioned_objects` with a content hash, path object, and version triple. A dispatch step is a record in `rev.task_attempts` with JSONB context columns.

The relational approach makes **relationships explicit**. You can query "all blocks in chapter 3 of guide X tagged with 'agent' in locale 'en'" with a single SQL join. The filesystem approach would require traversing paths, reading file contents, and filtering in application code.

The relational approach makes **transactions atomic**. `materialize_guide()` inserts the guide, all chapters, all blocks, all tags, all assets, the card, and all derived objects in a single database transaction. If any step fails, nothing is committed. The filesystem approach requires application-level rollback logic.

### Exemplar Comparison

**Memory-enabled agent** vs **Journey with persistent context:**

AIGNE's memory-enabled agent reads from `/memory/episodic/` at the start of each turn and writes observations back. dapple-prompt's journey context serves the same purpose: the dispatch runner loads `context_data` and `context_metadata` from JSONB before each step, executes the step, and writes results back. The difference is that journey context is structured into typed namespaces (`data.*`, `metadata.*`, `temp_data.*`) with defined lifecycle rules, while AIGNE's filesystem entries are unstructured files.

**MCP GitHub assistant** vs **Guide ingest + embedding + RAG pipeline:**

AIGNE's MCP assistant retrieves GitHub data through the MCP protocol and stores it in the AFS for agent consumption. dapple-prompt's equivalent pipeline ingests structured content (guides), transforms it through a typed pipeline (`parse_guide() -> plan_guide() -> materialize_guide()`), generates embeddings asynchronously, and serves it via RAG retrieval. The key difference is that our pipeline produces **indexed, searchable, versioned** artifacts, while MCP integration produces ephemeral tool output.

### Security Model

**AIGNE:** Relies on filesystem permissions and MCP's capability model for access control. The paper mentions governance as a concern but the implementation details are thin.

**dapple-prompt:** `StepEngine` enforces security at parse time. External commands are always blocked. Environment mutation is always blocked. Nushell commands must be on an explicit allowlist. The `SecurityPolicy` is checked before any script executes, not after. Database-level schema isolation (`auth`, `web`, `rev`) prevents cross-concern data access.

### ONNX and Local Inference

**AIGNE:** Does not address local model inference. LLM access is via API calls (OpenAI, Anthropic via MCP).

**dapple-prompt:** The conduit service runs local inference via ONNX: ModernBERT for embeddings (`EmbeddingBackend`), ModernBERT for masked language modeling (`MlmService`), VITS for text-to-speech (`TtsService`), and Flan-T5 for summarization (`SummarizationService`). Model files are auto-provisioned from HuggingFace during deployment. This enables operations without external API dependencies.

## Comparison

**Alignment:** Both systems recognize that context persistence is the critical missing piece in current agent frameworks. Both provide structured storage, both version context, both support multiple memory types. The goals are identical.

**Divergence — Simplicity vs Power:** This is the fundamental tradeoff. AIGNE optimizes for accessibility: TypeScript, SQLite, zero infrastructure, familiar filesystem metaphor. dapple-prompt optimizes for correctness and scale: Rust, PostgreSQL, compile-time verification, ACID transactions, Kubernetes deployment. Neither is wrong — they serve different stages of adoption and different reliability requirements.

**Divergence — Abstraction Level:** The filesystem metaphor is lower-level and more universal. The relational model is higher-level and more specialized. You can build a relational model on top of a filesystem (that's what PostgreSQL does), but you lose the file system's universality. You can simulate a filesystem on top of a relational model, but you lose the database's query power.

**Divergence — Localization and Multi-Modality:** AIGNE does not address localization or multi-modal content (audio, images, video). dapple-prompt's Fluent integration, chunked asset storage, and ONNX-based TTS/embedding/MLM services address these as first-class concerns.

## TLDR

AIGNE is a TypeScript/SQLite reference implementation optimizing for simplicity and accessibility. dapple-prompt is a Rust/PostgreSQL production system optimizing for correctness, type safety, and scale. They share the same goals (persistent, versioned, auditable context) but make opposite tradeoffs: filesystem universality vs relational power, zero-infrastructure setup vs Kubernetes deployment, runtime flexibility vs compile-time verification. Both validate the paper's thesis that context persistence is architecturally essential — they just implement it at different points on the simplicity-power spectrum.
