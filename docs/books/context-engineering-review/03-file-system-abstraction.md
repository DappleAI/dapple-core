# File System Abstraction — Infrastructure Principles

## What the Paper Says

The paper's central contribution is mapping five classical software engineering principles to context engineering via a file system abstraction. The argument: just as operating systems use the file system to provide uniform access to diverse hardware, an "Agentic File System" (AFS) should provide uniform access to diverse context sources.

The five principles:

1. **Abstraction** — A uniform file interface hides the heterogeneity of underlying context sources (databases, APIs, documents, memories). Every context element is accessed through the same read/write/list operations.

2. **Modularity & Encapsulation** — Components are mounted at known paths with clear boundaries. An agent's memory module doesn't know or care how the document retrieval module works internally.

3. **Separation of Concerns** — Data storage, tool execution, and governance are distinct layers. The file system provides data; tools operate on data; governance policies control access and lifecycle.

4. **Traceability & Verifiability** — Every interaction is logged as a filesystem transaction. You can reconstruct what an agent read, when, and what it produced — enabling audit, debugging, and compliance.

5. **Composability & Evolvability** — New backends can be added as filesystem plugins without changing existing components. The abstraction layer absorbs change.

## What dapple-prompt Does

Each principle maps to concrete dapple-prompt mechanisms:

### 1. Abstraction

The paper uses a uniform file interface to hide heterogeneity. dapple-prompt achieves this through two complementary abstractions:

**sql-composer's `:compose()` and `:bind()` directives** provide a uniform query interface. Regardless of whether you're inserting a task, a playbook, or an embedding, the pattern is identical: `.sqlc` source files use `:bind(name)::TYPE` for parameters and `:compose(shared/file.sqlc)` for shared CTE fragments. The generated `.sql` files are consumed by `query_file_as!` with compile-time verification. The query author doesn't need to know whether the entity is versioned, has path objects, or uses associative tables — the shared CTEs handle that.

**Guide directives normalize heterogeneous content.** A guide can contain prose, stage definitions, step definitions, Nushell code blocks, brief packs, inference profiles, and cross-references. The parser reduces all of these to `ParsedBlock` records with a uniform shape: `directive_type`, `name`, `content`, `attributes`, `tags`. Downstream consumers (materialization, embedding, rendering) operate on this uniform type.

### 2. Modularity & Encapsulation

The paper's mounted components map to our **Cargo workspace crates**:

| Crate | Encapsulated Concern |
|-------|---------------------|
| `dapple-data` | PostgreSQL access, SQL queries, data models |
| `dapple-parser` | Guide content parsing, transform to IngestPlan |
| `dapple-render` | PDF generation with tag-based filtering |
| `dapple-prompt-api` | REST API, request/response schemas, handlers |
| `dapple-prompt-conduit` | LLM backends, embeddings, RAG, TTS |
| `dapple-prompt-flow` | Workflow orchestration, Nushell execution, directives |
| `dapple-fluent` | Fluent template parsing and rendering |

Each crate has its own `AGENTS.md` documenting its responsibilities and boundaries. Cross-crate dependencies are explicit in `Cargo.toml`. A change to the parser's internal representation doesn't propagate to the API layer — the `IngestPlan` type is the contract.

**LLM backends implement the `LlmBackend` trait.** Adding Anthropic, Mistral, or a stub backend requires implementing `execute_streaming()` and `supports()`. The prompt handler routes to the correct backend without knowing its internals. The same pattern applies to `EmbeddingBackend` for embedding models.

### 3. Separation of Concerns

The paper separates data, tools, and governance. dapple-prompt separates via **database schemas** and **crate boundaries**:

| Layer | Paper's Term | dapple-prompt Implementation |
|-------|-------------|------------------------------|
| **Data** | File storage | `rev` schema (versioned content, audit trail), `web` schema (user-facing) |
| **Tools** | Tool execution | `dapple-prompt-flow` (Nushell scripts, dataflow engine), `dapple-prompt-conduit` (LLM/TTS/embedding) |
| **Governance** | Access control, lifecycle | `auth` schema (NextAuth.js), content-addressable versioning, deprecation events, tag-based rendering profiles |

The data layer (`dapple-data`) never executes prompts. The flow engine (`dapple-prompt-flow`) never writes SQL directly — it calls `dapple-data` query functions. The API layer validates and routes but delegates all business logic.

### 4. Traceability & Verifiability

The paper logs every interaction as a filesystem transaction. dapple-prompt achieves traceability through:

- **Append-only data model** — No UPDATE operations in normal flow. State changes create new records (new versions, new attempts, new events).
- **`rev.task_attempts`** — Every dispatch step execution is recorded with status, duration, error codes, and timestamps.
- **Dispatch events** — `insert_task_attempt_event_with_attempt_id` records directive evaluations, branches, retries, and completions.
- **Content-addressable hashing** — SHA-1 hashes on `rev.versioned_objects` let you verify that a versioned entity hasn't changed. The Merkle pattern (parent hashes include child hashes) means verifying a playbook hash verifies the entire hierarchy.
- **Guide block provenance** — `rev.guide_block_provenance` links blocks to the derived objects (tasks, steps, stages, prompts) they generated during ingest.

### 5. Composability & Evolvability

The paper's plugin architecture maps to our **trait-based backends** and **sql-composer shared CTEs**:

- **`LlmBackend` trait** — New LLM providers are added by implementing two methods. The `supports()` method declares which model configurations the backend handles; `execute_streaming()` does the work.
- **`EmbeddingBackend` trait** — Same pattern for embedding models. Swapping from ModernBERT to a different model requires implementing `embed()`, `dimension()`, and `name()`.
- **sql-composer shared CTEs** — Six shared `.sqlc` files (`insert_object_paths.sqlc`, `numbered_path_details.sqlc`, `insert_versioned_objects.sqlc`, `patch_new_versions.sqlc`, `deprecate_latest_versions.sqlc`, `deprecate_events.sqlc`) are composed into entity-specific queries via `:compose()`. Adding a new versioned entity type reuses these shared fragments without duplicating the versioning logic.

## Comparison

**Alignment:** All five principles are present in dapple-prompt, often with stronger guarantees. Content-addressable hashing provides cryptographic verifiability that filesystem metadata cannot. ACID transactions provide atomicity that filesystem operations must simulate. Rust's type system and compile-time SQL verification provide a level of safety that dynamic filesystem APIs cannot.

**Divergence:** The paper's abstraction is **universally uniform** — everything is a file with the same interface. Our abstractions are **type-specific** — a `ParsedBlock` is not the same type as a `TaskModel` or an `InsertPlaybookArgs`. We trade universality for type safety. You cannot accidentally pass a guide where a playbook is expected; the compiler catches it. The filesystem approach would require runtime validation for the same guarantee.

**What we can learn:** The paper's emphasis on **mount points** — well-known paths where components are expected — is an organizational pattern we use implicitly (crate boundaries, schema names, `assets/sqlc/` directory structure) but don't formalize. Explicit documentation of our "mount points" (where each type of data lives, how to find it, what interface it exposes) could improve navigability.

## TLDR

The paper maps five SE principles (abstraction, modularity, separation of concerns, traceability, composability) to a file system metaphor. dapple-prompt implements all five through database schemas, Cargo workspace crates, trait-based backends, sql-composer shared CTEs, and content-addressable hashing. Our approach trades the universality of "everything is a file" for type safety and ACID guarantees. The paper's "mount point" formalization is a useful organizational concept we could adopt more explicitly.
