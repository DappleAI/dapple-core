# Guide Search & Agent RAG: Architecture Plan

> **Errata (2026-02-24):** The DDL in Section 1 of this plan contains several type
> mismatches with the actual codebase (BIGINT PKs instead of UUID, 1536 dimensions
> instead of 768, `object_id` columns that do not exist in the schema). See
> **`docs/plans/pg-vector-phase2-implementation.md`** for the corrected,
> implementation-ready specification with exact DDL, models, queries, and hook points.
> The high-level architecture, phasing, and read/write path design in this document
> remain valid.

## Context

This plan covers two consumers of the same pgvector infrastructure:

1. **Librarian guide** — A `/references` page in dapple-ui where users navigate all guides. The core experience is a guide (authored in dapple-prompt like any other) whose content is *about* the other guides. It has direction — walks you through — but is interruptible. Users can bail out at any point to explore whatever guide was just described. It includes a Nushell search command that queries guide content via Conduit. The librarian guide is the universal search interface: guides first, other entity types later.

2. **Agent RAG** — When agents execute tasks within the dispatch pipeline, Conduit enriches their prompt context with relevant guide content pulled from pgvector and held in Lance (an in-process vector store). Agents never search — they receive precisely the content relevant to their execution point, audience, and locale.

Both consumers share the same pgvector cold store, the same embedding pipeline, the same metadata schema, and the same Conduit query layer. They differ in how queries are constructed (human search terms vs. Flow-assembled execution context) and what happens with results (returned as JSON vs. loaded into Lance for prompt injection).

**Out of scope:**
- The prompt-bar UI treatment for search input (needs more design thinking)
- Embeddings for non-guide entity types (hats, projects) — each gets its own table, same pattern, future work

---

## Architecture

```
                              WRITE PATH
                              ==========

  Guide canonical        ┌─────────────────────────────────────────────┐
  sync event ───────────►│            Embedding Pipeline               │
                         │                                             │
                         │  1. Extract blocks from canonical version   │
                         │  2. Chunk (block-level)                     │
                         │  3. Generate embeddings (model TBD — Q1)    │
                         │  4. Store in pgvector with tags + locales   │
                         │  5. Formalized metadata per chunk           │
                         └──────────────────┬──────────────────────────┘
                                            │
                                            ▼
                         ┌─────────────────────────────────────────────┐
                         │           pgvector (Cold Store)             │
                         │                                             │
                         │  rev.guide_content_embeddings               │
                         │  rev.guide_embedding_tags                   │
                         │  rev.guide_embedding_locales                │
                         │                                             │
                         │  Durable. All guides, all audiences,        │
                         │  all locales. Updated on canonical sync.    │
                         └──────────────────┬──────────────────────────┘
                                            │
                              READ PATH     │
                              =========     │
                                            │
              ┌─────────────────────────────┴─────────────────────────┐
              │                                                       │
              ▼                                                       ▼
  ┌───────────────────────┐                         ┌─────────────────────────────┐
  │   Human Search Path   │                         │      Agent RAG Path         │
  │                       │                         │                             │
  │  Librarian guide      │                         │  Flow assembles             │
  │  Nushell task ─► HTTP │                         │  RagQueryContext ──► Conduit │
  │  to Conduit endpoint  │                         │                             │
  │                       │                         │  Conduit queries pgvector,  │
  │  Conduit queries      │                         │  loads into Lance,          │
  │  pgvector, returns    │                         │  injects into prompt        │
  │  ranked JSON to UI    │                         │                             │
  └───────────────────────┘                         │  Lance accumulates across   │
                                                    │  steps within dispatch      │
                                                    └─────────────────────────────┘
```

---

## 1. pgvector Schema (Cold Store)

### Tables

```sql
-- Core embeddings table. One row per embedded chunk (block-level by default).
CREATE TABLE rev.guide_content_embeddings (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  object_id UUID DEFAULT gen_random_uuid() NOT NULL UNIQUE,
  guide_id BIGINT NOT NULL REFERENCES rev.guides(id),
  chapter_id BIGINT NOT NULL REFERENCES rev.guide_chapters(id),
  block_id BIGINT NOT NULL REFERENCES rev.guide_blocks(id),
  directive_type_id SMALLINT NOT NULL REFERENCES rev.guide_block_directive_types(id),
  chunk_text TEXT NOT NULL,              -- The source text that was embedded
  embedding vector(1536) NOT NULL,       -- Dimension depends on model choice (Q1)
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,  -- Formalized schema below
  created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

-- Junction table: which audience tags apply to each embedding.
-- Mirrors the guide_block_tags pattern. Populated from the source block's
-- tags at embedding time. Untagged blocks get the 'universal' tag.
CREATE TABLE rev.guide_embedding_tags (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  embedding_id BIGINT NOT NULL REFERENCES rev.guide_content_embeddings(id) ON DELETE CASCADE,
  tag_id SMALLINT NOT NULL REFERENCES rev.guide_tags(id),
  UNIQUE (embedding_id, tag_id)
);

-- Locale lookup table. Seeded with supported locales.
CREATE TABLE rev.locales (
  id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,              -- e.g. 'en', 'es', 'fr', 'pt-br', 'pt-pt'
  description TEXT,
  created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

-- Junction table: which locales an embedding is relevant to.
-- A locale-specific block (French prose) gets one locale. A locale-neutral
-- block (code snippet, diagram) can be associated with multiple locales
-- to avoid duplicate embeddings for identical content.
CREATE TABLE rev.guide_embedding_locales (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  embedding_id BIGINT NOT NULL REFERENCES rev.guide_content_embeddings(id) ON DELETE CASCADE,
  locale_id SMALLINT NOT NULL REFERENCES rev.locales(id),
  UNIQUE (embedding_id, locale_id)
);

-- Similarity search index. IVFFlat is a good starting point; can migrate
-- to HNSW later if recall needs to improve at scale.
CREATE INDEX ON rev.guide_content_embeddings
  USING ivfflat (embedding vector_cosine_ops)
  WITH (lists = 100);

-- Fast lookup: "all embeddings visible to audience X"
CREATE INDEX ON rev.guide_embedding_tags (tag_id);

-- Fast lookup: "all embeddings for locale Y"
CREATE INDEX ON rev.guide_embedding_locales (locale_id);

-- Fast cleanup on re-embedding (delete old embeddings for a guide version)
CREATE INDEX ON rev.guide_content_embeddings (guide_id);
```

### Schema design notes

- **Junction tables for both tags and locales.** Tags are a small set today (6 values) and locales are modest, but junction tables avoid baking in cardinality assumptions. Tags mirror the existing `rev.guide_block_tags` pattern. Locales use a dedicated `rev.locales` lookup table rather than free-text, keeping locale values consistent and queryable.
- **Locale junction avoids duplicate embeddings.** A locale-specific block (French prose) maps to one locale. A locale-neutral block (code snippet, Nushell command, diagram) can map to all relevant locales without duplicating the embedding vector. This matters when you have 4+ Portuguese variants — you embed once, associate many.
- **`ON DELETE CASCADE`** on both junction tables so re-embedding a guide (delete old rows, insert new) doesn't leave orphaned tag or locale rows.
- **`directive_type_id`** is denormalized from the block for query convenience — search results often need to know whether a hit is prose, a step, a stage, etc. without joining back through blocks.
- **`metadata` JSONB** carries structured fields for both display and ranking. Formalized schema below — not freeform.
- **Per-entity-type tables.** Guides get `guide_content_embeddings`. Hats, projects, and other entity types will get their own embeddings tables (e.g. `rev.hat_embeddings`) — not mixed into one table. Cross-entity similarity is meaningless, and the UI wants best-match-per-category results. Separate tables keep FKs clean, indexes small, and pipelines independent.

### Example query: audience + locale filtered search

```sql
SELECT e.chunk_text, e.metadata, 1 - (e.embedding <=> $1) AS similarity
FROM rev.guide_content_embeddings e
JOIN rev.guide_embedding_tags et ON et.embedding_id = e.id
JOIN rev.guide_embedding_locales el ON el.embedding_id = e.id
WHERE et.tag_id IN (
  SELECT id FROM rev.guide_tags WHERE key IN ('product', 'universal')
)
AND el.locale_id = (
  SELECT id FROM rev.locales WHERE key = 'pt-br'
)
ORDER BY e.embedding <=> $1
LIMIT 20;
```

---

## 2. Embedding Metadata Schema

The `metadata` JSONB column on each embedding row conforms to this structure. Both pgvector queries (for ranking boosts) and Lance (for agent context) depend on these fields being present and consistently shaped.

```json
{
  "guide_slug": "api-consumer-guide",
  "guide_title": "API Consumer Guide",
  "chapter_title": "Authentication",
  "chapter_object_id": "uuid",
  "block_object_id": "uuid",
  "block_name": "gather-credentials",
  "directive_type": "step",
  "depth": 2,
  "position": 0,
  "parent_stage_name": "Setup",
  "parent_stage_object_id": "uuid",
  "has_emphasis": true,
  "has_annotations": false,
  "brief_unit_keys": ["agent_context", "product_context"]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `guide_slug` | string | Guide's URL-safe identifier |
| `guide_title` | string | Guide's display title |
| `chapter_title` | string | Containing chapter's title |
| `chapter_object_id` | uuid | Containing chapter's object_id |
| `block_object_id` | uuid | Source block's object_id |
| `block_name` | string? | Block's name from directive metadata (null if unnamed) |
| `directive_type` | string | Block type: prose, stage, step, ref |
| `depth` | int | Hierarchy depth (0 = chapter-level, 1 = stage, 2 = step, 3 = nested) |
| `position` | int | Ordinal position within parent (0-indexed) |
| `parent_stage_name` | string? | Containing stage name (null if top-level) |
| `parent_stage_object_id` | uuid? | Containing stage object_id |
| `has_emphasis` | bool | Whether source text contains bold/italic formatting |
| `has_annotations` | bool | Whether block has non-default attributes |
| `brief_unit_keys` | string[] | Brief unit keys derived from block tags (e.g. `agent_context`, `product_context`) |

This schema enables:
- **Human search:** Display-oriented fields (titles, slugs) for rendering results
- **Agent RAG:** Hierarchy-aware ranking (boost same chapter/stage, boost named blocks, boost structural content)
- **Overlay alignment:** `brief_unit_keys` let Conduit match embeddings to the overlay/audience context Flow provides

---

## 3. Embedding Pipeline (Write Path)

### Trigger: Canonical version sync

When a guide version is designated as canonical:

1. Delete existing embeddings for the guide (`DELETE FROM rev.guide_content_embeddings WHERE guide_id = $1` — cascades clean up junction tables)
2. Extract blocks from the new canonical version
3. For each embeddable block (prose, stage, step — skip nushell, brief_pack, inference_profile):
   a. Chunk the content (block-level to start — see Q2)
   b. Generate embedding vector via the chosen model (see Q1)
   c. Insert into `rev.guide_content_embeddings` with formalized metadata
   d. Copy block's tags to `rev.guide_embedding_tags` (default to `universal` if untagged)
   e. Associate locales in `rev.guide_embedding_locales` (guide's locale for text blocks, all locales for locale-neutral blocks)
4. All within a transaction — atomic swap of old → new embeddings

### Re-embedding triggers

- **Primary:** Canonical version sync (guide content changes)
- **Tag changes:** Adding/removing tags on blocks updates junction table entries only — vectors don't change, no re-embedding needed
- **Locale additions:** New locale added to system — existing locale-neutral blocks should be associated via junction table update, no re-embedding

---

## 4. Conduit: RAG Orchestrator (Read Path)

Conduit serves both consumers through the same query layer.

### 4a. pgvector query layer

Conduit receives a query context and translates it into a pgvector similarity query with audience, locale, and hierarchy filtering.

For **human search** (librarian guide Nushell command), the context is simple: query string, audience tags, locale.

For **agent RAG** (dispatch pipeline), the context is the full `RagQueryContext` assembled by Flow:

```rust
pub struct RagQueryContext {
    /// Which guide(s) to search within. Derived from the playbook's
    /// source guide. Could be expanded to related guides.
    pub guide_object_ids: Vec<Uuid>,

    /// Audience tags to filter by. Derived from the task's tags
    /// (e.g., ["agent", "universal"] for an agent-facing task).
    pub audience_tags: Vec<String>,

    /// Locale for the current execution.
    pub locale: String,

    /// Current position in the guide hierarchy, for proximity-based
    /// ranking. Content closer to where we are in the guide is more
    /// relevant than content from distant chapters.
    pub hierarchy_position: HierarchyPosition,

    /// Overlay visibility — which value slots are exposed at this
    /// execution point. Lets Conduit filter embeddings that reference
    /// masked concepts.
    pub visible_slots: Vec<ValueSlotPath>,

    /// What the agent already knows. Conduit can use this to avoid
    /// retrieving content that's redundant with accumulated context.
    pub accumulated_context_keys: Vec<String>,
}

pub struct HierarchyPosition {
    pub chapter_object_id: Option<Uuid>,
    pub stage_name: Option<String>,
    pub step_name: Option<String>,
    /// Depth in the hierarchy (0 = guide-level, 1 = chapter, 2 = stage, 3 = step)
    pub depth: u8,
}
```

**Ranking enhancements using metadata:**
- Boost results from the same chapter/stage/step as current hierarchy position
- Boost results with directive_type `stage` or `step` (structural/summary content)
- De-rank results whose metadata keys overlap with `accumulated_context_keys` (agent already has this)

### 4b. Lance lifecycle management (Hot Store)

Lance is an in-process vector store that Conduit holds for the duration of a dispatch. It accumulates relevant context across steps so agents build up knowledge as the dispatch progresses.

| Event | Action |
|-------|--------|
| Dispatch starts | Create new Lance instance, keyed by `dispatch_id` |
| Step executes | Query pgvector with step's context, load results into Lance |
| Prompt builds | Pass Lance handle to RAG pipeline as retrieval source |
| Step completes | Lance retains results — next step inherits accumulated context |
| Dispatch completes | Drop Lance instance, free memory |
| Dispatch fails | Drop Lance instance (or persist for debugging — see Q3) |

**Lance instance contents per row:**
- `chunk_text` — the embedded content
- `embedding` — the vector (for in-Lance similarity search during prompt execution)
- `metadata` — the formalized metadata from section 2
- `source_block_id` — for deduplication across steps
- `step_added` — which step loaded this row (for provenance)

**Deduplication:** When a new step's pgvector results include blocks already in Lance, skip them. The `source_block_id` provides the dedup key.

**Why Lance:**
- In-process, no network hop
- Columnar format optimized for vector search
- Supports incremental additions
- Conduit holds the handle and passes it forward — no serialization overhead between steps

### 4c. RAG prompt integration

When building a prompt for an agent task:

1. Conduit has the Lance handle for this dispatch
2. The prompt template references RAG context (e.g., `{{rag_context}}`)
3. Conduit queries Lance with the prompt's semantic needs (derived from the prompt template + task inputs)
4. Retrieved chunks are formatted and injected into the prompt
5. The LLM sees: task inputs + accumulated data + RAG-retrieved guide content

The agent gets precisely the guide content relevant to its current execution point, audience, and locale — without ever having to search for it.

---

## 5. Flow → Conduit Handoff

### What Flow already builds

At task execution time, Flow has assembled (from `dispatch_runner.rs`):

| Field | Source | Purpose for RAG |
|-------|--------|-----------------|
| `dispatch_id` | Dispatch entity | Scope the Lance instance |
| `journey_id` | Journey entity | Cross-dispatch accumulation (future) |
| `dispatch_intent` | User inputs from debrief | Semantic context for query |
| `stage_id` / `playbook_stage_id` | Current stage | Hierarchy position filtering |
| `step_id` / `playbook_step_id` | Current step | Hierarchy position filtering |
| `stage_params` | Stage configuration | May contain audience/scope hints |
| `step_params` | Step configuration | May contain audience/scope hints |
| `accumulated_data` | Prior task outputs (flattened) | What the agent already knows |
| `task.function_input` | Task definition | Which guide, which prompt |

### What Flow needs to add

Flow assembles the `RagQueryContext` struct from the fields above and passes it to Conduit as part of the task handoff. Flow logs the full `RagQueryContext` to `dispatch_task_attempt_events` alongside existing context snapshots — same logging discipline, everything we send to Conduit is recorded.

---

## 6. Librarian Guide & Human Search

### The guide

A new guide whose content is *about* all other guides:
- Authored using existing directive types: `stage`, `step`, `prose`, `ref`
- Set `classification` to `"reference"` (or `"internal"` — see X1)
- Content: stages that group guides by theme or audience, prose blocks describing each guide
- Includes a Nushell search command as a task block
- Does NOT need a matching playbook — the UI renders it directly without the journey pipeline

### The Nushell search command

A reusable Nushell module that calls Conduit's search endpoint:
- Calls Conduit via HTTP (Conduit URL is already available in `DAPPLE_METADATA`)
- Accepts: query string, optional audience filter (defaults to `["universal"]`), required locale
- Conduit handles embedding the query, filtering, ranking, and returning results
- One query path for both human search and agent RAG — same Conduit layer, different context

**JSON result schema (contract with dapple-ui):**
```json
[
  {
    "guide_slug": "api-consumer-guide",
    "guide_title": "API Consumer Guide",
    "chapter_title": "Authentication",
    "directive_type": "prose",
    "block_object_id": "abc123",
    "tags": ["product", "universal"],
    "content": "To authenticate with the API, first obtain a service token...",
    "relevance_score": 0.92
  }
]
```

### `by_classification` query for guides API

The UI needs to exclude the librarian guide (and any future internal guides) from the explore carousel:
- Add `by_classification` query variant to `GET /api/guides/` and `GET /api/guides/cards`
- Allow filtering like: `?query=by_classification&classification=reference`
- UI can filter client-side until this is available

---

## 7. dapple-ui Work

| # | Title | Phase | Depends on |
|---|---|---|---|
| U1 | Create `/references` page | 1 | Librarian guide authored |
| U2 | Build `ReferenceRenderer` component | 1 | — |
| U3 | Create `useReferenceGuide` hook | 1 | — |
| U4 | Add "References" to sidebar navigation | 1 | U1 |
| U5 | Filter explore carousel to exclude internal guides | 1 | — |
| U6 | Wire dispatch integration for search results | 3 | Nushell search command |
| U7 | Search result rendering | 3 | U6 |

---

## Open Questions

### Q1: Embedding model
- Which model? Affects dimensions, cost, multilingual quality, hosted vs. self-hosted.
- Does Conduit already have infrastructure for calling embedding models (e.g., for T5/TTS)?
- Multilingual support is important (en, es, fr, pt-br, pt-pt and growing).

### Q2: Chunking strategy
- Block-level (one embedding per visible block) is the starting point.
- Stage-group level (stage + child blocks = one embedding) provides more context per result.
- Does the agent benefit from larger chunks (more context per retrieval) or smaller chunks (more precise retrieval)?
- This may differ by directive type — prose blocks are self-contained, but step blocks may need their parent stage for context.
- What's the expected corpus size? (Number of guides x blocks)

### Q3: Lance persistence on failure
- When a dispatch fails, should the Lance instance be persisted for debugging?
- Could write Lance to disk (it supports file-backed storage) keyed by dispatch_id.
- Trade-off: debugging value vs. disk usage.

### Q4: Lance scoping — dispatch vs. journey
- Current proposal: Lance is scoped per dispatch, dropped on dispatch end.
- Alternative: Lance persists across dispatches within a journey, so later dispatches inherit accumulated RAG context from earlier ones.
- Journey-scoped Lance would match how `accumulated_data` already flows across dispatches via `select_completed_task_context_by_journey`.
- Trade-off: richer context vs. memory lifetime and stale content.

### Q5: Librarian guide organization
- How should the librarian guide's content be structured? By audience, by task, by guide?
- Who authors it? Is there a guide authoring workflow to follow?

### Q6: What triggers re-embedding?
- Canonical version sync is the primary trigger (guide content changes).
- Should adding/changing tags on blocks trigger re-embedding? (The vectors don't change, but the junction table entries do.)
- Should locale additions trigger re-association? (New locale added to system — existing locale-neutral blocks should be associated.)

### Q7: Conduit search endpoint shape
- What does the Conduit RAG query endpoint look like? (REST? gRPC?)
- Does it accept the full `RagQueryContext` struct, or a simplified form for human-facing search?
- Does it need both a "simple search" endpoint (for Nushell) and a "full RAG" endpoint (for Flow)?

---

## Cross-Cutting Agreements Needed

### X1: "Internal guide" convention
- What `classification` value marks a guide as internal? (`"reference"`, `"internal"`, something else?)
- Which API surfaces should exclude internal guides by default?
- Should the explore carousel, guide cards, and guide listing all filter them out?

### X2: Search result JSON schema
- The contract between the Nushell search command and dapple-ui
- See proposed schema in section 6 — needs backend team agreement

---

## Phasing

### Phase 1: Browsing experience (zero search infrastructure)
- Author the librarian guide — content work only
- Build dapple-ui: `/references` page, `ReferenceRenderer`, sidebar nav (U1-U5)
- Add `by_classification` query to guides API
- Agree on conventions (X1, X2)

### Phase 2: pgvector foundation
- Enable pgvector extension
- Create embeddings tables + junction tables (section 1)
- Build embedding pipeline triggered by canonical sync (section 3)
- Formalized metadata schema (section 2)

### Phase 3: Conduit query layer + human search
- Conduit pgvector query layer (section 4a)
- Nushell search command routed through Conduit (section 6)
- Librarian guide updated with search task block
- dapple-ui dispatch integration + result rendering (U6, U7)

### Phase 4: Agent RAG with Lance
- Flow builds and logs `RagQueryContext` (section 5)
- Conduit Lance lifecycle management (section 4b)
- Conduit RAG prompt integration (section 4c)
- Lance scoping decision (Q4)

### Future: Other entity types
- Hats, projects get their own embeddings tables and Nushell search commands
- Same pattern: per-entity-type table, junction tables, Conduit query layer
- The Nushell search command pattern is replicated per entity type
