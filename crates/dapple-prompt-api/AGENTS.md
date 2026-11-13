# Agent Instructions — dapple-prompt-api

Quick reference for agents working with the REST API layer. See [README.md](./README.md) for full patterns.

> **See also:**
> - [`docs/books/api-consumer-guide/`](../../docs/books/api-consumer-guide/) — consumer-facing REST/SDK documentation
> - [`docs/books/journey-planner-guide/`](../../docs/books/journey-planner-guide/) — guide-authoring (ingest pipeline, directives, Fluent localization)
> - [`docs/books/journey-builder-guide/`](../../docs/books/journey-builder-guide/) — internal API/handler architecture
> - [`crates/dapple-prompt-external-api/AGENTS.md`](../dapple-prompt-external-api/AGENTS.md) — customer-facing API surface; do NOT cross-import handlers between the two crates

## Single-Tenant Posture

`dapple-prompt-api` is the **internal** surface — fronted by `api.dapple.x` (cluster-internal, not exposed to the public). Internal handlers do **not** filter queries by company / namespace / external_org. The internal API operates on the dapple namespace implicitly.

**Customer-facing (third-party) API access goes through `dapple-prompt-external-api`** — a separate Axum router served on `external-api.dapple.x` from the same workspace, with its own extractor (`ExternalCaller`), its own service-token table (`rev.external_service_tokens`), and its own JWT signing key.

**Anti-patterns in this crate:**

- ❌ Adding `WHERE namespace_company_id = …` filters to internal handlers. Internal queries are unscoped by design.
- ❌ Reading `caller.company_id` or similar from `RegisteredUser` to gate visibility. The internal API doesn't carry that information.
- ❌ Routing customer requests through internal handlers "just for now." Cross-surface boundary violations are an authorization-boundary mistake; they belong in `dapple-prompt-external-api`.

When in doubt: if a request is from a customer (Acme et al), it must arrive via `external-api.dapple.x` and be served by `dapple-prompt-external-api`, not by anything in this crate.

## Quick Checklist: Adding a New Endpoint

1. [ ] Schema in `src/schema/{resource}.rs`:
   - `Create{Resource}Schema` — single object POST body
   - `Create{Resource}SetSchema` — batch wrapper `{ objs: Vec<...> }`
   - `Get{Resource}SetSchema` — enum with **required query variants** (see below)
2. [ ] Handler in `src/handler/{resource}.rs`
3. [ ] Routes in `src/route/{resource}.rs`
4. [ ] Register in `src/lib.rs` router
5. [ ] Consumer test in `tests/consumer/{resource}.rs`
6. [ ] Trial in `tests/user_lifecycle.rs`

## Required GET Query Variants

Every `Get{Resource}SetSchema` enum **must** include the standard query variants appropriate for its entity type. All list endpoints use `index_response_handler()` for consistent response formatting.

### Standard variants by entity classification:

| Entity Type | Required Variants | Examples |
|-------------|-------------------|----------|
| **System config** (reusable definitions) | `ByIds`, `All` | InferenceProfiles, Directives, ValueSlots, FluentMessages |
| **System config with slugs** | `ByIds`, `All`, `BySlugs` | Tasks, Steps, Stages, Categories, Hats, Playbooks |
| **Versioned with paths** | `ByIds`, `ByPathIds`, `All` | Prompts, FluentMessages, Directives, ValueSlots |
| **Content objects** (accessed via relationships) | `ByIds`, `ByPathIds` | Briefs, BriefGroups, BriefPacks, Assets |
| **User-scoped** (never enumerate all) | `ByIds`, `ByUser`/`ByPlaybook` | Journeys, Dispatches |
| **Child entities** (queried by parent) | `ByParentIds` only | DispatchIntentBindings |

### Implementation pattern:

Each query variant requires:
1. **SQL file** — `select_{resources}.sql` (for `All`), `select_{resources}_by_{variant}.sql`
2. **Query function** — `select_{resources}(pool, filter_ops)` in `dapple-data/src/query/`
3. **Handler match arm** — Wire the variant in the index handler
4. **`FilterOptions` on `All`** — Always include `options: Option<FilterOptions>` and pass through to the SQL `LIMIT $N OFFSET $M`

### Anti-patterns:
- ❌ Stubbing query variants with empty arrays (e.g., returning `{ objs: [] }` for `ByIds`)
- ❌ Adding a schema variant without the corresponding SQL + query function
- ❌ Using manual `json!()` response construction instead of `index_response_handler()`
- ❌ Omitting `All` on system config entities — these need enumeration for admin/config UIs

## Journey-Centric Reminders

**Users don't see internal objects.** They experience:
- **Playbook** → Workflow they pick
- **Journey** → Their session
- **Stage** → A form / pane they advance between
- **Step** → A sub-interaction *on* a pane (generate → pick → refine)
- **Debrief** → Interview experience (not "Brief")
- **Dispatch** → Background execution

**Stages are panes; steps are the interactions on them.** A stage is a static
form / pane; its steps are the dynamic, AJAXy sub-interactions on that pane
before the user navigates to the next stage. Multi-step stages are the norm. The
`/journeys/{id}/steps` endpoint is the fine-grained "which sub-interaction is
awaiting input" signal; `/journeys/{id}/stages` is the per-pane roll-up. Both are
served from the same
`select_*_binding_status_for_journey` queries and are consumed by dapple-ui and
the dapple-suite runner (internal) exactly as the external surface serves them —
this is shared-core, not surface-specific.

## Journey ↔ Guide pinning (handler responsibilities)

Every journey is version-pinned to a specific `rev.guides.id`. The caller supplies
that pin explicitly — `POST /api/journeys` does **not** resolve "latest" or by path.
By the time a journey is fired off, the consumer already holds the exact guide
`object_id` it navigated to (guide browse is canonical-pin-aware), so the handler's
job is to **validate**, not guess:

1. `guide_id` and `playbook_id` are BOTH required (specific version object_ids).
2. For each create-schema, validate the pair is internally consistent — the guide
   exposes that playbook version — via
   `dapple_data::query::guides::guide_exposes_playbook(guide_id, playbook_id)`. On
   mismatch return 400 `GUIDE_PLAYBOOK_MISMATCH`. **A matching-but-non-canonical pair
   is normal and allowed** (experiments / a consumer deliberately on an older
   version); canonical-pin selection is a browse concern, never re-derived here.
3. Chained journeys additionally require `guide_id == predecessor.guide_id` (the
   Migration 44 trigger enforces this regardless of handler logic).
4. The external surface ALSO namespace-checks `guide_id` against `caller.company_id`
   (`guide_in_namespace`) — a foreign/unknown guide returns the same 404 (no leak).
5. **Stage-level forks (Migration 74):** an optional `fork_stage_id` requires
   `predecessor_journey_id` and must be a stage of the *predecessor's* playbook
   (validate via `playbook_stage_in_playbook(ancestor.playbook_id, fork_stage_id)`;
   the fork-coherence trigger backs it at the DB). Semantics: the chain walk
   crossing that edge admits only predecessor-owned bindings whose producing
   dispatch stage has `ord <` the fork stage's ("fork AT stage N" = redo N —
   inherit strictly before). The filter is **edge-local**: bindings the
   predecessor itself inherited from deeper ancestors flow through untouched.
   Stage *completion* is never consulted — forking is provenance-scoped, not
   completion-gated — and the DONE rollups then report pre-fork stages done /
   fork-stage-onward clean with no extra machinery. The shared filter lives in
   `assets/sqlc/shared/fork_cutoff_filter.sqlc` (composed into every
   chain-aware binding reader alongside `shared/journey_chain.sqlc`).

There is NO auto-pin. `select_latest_guide_id_for_playbook` was deleted; resolving a
journey's version from "latest non-deprecated" let a brand-new journey adopt a
version the consumer didn't choose.

**Brief-pack auto-derive flows through the (now explicit) pin.** When `brief_pack_ids`
is empty, the handler calls `select_brief_pack_ids_for_guide(guide_id)` — NOT
`select_brief_pack_ids_for_playbook` (deleted; it walked through all guide versions
of the playbook and caused duplicate attachments).

**Anti-patterns:**

- ❌ Resolving `guide_id` from the playbook ("latest"/by-path) at journey-create.
  The caller passes the exact guide it already holds; the backend only validates.
- ❌ Calling `select_latest_guide_id_for_playbook` or `select_brief_pack_ids_for_playbook`
  (both deleted). Validate with `guide_exposes_playbook`; derive packs with
  `select_brief_pack_ids_for_guide`.
- ❌ Stamping `guide_id = None` / NULL into a new journey.
- ❌ Re-resolving a journey's brief_packs from the playbook downstream of
  creation. Once pinned, `journey.guide_id` is the source of truth.

See `crates/dapple-data/AGENTS.md` ("Journey ↔ Guide version pinning") for the
data-layer rationale, soft-deprecation behavior, and the b-side / experiment
caveat.

## Conduit Context Frames (task_context endpoint)

`GET /api/dispatches/{id}/task_context/{task_id}` (`resolve_task_context` in
`handler/dispatches.rs`) is the conduit's context rail. Beyond bindings /
assets / prior tasks / brief context it now carries:

- **`guide_frame`** — the journey's PINNED guide version's raw `CONDUITS.md`
  (via `select_guide_conduits_by_dispatch_id`; pinned read, never
  deprecation-filtered). The conduit renders it as its own `# Guide Frame`
  system message. `None` for unpinned journeys / pre-frame guide versions.
- **Position fields** on `dispatch_position` — journey-pinned stage/step
  names + fluent-resolved descriptions + `stage_ord`/`total_stages`
  (`select_task_position_for_dispatch_task`, scoped through the journey's
  playbook so shared step versions can't fan out).
- **Overlay blinders on `bindings`** — if the task's stage→step→task cascade
  carries ANY overlays (`select_overlay_cascade_for_task`, which keeps masked
  rows precisely so "no overlays" ≠ "zero exposes"), the binding rows are
  filtered to the overlay-exposed subset BEFORE the `## Bindings` list, the
  asset-extraction list, and `resolve_task_brief_context` are built. Zero
  overlays on the cascade → unfiltered (blinders are opt-in per node).

**Invariants:**
- ❌ Never scope the flow engine's `$in.bindings` seeding by overlays — the
  blinder is the conduit-facing render boundary ONLY (see dapple-prompt-flow
  AGENTS.md "Binding read availability").
- The blinder fails OPEN (warn + unfiltered) on cascade-query errors: it is a
  signal-quality measure, not a security boundary — the task script's own
  `prompt send` pipeline is author-controlled anyway.
- `conduits_md` materialization is symmetric by construction: both POST and
  PATCH arms build args through `build_guide_insert_args_with_fluent`. Keep it
  that way — do not add a second construction site.
- Schema-driven `POST /api/guides` (no tarball) stores `conduits_md: None`;
  only tarball ingest populates the frame.

## Guide System

The guide system allows authors to write structured content that the platform parses, transforms, and materializes into database entities. Guides are **locale-independent** — all localizable text lives in Fluent `.ftl` files, not in the guide structure itself.

**Pipeline:** Author writes guide → `dapple-parser` parses to `ParsedGuide` → `plan_guide()` transforms to `IngestPlan` → `dapple-prompt-api` materializes to database

### Inline vs Fluent-Backed Blocks

| Directive | Content Source | Rationale |
|-----------|---------------|-----------|
| `prose` | Fluent-backed | Localizable narrative text |
| `stage` | Fluent-backed | Stage description varies by locale |
| `step` | Fluent-backed | Step description varies by locale |
| `brief_pack` | Fluent-backed | Pack content and card metadata vary by locale |
| `inference_profile` | Fluent-backed | System prompts and descriptions vary by locale |
| `block` | Fluent-backed | Generic block with localizable content |
| `nushell` | Inline (`content = Some(...)`) | Code is not localized |
| `ref` | Inline (`content = Some(...)`) | Structural cross-reference, not localized |

In the `IngestPlan`, Fluent-backed blocks have `content: None` (unless the markdown contains non-empty fallback text). Inline blocks always retain their `content: Some(...)`.

### Display titles/descriptions are Fluent-**referenced**, not baked

Brief/group/pack titles and stage/step descriptions resolve live from a Fluent
message body via `*_fluent_message_id` — the read returns
`COALESCE(rev.fluent_body(ref, 'en'), text, name)`. The materializer
(`materialize_brief_system`, `materialize_single_playbook`) MUST:

1. **Set `*_fluent_message_id` from `message_id_map.get(name)` in BOTH the INSERT
   and PATCH arms** (and leave the `*_text` column `None`). The two arms must be
   symmetric — the original slug-rot bug was a PATCH arm that baked
   `title = name`. The brief INPUT/NOTICE units (`label_fluent_message_id`,
   `title_fluent_message_id`) are the reference precedent.
2. **Fold the display Fluent message's `content_hash` into the entity's content
   hash** so a Fluent-body edit re-versions the entity and re-pins the new
   version (append-only Merkle). `materialize_fluent_messages` returns a
   `(message_id_map, message_hash_map)` pair for exactly this; pass the
   `content_hash` where the `name` placeholder used to go in
   `hash_brief`/`hash_brief_group`/`hash_brief_pack`/`hash_stage`/`hash_step`.

**Anti-patterns:** baking `title = name` / `description = name` in either arm;
asymmetric INSERT-vs-PATCH title logic; adding a `*_fluent_message_id` reference
without also folding the Fluent `content_hash` into the parent hash (the pin
would never re-pin on edits). Descriptions sourced from a Fluent `.description`
**attribute** (brief/group/pack) are baked consistently in both arms because
attributes are not persisted as bodies — that is the one allowed bake, and it
must be identical in INSERT and PATCH. `value_slots`/`tasks`/`playbooks` keep
`name` as an intentional placeholder (no Fluent display source). See
`crates/dapple-data/AGENTS.md` → "Versioned object references — pin exact versions".

### Shared Content Library (deferred to V2+)

The `_shared/` content reference system was removed as part of Migration 33's
path-layer cleanup — `object_paths.ref_key` is gone and Migrations 33/35
dropped `rev.guide_shared_refs` and `rev.shared_content_objects`. The handler,
route, schema, parser pipeline, and SDK client for `POST /api/shared-content/ingest`
are no longer present.

When shared content returns as V2+ functionality it will get a proper
`rev.shared_content_path_details` table mirroring the other 16 path-detail
tables, plus a fresh ingest pipeline. The `:::ref` directive remains parsed
(it appears in `KNOWN_DIRECTIVES`) but currently has no materialization
target — `dapple-parser`'s `extract_derived_from_block` silently skips
blocks carrying `ref_path`.

See `crates/dapple-data/AGENTS.md` (Multi-Tenancy Primitives) for the V1 schema state and the V2+ deferral.

## Guide Search Endpoint

Natural language search over guide content using pgvector similarity.

**Endpoint:** `GET /api/guides/search`

### Query Parameters (`GuideSearchParams`)

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `q` | String | (required) | Natural language search query |
| `tags` | `Vec<String>` | `["universal"]` | Audience tag filter |
| `locale` | String | `"en"` | Locale filter |
| `limit` | i32 | 20 | Results limit, clamped 1..100 |
| `guide_ids` | `Vec<Uuid>` | (all) | Scope to specific guides |
| `threshold` | f64 | 0.0 | Minimum similarity score |

### Flow

1. Validate `q` is non-empty (400 if empty)
2. Resolve parameters with defaults → `ResolvedSearchParams`
3. Get `DAPPLE_PROMPT_CONDUIT_URL` from AppState (503 if not configured)
4. Call `EmbeddingClient::embed()` to get query vector
5. Query pgvector: `search_guide_content_embeddings()` or `search_guide_content_embeddings_by_guide_ids()`
6. Filter results by similarity threshold
7. Convert to `GuideSearchResult` DTOs
8. Return via `index_response_handler()`

### Error States

| Status | Cause |
|--------|-------|
| 400 | Empty `q` parameter |
| 503 | `DAPPLE_PROMPT_CONDUIT_URL` not configured or embedding service unavailable |
| 500 | Embedding or search failure |

### Files

| File | Purpose |
|------|---------|
| `src/schema/guide_search.rs` | `GuideSearchParams`, `ResolvedSearchParams` |
| `src/handler/guide_search.rs` | Search handler |
| `src/route/guide_search.rs` | Route registration at `/api/guides/search` |

## Embedding Generation Pipeline

Embeddings are generated **synchronously, inside the ingest request**, and a
failure **fails the ingest**. This is deliberate: a guide whose blocks aren't
embedded has no RAG index, so it does not run properly. Returning a green
ingest for a guide that can't work is the worse outcome — better a loud failure
the author retries.

> **This section previously documented the opposite** (`tokio::spawn`,
> "non-blocking", "never fail the ingest"). That was wrong, and it was wrong in
> the exact place someone debugging a hung ingest would look. There is no
> `tokio::spawn` in `guide_ingest.rs`.

### Trigger

`generate_embeddings_for_guide_sync()` is awaited at three call sites in
`handler/guide_ingest.rs` — the POST-ingest, new-version, and refresh arms.
The fire-and-forget `generate_embeddings_for_guide()` wrapper is
`#[allow(dead_code)]` and has no live caller; don't "restore" it to one of
these call sites without reading the blocking rationale above.

On the refresh arm, `needs_embeddings` is effectively always true:
`derived_objects_repaired` is set whenever the materializers run, and a guide
with zero embeddings trips the count check anyway.

### Pipeline Steps

1. Check `DAPPLE_PROMPT_CONDUIT_URL` is configured (absent ⇒ skip, `Ok(())`)
2. Load guide with full chapter/block hierarchy
3. Collect `fluent_message_id`s from blocks
4. Batch-resolve Fluent bodies (prefer `"en"` locale)
5. Resolve lookup tables (directive types, tags, locales)
6. Filter blocks by `EMBEDDABLE_DIRECTIVE_TYPES`: `["prose", "stage", "step", "block", "ref"]`
7. Call Conduit `POST /embed` **once per `EMBED_BATCH_SIZE` (8) chunk**,
   sequentially, accumulating in order (see "Batching" below)
8. Create a new embedding generation row — **append-only, no DELETE**
9. Insert new embeddings + tag associations + locale associations

### Batching (`EMBED_BATCH_SIZE`)

ModernBERT pads a batch to its **longest** member, so one long block sets the
sequence width for every text batched with it. Sending a whole guide in one
call made peak memory scale as `block_count * longest_block_tokens` and
**OOM-killed conduit-onnx** (exit 137 under a 12Gi limit) on `mark-the-bdc`:
162 blocks whose longest is 11,455 chars (~2,900 tokens).

Chunking is a **transport** change, never a semantic one — the backend masks
padding correctly, so a text's vector is bit-identical regardless of its
batch-mates (verified 2026-08-01: cosine `1.0`, max elementwise diff exactly
`0.0`). Total compute is unchanged (~68s either way); only peak memory differs.

Calls are **sequential, not concurrent** — conduit-onnx runs CPU inference
under a 2-core limit, so parallel calls buy no throughput and just multiply the
peak memory the batch size exists to bound.

### Windowing (`EMBED_WINDOW_CHARS`) — semantic, NOT the same thing as batching

**Do not conflate these two.** `EMBED_BATCH_SIZE` above is *transport*: how many
already-decided texts share one HTTP call, explicitly semantics-preserving
(bit-identical vectors). **Windowing is *semantic*: one guide block becomes
several embedding ROWS.** The note that "chunking is a transport change only" is
about batching and must not be read as forbidding windowing.

conduit-onnx caps input at `MAX_SEQUENCE_TOKENS` (4,096) and **truncates**
beyond it, so a long block's tail was never retrievable — silent index loss for
a platform premised on agent prose being RAG-reachable.
`service::text_window::window_text` splits such a block into overlapping
windows (12,000 chars, 1,500 overlap), and `extract_embeddable_blocks` emits one
`EmbeddableBlock` **per window**.

- **Threshold-triggered.** A block at or under `EMBED_WINDOW_CHARS` yields
  exactly one window, byte-identical to today. Only blocks that are currently
  broken change.
- **Fan out at extraction, nowhere else.** Everything downstream maps
  positionally over `blocks_to_embed` — texts, metadata, the column-wise insert
  args, and the `inserted[i]` zips that attach tags and locales. Widening that
  vec before any of it keeps every correspondence valid. A guard before the tag
  loop asserts `inserted[i].block_id == blocks_to_embed[i].block_id`, because a
  silent reorder would mis-attach tags and the affected rows would then vanish
  from every search (tags and locales are INNER JOINs) with no error anywhere.
- **`position` counts blocks, not windows** — all windows of a block share it.
- **No migration.** `guide_content_embeddings_block_idx` is a plain btree, not
  UNIQUE; `chunk_text` is already the column name; the tag/locale junctions
  cascade off the embedding `id`. Verified against the live DB by
  `test_multi_window_embeddings_are_independently_retrievable`.
- `metadata.chunk_ordinal` / `chunk_count` identify the window. Search DTOs drop
  the embedding row `id` and expose only `block_id`, but pass `metadata`
  through — so this is what lets a client tell two windows of one block apart.

### Truncation is a HARD ingest failure (`truncated_count`)

Windows are sized in **chars** against a **token** cap via an estimate
(~4 chars/token; measured 4.86 on English guide prose), so pathological content
— dense tables, CJK — could still overshoot. Rather than trust the estimate,
`POST /embed` reports `truncated_count` and
`generate_embeddings_inner` **returns `Err` before the generation row is created**
if it is non-zero. The guide keeps its previous complete generation instead of
gaining a lossy one.

The field rides the whole chain (conduit-onnx `EmbedBatch` → its
`EmbedResponse` → `OnnxInferenceClient` → conduit's `EmbedResponse` →
`EmbeddingClient::embed`), `#[serde(default)]` at each parse so no lockstep
deploy is needed.

**Scoped to ingest.** RAG and guide-search embed *queries*, which clamp on
purpose and persist nothing — those paths ignore the count deliberately, and
each says so at the call site. Failing them would be wrong.

### LANDMINE — ingress timeout must exceed the whole embed run

The ingest request stays open for the entire embedding pass (~70s for a large
guide). nginx's **default `proxy_read_timeout` is 60s**, which is *below* the
`EmbeddingClient`'s own 120s budget — so the ingress killed the connection
first, cancelling the axum handler mid-pass. The result was pathological:

- the guide content was already **committed** (the tx commits before step 8),
- **no embeddings were written**, so the guide silently had no RAG index,
- the client saw an opaque nginx **504 HTML page**, never the app's error,
- and it **never self-healed** — zero embeddings is itself a `needs_embeddings`
  trigger, so every retry re-entered the same pass and died at the same wall.

`assets/k8s/registry/dapple-prompt-api.yaml` and `dapple-prompt-conduit.yaml`
therefore pin `proxy-read-timeout: "300"`. Keep any ingress fronting ingest
**above** the client's 120s per-batch budget, so a genuine failure surfaces as
the application's JSON error rather than a timeout page.

### EmbeddingClient (`src/service/embedding_client.rs`)

HTTP client to Conduit's `POST /embed`:
- 120s timeout — a **per-batch** budget, not per-guide
- Returns `Ok(None)` on 503 (service unavailable) — graceful degradation;
  the caller bails before any DB write, so a partial index is impossible
- Returns `Ok(Some(embeddings))` on success

### Files

| File | Purpose |
|------|---------|
| `src/handler/guide_embeddings.rs` | `generate_embeddings_for_guide_sync()`, `EMBED_BATCH_SIZE`, block extraction, metadata building |
| `src/service/embedding_client.rs` | `EmbeddingClient` HTTP wrapper (one call = one batch) |

## Classification Filtering

Guide and guide card queries support filtering by classification:

- `GetGuideSetSchema::ByClassification { classification }` — filter guides
- `GetGuideCardSetSchema::ByClassification { classification }` — filter cards

### SQL

| File | Purpose |
|------|---------|
| `select_guides_by_classification.sql` | Filter guides by `guide_path_details.classification` |
| `select_guide_cards_by_classification.sql` | Filter cards by classification |

## Environment Notes

- `DAPPLE_PROMPT_CONDUIT_URL` — Conduit service URL (e.g., `http://dapple-prompt-conduit`). When `None`, guide search and embedding generation are silently disabled.

## Handler Pattern

```rust
pub async fn resource_post_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Create{Resource}SetSchema>,
) -> impl IntoApiResponse {
    // 1. Validate input
    // 2. Build Insert{Resource}Args (UUIDs pass through directly)
    // 3. Call dapple_data::query::{resource}::insert_{resource}()
    // 4. Convert to public DTOs
    post_response_handler("{resource}", Ok(dtos))
}
```
## API Handler Patterns
### Query Parameters

- `dry_run=true` — Parse and transform only; return validation summary without materializing. Returns warnings, block/chapter/asset counts, and derived object counts.
- `source_path=<path>` — Override the guide's `source_path` metadata field.


### Handler Structure
```rust
// File: crates/dapple-prompt-api/src/handler/journeys.rs
use aide::axum::{routing::post, ApiRouter, IntoApiResponse};
use axum::{extract::State, Json};
use dapple_data::{query::journeys, model::journeys::JourneyModel};

pub async fn create_journeys(
    State(pool): State<PgPool>,
    Json(req): Json<CreateJourneySetSchema>,
) -> Result<impl IntoApiResponse, ApiError> {
    // 1. Validate input
    if req.objs.is_empty() {
        return Err(ApiError::BadRequest("Empty batch".into()));
    }

    // 2. Build InsertArgs (UUID FKs pass through directly)
    let args = InsertJourneyArgs {
        names: req.objs.iter().map(|o| o.name.clone()).collect(),
        playbook_ids: req.objs.iter().map(|o| o.playbook_id).collect(),
    };

    // 3. Insert
    let inserted = journeys::insert_journeys(&pool, args).await?;

    // 4. Convert to public DTOs
    let dtos: Vec<Journey> = inserted.into_iter()
        .map(|m| m.into())
        .collect();

    Ok(Json(dtos))
}
```

### Request/Response Schemas
```rust
// POST request schema
#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateJourneySchema {
    pub name: String,
    pub playbook_id: Uuid,
}

// Batch create schema
#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateJourneySetSchema {
    pub objs: Vec<CreateJourneySchema>,
}

// GET query schema with enum variants
#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetJourneySetSchema {
    ByIds { ids: Vec<Uuid>, options: Option<FilterOptions> },
    ByPlaybook { playbook_id: Uuid, options: Option<FilterOptions> },
}
```

### Required GET Query Variants

Every `Get{Resource}SetSchema` **must** include the standard variants for its entity classification. All index handlers must use `index_response_handler()` — never construct JSON responses manually.

| Entity Classification | Required Variants |
|----------------------|-------------------|
| System config (reusable definitions) | `ByIds`, `All` |
| System config with slugs | `ByIds`, `All`, `BySlugs` |
| Versioned with path history | Add `ByPathIds` to above |
| Content objects (via relationships) |
| User-scoped (never enumerate all) |
| Child entities (by parent) |

**For each variant you must provide:**
1. `.sqlc` source in `dapple-data/assets/sqlc/{resource}/` (e.g., `select_{resources}.sqlc` for `All`), then run `cargo sqlc compose` to generate `assets/.sql/`
2. Rust query function in `dapple-data/src/query/{resource}.rs` — reference `"assets/.sql/..."` paths, pass params in alphabetical `:bind()` order
3. Handler match arm wired to the query function
4. `FilterOptions` support (`LIMIT $N OFFSET $M`) on any variant that returns unbounded results

**Anti-patterns:**
- ❌ Stubbing a variant with empty results (`json!({ "objs": [] })`)
- ❌ Defining a schema variant without implementing the backing SQL + query function
- ❌ Omitting `All` on system config entities
- ❌ Using `object_id` in field names -- the PK is now `id: Uuid` directly

### Pipeline Flow

```
multipart → extract_tarball_field()
         → decompress_and_plan()
             → tar::Archive extraction (with symlink/path-traversal security checks)
             → find_guide_root() (locate guide.toml in tarball)
             → dapple_parser::parse_guide() → ParsedGuide
             → dapple_parser::transform::plan_guide() → IngestPlan
         → [if dry_run: return validation summary]
         → materialize_guide() / materialize_guide_version()
             → upload_assets() (draft/chunk/finalize pipeline)
             → insert_guides() / patch_guides() (atomic CTE)
             → create_guide_asset_associations()
             → create_guide_card()
```

### Testing

**Test Environment Setup:**
The development environment is fully configured with:
- PostgreSQL database accessible via `DAPPLE_PROMPT_DATABASE_URL`
- Redis instance for integration testing (optional, uses noop fallback)
- `.env.test` file for test-specific configuration
- Auto-seeded test user (`TEST_USER_SUB`) for auth tests
- All database migrations applied

```bash
# Run all tests (requires DAPPLE_PROMPT_DATABASE_URL)
cargo test --workspace

# Run specific test
cargo test --test user-lifecycle --package dapple-prompt-api

# Run with logging
RUST_LOG=debug cargo test

# Run with strict Redis requirement (fail if Redis unavailable)
DAPPLE_REDIS_STRICT=1 cargo test
```

**Lifecycle Test Structure:**
- Tests located in `crates/dapple-prompt-api/tests/`
- Main integration test: `user_lifecycle.rs`
- Consumer test helpers: `consumer/` directory
- Each endpoint has a consumer module (e.g., `consumer/fluent_messages.rs`)
- Tests use `ScenarioContext` to track and share created objects between trials
- Tests run concurrently using `libtest_mimic` framework


### Constraints

- **50MB maximum tarball size** — Enforced at the multipart extraction step.
- **Atomic transaction** — All database operations (guide + chapters + blocks + tags + assets + card) are performed within a single transaction.
- **Security** — Symlinks and path traversal (`..`) in tarballs are rejected.

### IngestPlan Structure

The `IngestPlan` is the output of the transform layer and the input to materialization:

```
IngestPlan
├── title, description, author, classification, source_path
├── chapters: Vec<ChapterDef>
│   └── blocks: Vec<BlockDef>
│       └── directive_type, name, content (Option), attributes, tags
├── card: Option<CardDef>
├── asset_files: Vec<AssetFileEntry>
├── derived_brief_packs: Vec<BriefPackDef>
├── derived_playbook: Option<PlaybookDef>
│   └── stages: Vec<StageDef>
│       └── steps: Vec<StepDef>
├── derived_tasks: Vec<TaskDef>
├── derived_inference_profiles: Vec<InferenceProfileDef>
├── derived_refs: Vec<RefDef>
├── fluent_messages: HashMap<String, FluentMessageSet>
└── warnings: Vec<String>
```

### Route Registration
```rust
// File: crates/dapple-prompt-api/src/route/journeys.rs
pub fn routes() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route("/journeys", post(create_journeys))
        .api_route("/journeys", get(get_journeys))
}
```

## Integration Testing

### Test Structure
```rust
// File: crates/dapple-prompt-api/tests/lifecycle_journeys.rs
use dapple_prompt_api::schema::journeys::*;

#[sqlx::test]
async fn test_journey_lifecycle(pool: PgPool) {
    let mut ctx = ScenarioContext::new();

    // 1. Create shared dependents first
    let playbooks = create_test_playbooks(&pool, 2).await;
    ctx.add_batch("playbooks-init", playbooks.clone());

    // 2. Create principals
    let journey_reqs = vec![
        CreateJourneySchema {
            name: "Journey 1".into(),
            playbook_id: playbooks[0].id,
        },
        CreateJourneySchema {
            name: "Journey 2".into(),
            playbook_id: playbooks[1].id,
        },
    ];

    let journeys = create_journeys(&pool, journey_reqs).await;
    ctx.add_batch("journeys-init", journeys.clone());

    // 3. Verify GET returns same data
    let fetched = get_journeys_by_ids(&pool,
        &journeys.iter().map(|j| j.id).collect::<Vec<_>>()
    ).await;

    assert_eq!(journeys.len(), fetched.len());
    for (created, fetched) in journeys.iter().zip(fetched.iter()) {
        assert_eq!(created.id, fetched.id);
        assert_eq!(created.name, fetched.name);
    }
}
```

### Testing Best Practices
- **Seed principals** explicitly in tests via `dapple_data::query::*::insert_*`
- **Use ScenarioContext** to track batches with naming pattern `"{endpoint}-init"`
- **Preserve ordering** — verify POST response order matches GET order
- **Wait for dependencies** — poll ScenarioContext, don't sleep
- **Log diagnostics** — CreateSchema payload, InsertArgs lengths, SQL rows on failure
- **Compare POST vs GET** — ensure INSERT...RETURNING matches SELECT results


## Common Mistakes

### API Design
- **Associative table endpoints** — Tables like `brief_group_briefs` don't get their own `/api/` endpoint; they're created with the parent
- **Separate path endpoints** — Versioned objects create paths atomically; don't create separate path endpoints

### Schema
- **Missing JsonSchema derive** — All schemas need `#[derive(JsonSchema, Serialize, Deserialize)]`
- **Using `object_id` in field names** — The PK is `id: Uuid` directly. FKs use `{table_singular}_id: Uuid`. There is no separate `object_id` field.

### Handlers
- **Wrong response wrapper** — Use `{ "objs": [...] }` for batch, `{ "obj": {...} }` for single
- **Lost ordering** — Preserve input order in responses

## Schema Naming

| Type | Pattern | Example |
|------|---------|---------|
| Single create | `Create{Resource}Schema` | `CreateJourneySchema` |
| Batch create | `Create{Resource}SetSchema` | `CreateJourneySetSchema` |
| Query enum | `Get{Resource}SetSchema` | `GetJourneySetSchema` |
| Public DTO | `{Resource}` | `Journey` |

## Route Registration

```rust
pub fn {resource}_routes() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route("/{resources}", post({resource}_post_handler))
        .api_route("/{resources}", get({resource}_index_handler))
}
```

## Testing Requirements

After adding an endpoint:
1. Create `tests/consumer/{resource}.rs` implementing `Endpoint` trait
2. Add batch fields to `ScenarioContext`
3. Add trial to `tests/user_lifecycle.rs`
4. Run `cargo test --test user-lifecycle --package dapple-prompt-api`

## Testing Repair Operations

**Asserting `*_repaired: true` (or any equivalent boolean flag claiming "we did the work") is NOT sufficient evidence of a successful repair.** Any test that exercises a repair / backfill / rebuild / refresh code path MUST corrupt the DB state first (DELETE or UPDATE a row that the operation is supposed to repair), then assert via direct SQL query that the corrupted state has been restored.

### Right pattern (block-level backfill tests in `tests/guide_ingest_test.rs`)

1. Ingest the fixture.
2. `DELETE FROM rev.guide_blocks WHERE chapter_id = $1` — corrupt the state.
3. `POST /api/guides/{id}/ingest?backfill=true`.
4. `SELECT COUNT(*) FROM rev.guide_blocks WHERE chapter_id = $1` — assert the rows are back.

### Wrong pattern (the two deleted derived-object tests, kept here as a cautionary example)

1. Ingest the fixture.
2. `POST /api/guides/{id}/ingest?backfill=true`.
3. Assert `response.derived_objects_repaired == true`.

The wrong pattern doesn't fail when the repair is a no-op; it just signs off on the response shape. For 20+ iterations the production `?backfill=true` flag was a documented no-op (`let _ = params.backfill;` at the top of `guide_ingest_patch_handler`) and both `test_backfill_repairs_derived_objects` and `test_backfill_derived_objects_idempotent` reported green CI because their only assertion was on `response.derived_objects_repaired == true` — a boolean the function set unconditionally whenever the materializers ran, regardless of whether they wrote anything.

Real repair tests must touch the DB on **both** sides of the operation: one query to establish the corrupt state, one query to verify restoration. See `test_backfill_repairs_brief_unit_input_control_spec` for the canonical example.

### Code-review checklist

When reviewing a PR that adds or modifies a test against a repair endpoint, look for two SQL queries (or two DTO comparisons against persisted state):

- [ ] **Setup query** establishes the corrupt state (DELETE, UPDATE, or a missing-row precondition).
- [ ] **Verification query** confirms the corruption has been undone.
- [ ] Either query may be wrapped in a helper function, but both must run against the live DB — not the response body.

If you see only response-body assertions, **request changes**. The test isn't testing the repair; it's testing that the endpoint returns a 200 with the right JSON shape.

## File Locations

```
src/schema/{resource}.rs    # Request/response types
src/handler/{resource}.rs   # Business logic
src/route/{resource}.rs     # Route definitions
tests/consumer/{resource}.rs # Test consumer
```

## Asset Content Endpoints (Streaming)

For assets containing large binary data (audio, video), the API supports chunk-based streaming:

### Required Endpoints

| Endpoint | Purpose |
|----------|---------|
| `GET /api/assets/{id}` | Metadata only (no contents) |
| `GET /api/assets/{id}/content` | Raw binary stream from chunks |
| `GET /api/drafts/{id}/content` | Raw binary stream (may be incomplete) |

### Content Endpoint Requirements

The `/content` endpoint must support HTTP streaming for browser `<audio>`/`<video>` elements:

```
GET /api/assets/{id}/content
Accept: */*
Range: bytes=0-1023  (optional)

Response (full):
HTTP 200 OK
Content-Type: audio/wav
Content-Length: 1440000
Accept-Ranges: bytes
[binary data streamed from chunks]

Response (range):
HTTP 206 Partial Content
Content-Type: audio/wav
Content-Range: bytes 0-1023/1440000
Content-Length: 1024
[partial binary data from relevant chunks]
```

### Handler Pattern for Streaming

```rust
pub async fn asset_content_handler(
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> impl IntoApiResponse {
    // 1. Get asset metadata (NOT contents)
    let asset = get_asset_metadata(&state.pool, id).await?;

    // 2. Parse Range header if present (content_length calculated from chunks)
    let range = headers.get("Range")
        .and_then(|v| parse_range(v, content_length));

    // 3. Stream from chunks
    let body = if let Some((start, end)) = range {
        // Query only chunks that overlap the range
        let chunks = select_chunks_for_range(&state.pool, asset.id, start, end).await?;
        // Return 206 Partial Content
        stream_chunks_with_range(chunks, start, end)
    } else {
        // Query all chunks for asset
        let chunks = select_asset_chunks(&state.pool, asset.id).await?;
        // Return 200 OK with full content
        stream_chunks(chunks)
    };

    // 4. Set headers
    Response::builder()
        .header("Content-Type", asset.mime_type)
        .header("Content-Length", content_length)
        .header("Accept-Ranges", "bytes")
        .body(body)
}
```

### Mime Type Mapping

Map `asset_kind` to MIME types:

| asset_kind | Content-Type |
|------------|--------------|
| `audio` | `audio/wav` (or detect from content) |
| `image` | `image/png`, `image/jpeg`, etc. |
| `video` | `video/mp4`, etc. |
| `document` | `application/pdf`, etc. |
| `binary` | `application/octet-stream` |

### Creating Lifecycle Tests
1. Create `tests/consumer/{principal}.rs` with endpoint implementation
2. Implement `Endpoint` trait with all required methods
3. Add batch fields to `ScenarioContext` in `tests/consumer/mod.rs`:
   - `{principal}_batches: Vec<Vec<Principal>>`
   - `{principal}_named_batches: HashMap<String, BatchHandle<Principal>>`
4. Add registration and random selection methods to `ScenarioContext`
5. Add trials to `tests/user_lifecycle.rs`:
   - Wait for dependencies using polling loop
   - Create test objects via endpoint
   - Register batch in `ScenarioContext` with naming pattern `"init-{principals}"`
6. Add trial to the `run()` call in `lifecycle()` function

### Common Mistakes

- **Returning base64 for binary** — `/content` must return raw bytes, not JSON
- **Missing Content-Length** — Browsers need this for progress/seeking
- **No Range support** — Audio/video seeking requires HTTP 206
- **Loading all chunks for range** — Only fetch chunks that overlap the requested byte range
- **Not streaming** — Use `Body::from_stream()` to avoid loading all chunks into memory

See `docs/plans/asset-streaming/002-streaming-content-endpoint.md` for full implementation plan.

### Integration Test Pitfalls
- ❌ **Creating test trials for paths separately from objects** — Tests for versioned objects should create the object (which includes creating the path) in a single trial, not separate path and object trials.
- ❌ **Over-complicating wait conditions** — Keep wait conditions simple: just check if the named batch key exists. Don't add extra conditions that can cause false negatives.
- ❌ **Spending tokens on sqlx JSON debugging** — If sqlx has issues, delete `.sqlx/` cache and run `dapple sqlx prepare` once (raw: `cargo sqlx prepare --workspace -- --all-targets`). Don't parse the JSON files manually.
- ❌ **Creating dependencies between trials when not needed** — If trial B needs data from trial A, consider whether B should just create its own data instead of waiting for A. Shared state between trials via Arc<Mutex> can be problematic with libtest_mimic.
- ❌ Using `timestamp` without timezone
- ❌ Inserting grandchildren from parent (only insert direct dependents)
- ❌ Comparing POST responses without ordering
