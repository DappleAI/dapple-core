# Persistent Context Repository — History, Memory, Scratchpad

## What the Paper Says

The paper introduces a taxonomy of persistent context types (Table I in the paper), arguing that current systems conflate different kinds of memory with different lifecycle requirements. The proposed "Persistent Context Repository" (PCR) organizes context into distinct categories, each with appropriate storage, retention, and access patterns:

| Category | Description | Lifecycle |
|----------|-------------|-----------|
| **Scratchpad** | Working memory for in-progress reasoning | Ephemeral, discarded after task completion |
| **Episodic Memory** | Records of specific interactions and their outcomes | Retained per-session or per-task, queryable |
| **Fact Memory** | Declarative knowledge extracted from sources | Long-lived, updated when source changes |
| **Experiential Memory** | Lessons learned, patterns discovered across interactions | Accumulated over time, curated |
| **Procedural Memory** | How-to knowledge, workflows, scripts | Versioned, updated deliberately |
| **User Memory** | Preferences, history, personalization data | Per-user, privacy-sensitive |
| **Historical Record** | Immutable audit log of all actions and decisions | Append-only, never modified |

The paper argues that each category needs different storage semantics: scratchpads should be fast and disposable, historical records should be append-only and immutable, fact memory should be updateable but with version tracking.

## What dapple-prompt Does

Every category in the paper's taxonomy has a concrete dapple-prompt equivalent — though we organize by database table and namespace rather than by filesystem directory.

### Historical Record

The paper's most foundational category — immutable audit logs — is our most deeply embedded pattern.

**Append-only `rev.*` tables.** The entire `rev` schema follows append-only semantics. No UPDATE operations in normal flow. State changes create new records: new versions via PATCH, new attempts for each dispatch step execution, new events for each directive evaluation.

**`rev.task_attempts`** records every task attempt with status (`queued`, `running`, `failed`, `ok`, `canceled`, `unsafe`), duration, error codes, and timestamps. Dispatch events (`insert_task_attempt_event_with_attempt_id`) record the directive evaluation that led to each attempt.

**Content-addressable versioning** on `rev.versioned_objects` provides cryptographic verification. You can prove that a versioned entity is identical to a previous snapshot by comparing SHA-1 hashes. The Merkle pattern propagates changes upward: modifying a step changes the stage hash, which changes the playbook hash.

**Deprecation events** record why and when a version was deprecated, preserving the full lifecycle.

### Episodic Memory

**Journey context** stored in JSONB columns (`context_data`, `context_metadata`) captures the evolving state of a user's workflow session. Each dispatch step reads from and writes to these namespaces:

- `data.*` — Canonical payloads from task execution (persisted)
- `metadata.*` — Control-plane state, signals, validation results (persisted)

The dispatch runner reconstructs full context from the database before each step execution. This is episodic memory — it records what happened in a specific journey, in order, with full fidelity.

### Fact Memory

**Guide content embeddings** in pgvector (`rev.guide_content_embeddings`) are declarative knowledge extracted from guide source material. Each embedding records:

- `chunk_text` — the source text
- `embedding` — the 768-dimensional vector representation
- `metadata` — JSONB with directive type, block name, chapter context

Tag (`rev.guide_content_embedding_tags`) and locale (`rev.guide_content_embedding_locales`) junction tables enable structured retrieval.

**Fluent message bodies** (`rev.fluent_messages`, `rev.fluent_message_sets`) store localized text resolved from `.ftl` files. These are fact memory: they represent what a guide says in each locale, updated only when the guide is re-ingested.

### Procedural Memory

**Nushell task scripts** stored as step content define how-to knowledge: the actual executable procedures that dispatch steps run. These are versioned through the content-addressable system — modifying a script creates a new step version.

**Playbook/stage/step definitions** are the procedural structure: which steps execute in which order, with which directives controlling flow. The `IngestPlan`'s `derived_playbook` contains the full hierarchy, materialized atomically.

**Directive definitions** (JSONLogic rules on stages and steps) encode conditional logic: when to retry, when to branch, when to skip. These are procedural memory about decision-making, not just execution.

### Scratchpad

**`temp_data.*` namespace** in dapple-prompt-flow is the explicit scratchpad. It provides engine scratch space for intermediate computations during dispatch execution. Unlike `data.*` and `metadata.*`, `temp_data.*` is not persisted to the database — it exists only during step execution and is discarded afterward.

**Draft chunks** (`rev.draft_chunks`) serve as a scratchpad for binary asset construction. Chunks are appended during creation, then finalized into immutable `rev.chunks` and linked via `rev.asset_chunks`. The draft is working memory; the asset is permanent.

### User Memory

**Debrief responses** capture human input during journey execution. When a step requires human-in-the-loop evaluation, the debrief system collects responses that become part of the journey context.

**Journey state** itself is per-user memory: which playbook was selected, how far through execution, what choices were made at branch points.

### Experiential Memory

This is the one category where the paper's taxonomy reveals a gap in our architecture. dapple-prompt does not currently have a mechanism for **accumulated cross-journey lessons learned**. Each journey is independent. Patterns discovered in one journey don't automatically inform future journeys.

The closest analog is the **guide system itself**: guides encode best practices and procedural knowledge authored by humans who have observed patterns across many interactions. But this is manual curation, not automatic accumulation.

## Comparison

**Alignment:** Six of the paper's seven memory categories have direct, well-implemented equivalents in dapple-prompt. Our append-only data model provides stronger historical record guarantees than a file-based audit log — database transactions ensure consistency that filesystem operations cannot guarantee.

**Divergence:** The paper organizes by memory category with different storage backends. We organize by database schema and table, with lifecycle semantics enforced by application-level conventions (append-only, content-addressable hashing) rather than storage-level differences. Our approach is simpler (one storage system) but means lifecycle rules must be documented and enforced in code rather than by infrastructure.

The paper gives scratchpad and episodic memory equal architectural weight. In dapple-prompt, `temp_data.*` is informal — it's a convention in the flow engine, not a first-class architectural concept with defined lifecycle rules.

**Gap — Experiential Memory:** The paper's experiential memory category — automatically accumulated lessons across interactions — has no equivalent in our system. This is worth considering: could dispatch execution metrics, directive evaluation patterns, or RAG retrieval effectiveness be aggregated across journeys to improve future context construction?

## TLDR

The paper's seven-category memory taxonomy (scratchpad, episodic, fact, experiential, procedural, user, historical) maps cleanly to dapple-prompt's architecture: `temp_data.*` for scratchpad, journey JSONB context for episodic, pgvector embeddings and Fluent messages for fact, Nushell scripts and playbook definitions for procedural, debrief responses for user, and append-only `rev.*` tables for historical. The gap is experiential memory — cross-journey learning from accumulated execution patterns — which we don't currently implement.
