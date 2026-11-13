# Context Engineering Pipeline — Constraints and Components

## What the Paper Says

The paper identifies three fundamental design constraints that any context engineering system must address, then proposes three pipeline components that operate under those constraints.

### Design Constraints

1. **Token Window** — The context window is a hard architectural constraint. You cannot send everything; you must curate, compress, or stream. This is not a limitation to work around — it's a design parameter that shapes every decision.

2. **Statelessness** — LLMs don't retain state between calls. Every invocation starts from scratch. The system must externalize all state and reconstruct context before each call.

3. **Non-Determinism** — The same input can produce different outputs. For audit, debugging, and governance, you must preserve input-output pairs. You cannot rely on reproducibility; you must record actuality.

### Pipeline Components

1. **Context Constructor** — Selects, retrieves, and assembles context from heterogeneous sources into a coherent prompt. Produces a "context manifest" documenting what was included, what was excluded, and why.

2. **Context Updater** — Processes new information (user input, tool output, observation) and integrates it into the persistent state. Handles incremental updates without full reconstruction.

3. **Context Evaluator** — Assesses whether the context produced good results. Evaluates output quality, triggers retry or correction, and feeds back into the Constructor for future improvements.

## What dapple-prompt Does

### Constraint: Token Window

**InferenceProfile configs** carry `temperature`, `max_tokens`, `top_p`, `top_k` — parameters that bound the model's behavior. These are stored in `rev.inference_profiles` and `rev.inference_profile_configs`, versioned like all other objects.

**RAG retrieval** (`RagService::retrieve()`) manages token budget by limiting the number of retrieved chunks. The `RagContext` struct specifies `limit` (default 10) and `exclude_block_ids` (blocks from prior steps, preventing duplication). Retrieved chunks are formatted as numbered `# Reference Context` sections with similarity percentages, giving the model structured rather than raw context.

**Fluent localization** reduces redundancy at the source level. A guide's content exists once per locale in `.ftl` files. At prompt or render time, only the relevant locale is resolved. Without this, multilingual content would multiply the context requirement by the number of supported locales.

**EmbeddingClient timeout** (120 seconds) and `EMBEDDING_DIMENSION` (768 with Matryoshka truncation) are engineering decisions shaped by the token window constraint — producing efficient, fixed-dimension representations of arbitrary-length text.

### Constraint: Statelessness

**PostgreSQL is our external state.** This is not incidental — it's the architectural foundation. Every piece of context that an LLM needs is reconstructed from the database before each call:

- The `dispatch_runner` loads the full dispatch context (`context_data`, `context_metadata`) from JSONB columns before executing each step.
- `ModelService::resolve_model_for_dispatch()` queries `rev.inference_profiles`, `rev.inference_profile_configs`, and `rev.model_releases` to determine which model to use and with what parameters.
- `RagService::retrieve()` queries pgvector to find relevant context blocks.
- Fluent message resolution queries `rev.fluent_messages` for the appropriate locale.

Nothing is cached in-process across LLM calls (except the Nushell parse cache in `StepEngine`, which is deterministic). The system treats every dispatch step as a fresh computation with all state loaded from the database.

### Constraint: Non-Determinism

**`rev.task_attempts`** records every execution: input context, output, status, duration, error codes. You can reconstruct exactly what the model saw and what it produced for any historical dispatch step.

**Dispatch events** record directive evaluations. When a `DirectiveEvaluator` triggers retry, branch, or skip, the decision is logged with the attempt ID. This creates a complete causal chain: input → model output → directive evaluation → action taken.

**Content-addressable hashing** ensures that the version of every input artifact (guide, playbook, step, task, prompt, inference profile) is recorded immutably. Even if the same entity is later modified, the version that was active during a specific dispatch is cryptographically pinned.

### Pipeline Component: Context Constructor

dapple-prompt's context construction happens in two distinct phases:

**Ingest-time construction** — `plan_guide()` in `dapple-parser` is a pure function that transforms a `ParsedGuide` into an `IngestPlan`. This is context construction in the paper's sense: it selects which blocks to include, groups Fluent messages by key, extracts brief packs, builds the playbook hierarchy, and collects asset files. The `IngestPlan` is an explicit record of what was constructed and how.

**Prompt-time construction** — `RagService::retrieve()` performs the runtime equivalent. Given a prompt text (or `query_hint`), it embeds the query, searches pgvector with tag and locale filters, excludes previously retrieved blocks, and formats the results as structured context. The `RagResult` records what was retrieved (`retrieved_block_ids`, `chunk_count`, `context_text`).

**Gap — Context Manifest:** The paper emphasizes that the Constructor should produce an explicit manifest of what was selected and what was excluded, with rationale. Our `IngestPlan` captures what was included and our `RagResult` captures what was retrieved, but neither records what was *excluded* or *why*. This is a concrete improvement opportunity.

### Pipeline Component: Context Updater

**Dispatch runner orchestration** is our Context Updater. It processes workflow steps incrementally:

1. Load current context from JSONB (`context_data`, `context_metadata`)
2. Execute the current step (Nushell script, LLM prompt, or dataflow task)
3. Write step output to `data.*` namespace
4. Write signals and control metadata to `metadata.*` namespace
5. Evaluate directives to determine next action
6. Persist updated context back to JSONB

This is incremental context updating — each step adds to the accumulated context without requiring full reconstruction from scratch. The `data.*` namespace provides a clean append model: each task writes to its own key, previous task outputs remain accessible.

### Pipeline Component: Context Evaluator

**The directive system** is our Context Evaluator. `DirectiveEvaluator` uses JSONLogic to assess step outputs against conditional rules:

- **`proceed`** — Output is acceptable, continue to next step
- **`retry`** — Output is unacceptable, re-execute after delay (up to `max_attempts`)
- **`loop`** — Output needs refinement, re-execute immediately
- **`skip`** — Output is irrelevant, skip this step entirely
- **`fail`** — Output is fatally wrong, halt execution
- **`{uuid}`** — Output triggers a branch to a different stage or step

The `run` operator integrates execution with evaluation: the directive can invoke the step (`{"run": []}`) and then evaluate the result in the same JSONLogic expression. This tightly couples execution with assessment, which is exactly the paper's vision of the Evaluator feeding back into the Constructor.

**Human-in-the-loop evaluation** happens through the debrief system. Steps can require human input before proceeding, making humans explicit participants in the evaluation loop — not just consumers of output.

## Comparison

**Alignment:** The three constraints (token window, statelessness, non-determinism) are precisely the constraints dapple-prompt was designed around. Our PostgreSQL-centric architecture directly addresses statelessness. Content-addressable hashing and audit trails directly address non-determinism. RAG retrieval with limits and Fluent localization directly address token window management.

The three pipeline components (Constructor, Updater, Evaluator) map cleanly to our ingest/RAG pipeline, dispatch runner, and directive system respectively. The paper's architecture and ours converge on the same decomposition.

**Divergence:** The paper treats the three components as stages in a single pipeline. In dapple-prompt, they operate at different time scales:

- **Constructor** runs at ingest time (minutes to hours between invocations) and at prompt time (per-step RAG retrieval)
- **Updater** runs during dispatch execution (seconds between steps)
- **Evaluator** runs after each step and can trigger loops that blend Updater and Evaluator

This temporal separation is an architectural strength — ingest-time construction can be thorough and expensive because it happens infrequently, while prompt-time construction must be fast.

**Key gap:** The paper's emphasis on the **context manifest** — an explicit record of what was included, excluded, and why — is the most actionable insight. Our `RagResult` returns `retrieved_block_ids` but doesn't record the full candidate set, similarity scores for excluded candidates, or the rationale for exclusion. Adding this would improve debugging and auditability of RAG retrieval.

## TLDR

The paper's three constraints (token window, statelessness, non-determinism) and three pipeline components (Constructor, Updater, Evaluator) map directly to dapple-prompt's architecture: InferenceProfiles and RAG limits for token management, PostgreSQL for state externalization, task_attempts for non-determinism audit, the ingest/RAG pipeline as Constructor, dispatch_runner as Updater, and the directive system as Evaluator. The key gap is the paper's "context manifest" concept — recording not just what was retrieved but what was excluded and why.
