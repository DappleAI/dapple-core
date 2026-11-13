# Content Pipeline + Scheduler + Agent Knowledge — project plan

> **Rough scope:** 7-10 weeks of focused work. This is explicitly a large plan; the user opted for full scope over splitting it into multiple landings, with the understanding that implementation can still be staged internally. `docs/plans/brief-flow-improvements.md` already exists and captures the preceding work; this plan starts where that one ended. Exploratory thinking about the dapple-shell microservice and dataset pipeline is parked in `docs/musings/dapple-shell-datasets.md`.

## Context

The previous plan (`brief-flow-improvements.md`) shipped brief rendering in Studio, markdown brief units, journey-wide binding seeding in `dispatch_runner`, and the `GET /api/dispatches/{id}/briefs` endpoint. It stopped short of actually running LLM-heavy task scripts that need to consume uploaded content (PDF, docx) or wait on async work (extraction, future HITL, future callbacks).

Three observations drove the scope of this plan:

1. **`pdf extract` in its current shape is broken** in four concrete ways — derived content shows up as its own `rev.assets` row, versioning between source and derivation drifts, re-extraction is awkward, and linking source↔derivation is clumsy. Fixing it cleanly requires a new storage model, not a patch.
2. **Tasks consuming extraction output need to wait for async work**, and the current dispatch_runner has no wait primitives. Adding a command-level polling loop (P1) works for the simple case but doesn't extend to HITL, external LLM callbacks, or sub-dispatches — all of which the user confirmed are coming soon. A proper scheduler with a generic wait framework (P2) is load-bearing for those future features.
3. **Agents running behind conduit have no built-in understanding** of the brief/binding/asset/dispatch model. Today's inference profile system prompts have to be hand-authored to describe the model, and they drift as the model evolves. Runtime context injection (with conduit querying `dapple-prompt-api` for task context) gives agents a consistent, centrally-maintained model awareness without polluting every profile.

Accepting all three means this plan touches three subsystems simultaneously: the content pipeline (PDF / docx extraction via the new `asset_extractions` model), the dispatch scheduler (generic wait framework with extraction as the first resolver), and conduit's prompt rendering pipeline (runtime context injection sourced from the api layer). The user is aware of the size and wants them tackled together to maintain architectural coherence.

## Architectural decisions

### AD-1: Extraction storage via `rev.asset_extractions` + chunk reuse

Extracted content is NOT a new `rev.assets` row. It lives in a dedicated `rev.asset_extractions` table keyed by `(asset_id, extractor_kind, extractor_version)`, reusing `rev.chunks` for byte storage via a new `rev.asset_extraction_chunks` associative table. Extractions are properties of an asset, not artifacts in their own right.

### AD-2: Keep history of extractions (append-only)

`UNIQUE(asset_id, extractor_kind, extractor_version)` — re-extraction creates new rows, old rows stay. Matches the append-only style of the rest of the data model. Queries filter by latest version via `MAX(extractor_version)` or an `is_current` flag depending on need.

### AD-3: Two-layer chunking with positions in a separate table

- **Storage chunks** (byte-aligned, in `rev.chunks` via `rev.asset_extraction_chunks`) — durable full-text retrieval
- **Semantic chunks** (token-aligned, ~500 tokens with 100-token overlap, in `rev.asset_content_embeddings`) — pgvector similarity search
- **Position map** in a separate `rev.asset_extraction_positions` table — normalized, joinable independently of embeddings

### AD-4: Generic scheduler wait framework (P2), extraction as first resolver

`dispatch_runner` gains a real scheduler replacing its current nested for-loop. The scheduler maintains ready and waiting queues and routes tasks based on declared input requirements. The wait kind is a discriminator — `extraction` is implemented first, but `human_input`, `llm_callback`, and `sub_dispatch` are first-class future wait types that plug into the same framework via resolver registration.

### AD-5: dispatch_runner decomposition into `flow::*` modules

The scheduler commitment implies decomposing the existing `dispatch_runner.rs` god object. Target module structure:

- `flow::scheduler` — ready/waiting queue, wait resolution, wake paths, deadlock/timeout detection
- `flow::executor` — runs a single task end-to-end (parse script, execute nushell, capture output)
- `flow::dispatch_runner` — thin coordinator owning the dispatch lifecycle, calling scheduler + executor
- `flow::extractors` — background extraction worker pool, `rev.background_jobs` queue, extractor registry
- `flow::extractors::pdf`, `flow::extractors::docx` — per-extractor implementations
- `flow::data_services` — binding seeding, brief resolution, journey context fetching (cleanup of shipped code)
- `flow::audit` — task_attempt_events wrapper

Shipped code moves as needed; no functional changes, just relocations.

### AD-6: Task declaration via AST scan (Approach B), polling fallback for dynamic cases

The scheduler derives task requirements by walking the parsed nushell AST at dispatch creation time, looking for `asset text $binding_var.asset_id`-style patterns. Static patterns populate `rev.dispatch_task_required_extractions`. Tasks whose requirements can't be statically determined (dynamic IDs, computed paths) run immediately and fall back to inline polling via the `asset text --wait` command — which is the P1 primitive, kept as an escape hatch.

The polling loop is written as a reusable module (`flow::scheduler::wait_polling` or similar) so the scheduler and the inline fallback share the same underlying implementation.

### AD-7: LISTEN/NOTIFY wake path

The scheduler uses Postgres `LISTEN/NOTIFY` to learn when a wait is resolved. Extraction worker issues `NOTIFY wait_resolved` with a JSON payload `{wait_kind, wait_target_id}` after marking an extraction row `ok`. The scheduler holds a dedicated `PgListener` connection, receives events, and re-evaluates affected waits. Cross-process ready from day one; no throwaway polling work to replace later.

### AD-8: Extraction via chained background jobs

`rev.background_jobs` is a generic job queue with a `job_kind` discriminator. Extraction jobs produce text + storage chunks + position map. On extraction completion, an embedding job is enqueued. The embedding job produces semantic chunks in `rev.asset_content_embeddings`. Independent retry semantics — if embedding fails, extraction is still usable for full-text retrieval; only `asset search` errors.

### AD-9: Lazy re-extraction trigger + admin backfill

When an extractor's version is bumped, existing assets are NOT automatically re-extracted at startup. The next task that requests the current version triggers re-extraction for that asset. An admin command (`dapple-prompt-api ... extraction backfill --kind pdf-text --since-version 2`) enqueues bulk re-extraction when an operator wants to proactively refresh everything.

### AD-10: New `rev.asset_content_embeddings` table, 768-dim ModernBERT

Dedicated schema scoped to assets, mirrors `rev.guide_content_embeddings`'s shape but with asset-specific columns. Uses the same ModernBERT embedding service conduit already runs (768-dim, Matryoshka truncation). HNSW index for similarity search.

### AD-11: Four retrieval commands for task scripts

- `asset text $id` — full extracted text as a UTF-8 string (with optional `--wait` flag for the inline-polling fallback)
- `asset search $id $query` — semantic search within one asset's content, returns top-N snippets with scores and source positions
- `asset status $id` — extraction state + metadata
- `asset chunks $id` — streamed chunks for large-file incremental processing

### AD-12: Trust `officemd_docx` v0.1.6 for docx extraction

Commit to `officemd_docx = "0.1.6"` without pending verification. Course-correct at implementation time if it falls short (fall back to `dotext` or `zip`+XML parsing).

### AD-13: PDF commands removed outright; guides migrate

Existing `pdf fetch`/`pdf extract` commands are removed in the same landing. Any guides that reference them get migrated to the new `asset text`/`asset search` surface. Zero keep-wrappers. Clean slate.

### AD-14: Agent knowledge via runtime injection, conduit callbacks to api

Conduit's prompt rendering pipeline is extended to inject task context at render time. The context is sourced via a new API endpoint `GET /api/dispatches/{id}/task_context/{task_id}` that returns a structured payload (bindings summary, asset extraction availability, dispatch lifecycle position, brief unit context). **Conduit does NOT gain direct DB access.** It stays a narrow LLM gateway with an HTTP client; api remains the DB authority.

### AD-15: Shared agent-model guide under `_shared/dapple/agent-model/`

Conceptual knowledge about the brief/binding/asset/dispatch model lives as a multi-chapter dapple guide in `dapple-guides/_shared/dapple/agent-model/`. Chapters include binding-namespace, asset-surface, dispatch-lifecycle, brief-structure (at minimum). The guide is ingested through the normal shared-content pipeline; its chapters get embeddings in `rev.guide_content_embeddings` like any other guide.

### AD-16: Implicit overlay inference via embedding similarity

Inference profiles do NOT explicitly declare which agent-model chapters they want. Instead, conduit computes embedding similarity between the profile's static system prompt and each agent-model chapter, and activates chapters above a threshold. The result is cached per profile. An explicit override field is available as a safety valve but not required.

Rationale: zero authoring burden, graceful evolution (new chapters auto-activate for existing profiles that mention their concepts), and the coupling between "what the profile talks about" and "what context it has" stays natural. Accepts the risk that embedding similarity is slightly magical and may need threshold tuning.

### AD-17: Unified retrieval pipeline with two similarity sources

Conduit's prompt rendering now queries both `rev.guide_content_embeddings` (existing) AND `rev.asset_content_embeddings` (new) for task-relevant content. Results are merged, deduped by chunk content hash, and injected. The current RAG path is extended to include the new source, not retired.

### AD-18: Full 7-10 week plan, not split

Scheduler rewrite + content pipeline + agent knowledge ship together. Implementation can still be staged internally (scheduler first, then extraction workers, then agent knowledge), but the plan captures all three as one coordinated effort.

## Target architecture

### Scheduler and task execution

```
  rev.dispatches
    → rev.dispatch_stages
      → rev.dispatch_steps
        → rev.dispatch_tasks
             ├── rev.dispatch_task_required_extractions  (AST-derived at creation time)
             ├── rev.dispatch_task_waits                 (runtime wait state, generic wait_kind)
             └── rev.dispatch_task_attempts              (existing, unchanged)

  flow::dispatch_runner (thin coordinator)
    ↓
  flow::scheduler (ready/waiting queue, wake path via LISTEN/NOTIFY)
    ↓
  flow::executor (nushell runtime with custom commands)
    ↓
  task script runs; calls asset text/search/status/chunks commands
```

When a task calls `asset text $id`, one of two things happens:
- **AST-resolved path**: the scheduler already ensured the extraction is ready. The command is a pure fetch against `rev.asset_extractions` + chunks.
- **Fallback path**: the asset_id wasn't determinable at parse time, so the command polls `rev.asset_extractions` until ready or timeout. Same polling primitive as the inline `--wait` flag.

### Extraction pipeline

```
  POST /api/assets/ finalize
    ↓
  enqueue `extract_pdf` or `extract_docx` in rev.background_jobs
    ↓
  flow::extractors worker claims job (SELECT FOR UPDATE SKIP LOCKED)
    ↓
  run extractor (pdf_oxide, officemd_docx)
    ↓
  write rev.asset_extractions row (status=ok)
    write chunks to rev.asset_extraction_chunks
    write positions to rev.asset_extraction_positions
    NOTIFY wait_resolved (wakes scheduler)
    ↓
  enqueue `embed_chunks` in rev.background_jobs (chained)
    ↓
  embedding worker claims job
    ↓
  chunk text into ~500-token windows with 100-token overlap
    call conduit /embed endpoint for each chunk
    write rev.asset_content_embeddings rows (with HNSW index)
    ↓
  asset is now fully available for both `asset text` and `asset search`
```

### Conduit prompt rendering

```
  flow task calls `prompt send $profile_name $input`
    ↓
  flow's prompt_send command POSTs to conduit /prompt with {profile_name, input, dispatch_id, task_id}
    ↓
  conduit resolves the inference profile (system prompt, model config)
    ↓
  conduit GETs /api/dispatches/{dispatch_id}/task_context/{task_id} from api
    receives { bindings, assets, dispatch_position, brief_context }
    ↓
  conduit builds the system prompt in 5 sections:
    1. shared conceptual knowledge (from _shared/dapple/agent-model chapters, selected via similarity to profile system prompt, cached per profile)
    2. dynamic runtime state (generated from the task_context payload)
    3. unified retrieval content (top-N from rev.guide_content_embeddings + rev.asset_content_embeddings via similarity on user input)
    4. task-specific system prompt (from the inference profile itself)
    5. user prompt (the actual task input)
    ↓
  conduit calls the LLM with the assembled prompt, returns response to flow
```

## Work breakdown

### Phase A — Schema and data layer (week 1)

- [ ] **A.1** Migration: `rev.asset_extractions` table + indexes
- [ ] **A.2** Migration: `rev.asset_extraction_chunks` associative table
- [ ] **A.3** Migration: `rev.asset_extraction_positions` table (JSONB `source_position` column)
- [ ] **A.4** Migration: `rev.asset_content_embeddings` table + HNSW index
- [ ] **A.5** Migration: `rev.background_jobs` table + `job_kind` discriminator + claim indexes
- [ ] **A.6** Migration: `rev.dispatch_task_waits` + `rev.dispatch_task_required_extractions` tables
- [ ] **A.7** Migration: ALTER `rev.dispatch_tasks` to add `scheduler_state` column
- [ ] **A.8** `.sqlc` files + `cargo sqlc compose` + `cargo sqlx prepare` for all new queries
- [ ] **A.9** `dapple-data` query wrappers for each new table
- [ ] **A.10** DTOs in `dapple-types::model` for the new tables

### Phase B — Extraction workers (weeks 2-3)

- [ ] **B.1** `flow::extractors` module scaffold + `Extractor` trait
- [ ] **B.2** Job queue claim/retry helpers (`SELECT FOR UPDATE SKIP LOCKED`, attempt counting, dead-letter)
- [ ] **B.3** PDF extractor (refactor current `pdf_extract.rs` to the new model)
- [ ] **B.4** docx extractor using `officemd_docx = "0.1.6"`
- [ ] **B.5** Page-map recording per extractor (PDF → `{page: i32}`, docx → `{paragraph: i32}`)
- [ ] **B.6** Chained embedding job handler
- [ ] **B.7** Conduit `/embed` client wrapper (reuse existing)
- [ ] **B.8** Semantic chunking (fixed 500-token window, 100-token overlap)
- [ ] **B.9** `LISTEN/NOTIFY` wake signal on extraction completion
- [ ] **B.10** Admin CLI command for bulk backfill

### Phase C — Scheduler refactor (weeks 3-5)

- [ ] **C.1** Extract `flow::executor` from `dispatch_runner.rs` — pure task execution, no orchestration
- [ ] **C.2** Extract `flow::data_services` from `dispatch_runner.rs` — binding seeding, brief resolution, journey context
- [ ] **C.3** Extract `flow::audit` from `dispatch_runner.rs` — task_attempt_events wrapper
- [ ] **C.4** New `flow::scheduler` module — generic wait framework, ready/waiting queues
- [ ] **C.5** Wait resolver registry with `ExtractionResolver` implementation
- [ ] **C.6** AST scanner for task declaration at dispatch creation time
- [ ] **C.7** `PgListener` wake path, dedicated DB connection
- [ ] **C.8** Deadlock detection + per-task wait timeout
- [ ] **C.9** `flow::dispatch_runner` becomes thin coordinator
- [ ] **C.10** Migration plan: existing in-flight dispatches run through the new scheduler without state changes

### Phase D — Task-facing commands (week 5)

- [ ] **D.1** `asset text $id` custom command (+ `--wait` flag for fallback polling)
- [ ] **D.2** `asset search $id $query` custom command
- [ ] **D.3** `asset status $id` custom command
- [ ] **D.4** `asset chunks $id` streaming custom command
- [ ] **D.5** Remove `pdf fetch`/`pdf extract` commands
- [ ] **D.6** Migrate existing guides that used the removed commands

### Phase E — API task-context endpoint (week 6)

- [ ] **E.1** New handler `GET /api/dispatches/{id}/task_context/{task_id}`
- [ ] **E.2** Task context DTO: `{bindings, assets, dispatch_position, brief_context}`
- [ ] **E.3** Binding summary generator (slot names, types, truncated values)
- [ ] **E.4** Asset extraction availability generator (which extractions exist, their status, retrieval commands)
- [ ] **E.5** Dispatch lifecycle generator (current stage/step/task, prior task outcomes)
- [ ] **E.6** Brief unit context generator (recent brief, its units, submitted values)
- [ ] **E.7** Route registration, schema, tests

### Phase F — Conduit prompt rendering (week 7)

- [ ] **F.1** API client for task_context endpoint in conduit
- [ ] **F.2** Prompt rendering pipeline refactor into 5-section model
- [ ] **F.3** Shared agent-model chapter loader (fetches from ingested guide)
- [ ] **F.4** Overlay inference via embedding similarity (cached per profile)
- [ ] **F.5** Unified retrieval over `rev.guide_content_embeddings` + `rev.asset_content_embeddings`
- [ ] **F.6** Retirement of the standalone RAG injection path, absorbed into unified retrieval
- [ ] **F.7** System prompt assembly + tests with representative profiles

### Phase G — Agent-model guide authoring (week 8)

- [ ] **G.1** Create `dapple-guides/_shared/dapple/agent-model/` with `guide.toml` + SUMMARY.md
- [ ] **G.2** Chapter: `binding-namespace.md` — explains `$in.bindings.*`, types, access patterns
- [ ] **G.3** Chapter: `asset-surface.md` — explains `asset text`, `asset search`, `asset status`, `asset chunks`
- [ ] **G.4** Chapter: `dispatch-lifecycle.md` — explains stages, steps, tasks, prior-task context, intent
- [ ] **G.5** Chapter: `brief-structure.md` — explains markdown/notice/input/output unit semantics
- [ ] **G.6** Ingest the guide via the normal shared-content pipeline
- [ ] **G.7** Verify embedding-similarity overlay selection picks sensible chapters for representative inference profiles

### Phase H — Integration, testing, migration (weeks 8-10)

- [ ] **H.1** End-to-end smoke test: upload PDF → extraction completes → task reads `asset text` → LLM call with injected context → response
- [ ] **H.2** Scheduler correctness tests: ready/waiting transitions, wake propagation, deadlock detection, timeout paths
- [ ] **H.3** Extraction retry test: simulate worker crash mid-extraction, verify cleanup
- [ ] **H.4** Agent-knowledge injection test: verify the chapters and dynamic state appear correctly in the assembled system prompt
- [ ] **H.5** Migration of existing guides away from `pdf fetch`/`pdf extract`
- [ ] **H.6** Deployment: build, sqlx prepare, deploy api + flow + conduit
- [ ] **H.7** Documentation in `AGENTS.md` files: flow modules, scheduler, extraction pipeline, agent-model guide
- [ ] **H.8** Final plan-to-ship checklist

## Files to create / modify

### dapple-prompt

**New migrations:**
- `crates/dapple-data/migrations/{NNN}_asset_extractions.sql`
- `crates/dapple-data/migrations/{NNN}_background_jobs.sql`
- `crates/dapple-data/migrations/{NNN}_scheduler_waits.sql`
- `crates/dapple-data/migrations/{NNN}_asset_content_embeddings.sql`

**New sqlc + sql files under `crates/dapple-data/assets/sqlc/`:**
- `asset_extractions/` — insert, select by asset, latest version, etc.
- `asset_extraction_chunks/` — insert, select by extraction
- `asset_extraction_positions/` — insert, lookup by chunk
- `asset_content_embeddings/` — insert, similarity search
- `background_jobs/` — claim, mark complete, mark failed
- `dispatch_task_waits/` — insert, resolve, query waiting
- `dispatch_task_required_extractions/` — insert, resolve on task start

**New model files under `crates/dapple-data/src/model/`:**
- `asset_extractions.rs`
- `asset_content_embeddings.rs`
- `background_jobs.rs`

**New query files under `crates/dapple-data/src/query/`:**
- `asset_extractions.rs`
- `asset_content_embeddings.rs`
- `background_jobs.rs`
- `dispatch_task_waits.rs`

**New DTOs in `crates/dapple-types/src/model/`:**
- `asset_extractions.rs`
- `asset_content_embeddings.rs`
- `background_jobs.rs`
- New structs in `dispatches.rs` for scheduler-visible state

**dapple-prompt-api:**
- `crates/dapple-prompt-api/src/handler/dispatches.rs` — new handler for `GET /api/dispatches/{id}/task_context/{task_id}`
- `crates/dapple-prompt-api/src/schema/dispatches.rs` — `TaskContextResponse` DTO
- `crates/dapple-prompt-api/src/route/dispatches.rs` — route registration
- Possibly: `crates/dapple-prompt-api/src/handler/assets.rs` — trigger `extract_pdf` / `extract_docx` job on finalize (or keep in assets handler; decide during implementation)

**dapple-prompt-flow — major refactor:**
- `crates/dapple-prompt-flow/src/dispatch_runner.rs` — shrinks to ~200-400 lines of coordinator
- `crates/dapple-prompt-flow/src/executor.rs` (new) — extracted task execution
- `crates/dapple-prompt-flow/src/scheduler/mod.rs` (new) — ready/waiting queues, wait framework
- `crates/dapple-prompt-flow/src/scheduler/ast_scan.rs` (new) — task declaration derivation
- `crates/dapple-prompt-flow/src/scheduler/resolvers/mod.rs` (new) — resolver registry
- `crates/dapple-prompt-flow/src/scheduler/resolvers/extraction.rs` (new) — extraction wait resolver
- `crates/dapple-prompt-flow/src/scheduler/wake.rs` (new) — `PgListener` wake loop
- `crates/dapple-prompt-flow/src/extractors/mod.rs` (new) — worker pool + registry
- `crates/dapple-prompt-flow/src/extractors/pdf.rs` (new) — PDF extractor (absorbs current `pdf_extract.rs`)
- `crates/dapple-prompt-flow/src/extractors/docx.rs` (new) — docx extractor
- `crates/dapple-prompt-flow/src/extractors/embedding.rs` (new) — chained embedding job handler
- `crates/dapple-prompt-flow/src/data_services/mod.rs` (new) — binding, brief, journey-context helpers (relocated from dispatch_runner.rs)
- `crates/dapple-prompt-flow/src/audit.rs` (new) — task_attempt_events wrapper (relocated)
- `crates/dapple-prompt-flow/src/commands/asset.rs` (new) — `asset text`/`search`/`status`/`chunks` commands
- `crates/dapple-prompt-flow/src/commands/mod.rs` — remove `pdf_fetch`/`pdf_extract`, register new `asset` commands
- `crates/dapple-prompt-flow/Cargo.toml` — add `officemd_docx = "0.1.6"`

**dapple-prompt-conduit:**
- New module for the task_context API client
- Prompt rendering pipeline refactor for the 5-section model
- Shared agent-model chapter loader
- Overlay inference via embedding similarity + per-profile cache
- Unified retrieval over both embedding tables
- Retirement of the standalone RAG injection path

### dapple-guides

- `internal/dapple/_shared/dapple/agent-model/guide.toml`
- `internal/dapple/_shared/dapple/agent-model/SUMMARY.md`
- `internal/dapple/_shared/dapple/agent-model/binding-namespace.md`
- `internal/dapple/_shared/dapple/agent-model/asset-surface.md`
- `internal/dapple/_shared/dapple/agent-model/dispatch-lifecycle.md`
- `internal/dapple/_shared/dapple/agent-model/brief-structure.md`
- `internal/dapple/_shared/dapple/agent-model/locales/en.ftl` (if needed for Fluent messages)
- Migration of any existing guides using `pdf fetch`/`pdf extract` to the new `asset *` commands

## Known non-goals / explicit deferrals

- **HITL waits, LLM callback waits, sub-dispatch waits** — the generic wait framework supports adding these, but the only resolver implemented in this plan is `extraction`. Adding the others is follow-up work, not part of this plan.
- **dapple-shell microservice** — parked in `docs/musings/dapple-shell-datasets.md`. Decomposition in this plan sets up the ground for it later but does not commit to it.
- **Dataset pipeline (xlsx, csv, tabular json)** — parked in `docs/musings/dapple-shell-datasets.md`. This plan covers only content assets (PDF, docx). Dataset handling is a separate future plan.
- **Fluent labels for brief unit inputs / help / placeholders** — previous plan's non-goal, still unchanged.
- **Media brief units (`rev.brief_unit_media`)** — schema supports it, not implemented in the runner. Still out of scope.
- **Asset upload UI in Studio** — the previous plan included this as Phase 9 (`BriefRenderer`'s asset slot branch). Already shipped via the base64 `POST /api/assets/` path. This plan doesn't change it.
- **Dispatch state resume after api/flow restart** — the scheduler is in-process; restart-resume of waiting tasks is a follow-up concern. Dispatches in flight when flow restarts will need to be recovered via a startup pass that promotes `running` rows back to `ready` (similar to the extraction job janitor pattern).
- **Multi-instance flow deployment** — LISTEN/NOTIFY is multi-instance-ready, but the scheduler's internal state isn't yet. Deploying multiple flow instances against the same DB requires careful thought about which instance is authoritative for which dispatches. Not a concern for single-node dev, but flag for production.
- **Upload progress in Studio** — out of scope, same as earlier.
- **Content sniffing for MIME detection** — MIME remains filename-extension-driven.

## Verification

### Unit / module level

- `flow::scheduler` — state machine tests (ready→waiting→ready, wait resolution, timeout, deadlock)
- `flow::extractors::pdf` — round-trip test with a known sample PDF, verify text + positions
- `flow::extractors::docx` — same for docx via officemd_docx
- `flow::extractors::embedding` — chunking test (verify overlap, token counts)
- Conduit prompt assembly — render test for representative profiles, verify 5-section output
- Overlay inference — embedding similarity test, verify threshold behavior

### Integration

- End-to-end smoke: upload PDF → extraction + embedding complete → task script calls `asset text` → LLM call with injected context → verify response references the PDF content
- Admin backfill: bump pdf extractor version, run backfill, verify all assets re-extracted
- LISTEN/NOTIFY wake: simulate extraction completion, verify scheduler resumes a waiting task within 1 second
- Deadlock detection: create a task with a wait on a dead extraction, verify it's failed with a clear error after deadlock detection runs

### Manual

- Ingest the agent-model guide, run the overlay-inference pass for a new inference profile, inspect the selected chapters
- Curl `GET /api/dispatches/{id}/task_context/{task_id}` for a live dispatch, verify the payload shape
- Deploy, walk a guide that uses both PDF and docx assets end-to-end in Studio

## Open questions for implementation

These are deliberately deferred to implementation time:

1. **Embedding similarity threshold** for overlay inference — needs empirical tuning. Start at 0.5 cosine similarity, adjust after measuring.
2. **Chunking strategy for long inputs to the embedding service** — 500-token windows may not match ModernBERT's max sequence length cleanly. Verify at implementation and adjust.
3. **Task-wait timeout default** — 60s for short extractions, configurable via `rev.dispatch_task_waits.timeout_at`. Actual default tbd.
4. **Scheduler tick interval** — LISTEN/NOTIFY gives event wakes, but a periodic safety poll is still prudent. 1s or 5s; decide after measuring wake latency.
5. **Background job retry limits** — start with `max_attempts = 3`, exponential backoff `2^attempts minutes`. Tune after observing real failures.
6. **Position schema per extractor kind** — PDF uses `{page: i32}`, docx uses `{paragraph: i32}`. Canonical shapes to be finalized at extractor implementation.
7. **Concurrency of the extractor worker pool** — start at 4, make configurable, adjust by measuring CPU and DB contention.
8. **Conduit's new DB connection for `PgListener`** — conduit doesn't need LISTEN/NOTIFY itself (scheduler is in flow); leave conduit's connection pool alone. Only flow adds the listener connection.
9. **officemd_docx edge cases** — verify tables, nested lists, embedded content, track changes. Fall back to `dotext` or manual XML parse if it falls short.
