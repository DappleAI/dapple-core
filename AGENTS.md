# Agent Instructions — dapple-prompt

This document provides guidance for LLM agents (Claude, etc.) working with this codebase.

## Quick Reference

Before making changes, understand:
1. **Journey-centric philosophy** — See CLAUDE.md "Journey-Centric Philosophy" section
2. **Data layer patterns** — See CLAUDE.md "Core Philosophy" section
3. **API consumer guide** — See Journey Planner's Guide chapters 14-17 (Authentication, API Patterns, Runtime Operations, Cookbook)

## Common Agent Tasks

### Adding a New Endpoint

1. Check if it's a Principal, Owned Dependent, or Shared Dependent
2. Follow the pattern in CLAUDE.md "When Adding a Principal Object"
3. Remember: Associative Dependents (e.g., `brief_group_briefs`) don't get endpoints

### Modifying the User Flow

1. Read the Journey-Centric Philosophy section first
2. Ask: "Does this change affect what users experience?"
3. If yes, ensure terminology stays internal (no "Brief" in user-facing strings)

### Working with Briefs

Briefs are accessed through Journeys, not standalone:
```
Journey → BriefPacks → BriefGroups → Briefs → BriefUnits
```

Never create endpoints or UIs that treat Briefs as independent entities.

### Testing Changes

**IMPORTANT**: Always run tests via nushell to ensure environment variables are loaded:

```bash
# Use nushell login shell to load environment (DAPPLE_PROMPT_DATABASE_URL, etc.)
nu -l -c 'cargo test --package dapple-prompt-api --test user-lifecycle'

# Or open an interactive nu shell first
nu -l
cargo test --package dapple-prompt-api --test user-lifecycle
```

**Why nushell?** The test suite requires environment variables like `DAPPLE_PROMPT_DATABASE_URL` and `DAPPLE_PROMPT_API_SIGNING_KEY` that are configured in the nushell environment profile (`<nushell-config-dir>/env.nu` → `dapple_sandbox.nu`; the config dir is `~/.config/nushell/` on Linux, `~/Library/Application Support/nushell/` on macOS).

**Test checklist:**
1. Verify the iterative flow works: debrief → dispatch → review → re-debrief
2. Check BriefGroup atomicity is preserved

## Key Files

| File | Purpose |
|------|---------|
| `CLAUDE.md` | Comprehensive development guide |
| `crates/dapple-prompt-api/docs/journey-planners-guide/` | Complete guide for playbook authoring and API consumption |
| `crates/dapple-prompt-api/docs/plan.md` | API improvement roadmap |

## Questions to Ask Before Changing

1. Does this maintain the journey-centric flow?
2. Am I exposing internal terminology to users?
3. Does this preserve BriefGroup atomicity?
4. Is this iterative (not one-shot linear)?

## The User Flow

```
1. Pick Playbook → Journey created (BriefPack assigned)
        ↓
2. Debrief → Partial or full (always complete BriefGroups atomically)
        ↓
3. Dispatch → Partial or full (based on what's been debriefed)
        ↓
4. Return to Journey → Complete or interrupted
        ↓
5. Review Results → Success, incomplete errors, or need more info
        ↓
6. Loop back → Re-debrief if needed, re-dispatch
        ↓
7. Complete → Enjoy results
```

## Terminology Quick Reference

| Internal | User Sees | Notes |
|----------|-----------|-------|
| Brief | (hidden) | Part of the debrief interview |
| BriefPack | (hidden) | Assigned to Journey at creation |
| BriefGroup | (hidden) | Atomic unit - must complete together |
| BriefUnit | (hidden) | Individual form field or content |
| Playbook | Workflow | What users pick to start |
| Journey | Session | Central runtime concept |
| Dispatch | (hidden) | Background execution |
| Debrief | Interview | Data gathering experience |
| InferenceProfile | (hidden) | Model configuration for Journey |

## Model Taxonomy Quick Reference

| Term | Purpose | Example |
|------|---------|---------|
| Provider | Where model is hosted | `anthropic`, `self-hosted` |
| Lineage | Prompting paradigm | `claude-3-5`, `vits`, `flan-t5` |
| Tier | Capability level | `sonnet`, `opus`, `ljspeech` |
| Release | Version snapshot | `20241022`, `v0.1` |
| Variant | Fine-tune branch | `acme-customer-v1` |
| ModelConfig | Resolved config | Contains lineage, provider, tier, release |
| InferenceProfile | Journey-level config | Maps prompts to models |

**Key tables:**
- `rev.providers`, `rev.lineages`, `rev.model_releases`, `rev.model_variants`
- `rev.inference_profiles`, `rev.inference_profile_configs`, `rev.journey_inference_profiles`

**Routing by lineage:** The conduit routes specialized prompts based on `lineage_key`:
- `vits` → TTS handler
- `flan-t5` → Summarization handler
- `modernbert` → MLM handler
- Others → LLM handler

## Anti-Patterns to Avoid

1. **Exposing "Brief" to users** — Use "form", "interview", or "debrief" instead
2. **Standalone Brief access** — Always traverse from Journey → BriefPacks
3. **Partial BriefGroup completion** — This is product-enforced atomicity
4. **Linear one-shot flows** — Support iteration (review → re-debrief → re-dispatch)
5. **N+1 queries** — Use CTEs to resolve object_id → id lookups inline
6. **Boolean columns for sparse flags** — Use sparse join tables instead of `is_admin BOOLEAN` on hot tables (see Database Patterns below)
7. **Stubbed/missing GET query variants** — Every list endpoint must have all variants fully wired (SQL + query function + handler match arm). System config entities (Tasks, Steps, Stages, Categories, Directives, ValueSlots, FluentMessages, Prompts, InferenceProfiles, Hats, Playbooks, Guides) **must** include an `All` variant. See `dapple-prompt-api/AGENTS.md` for the full variant requirements table.
8. **Mutating guide-derived objects via API** — Objects derived from guide ingestion (tracked in `rev.guide_derived_locks`) are read-only. Edit the source guide and re-ingest instead.
9. **`locale` field on guides** — Guides are locale-independent. There is no `locale` field on guides, guide metadata, or IngestPlan. All localized text lives in Fluent `.ftl` files under `locales/`.
10. **Inline content for Fluent-backed directives** — Text for stage, step, prose, brief_pack, and inference_profile blocks lives in `.ftl` files, not inline in the directive. Only nushell and ref blocks retain inline content.
11. **Parser accessing database** — The transform layer (`crates/dapple-parser/src/transform/`) is pure. It has no database access and produces an `IngestPlan` intermediate representation. Materialization happens in the handler.
12. **Creating separate parser in dapple-data** — The parser lives in the `dapple-parser` crate, not in `dapple-data`. Do not create parser modules in `crates/dapple-data/src/`.

## Database Patterns

### Sparse Tables Over Boolean Flags

**Anti-pattern:** Adding `is_admin BOOLEAN NOT NULL DEFAULT false` to a frequently-accessed table where 90%+ of rows will be `false`.

**Preferred:** Create a sparse join table where presence indicates the flag is true:

```sql
-- BAD: Wastes storage on every row
ALTER TABLE web.api_tokens ADD COLUMN is_admin BOOLEAN NOT NULL DEFAULT false;

-- GOOD: Only stores rows where the flag is true
CREATE TABLE web.admin_api_tokens (
    api_token_id BIGINT PRIMARY KEY REFERENCES web.api_tokens(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

Query with a LEFT JOIN and derive the boolean:
```sql
SELECT t.*, (a.api_token_id IS NOT NULL) AS is_admin
FROM web.api_tokens t
LEFT JOIN web.admin_api_tokens a ON a.api_token_id = t.id;
```

### sqlx Type Override Syntax

When sqlx infers nullable types incorrectly (common with LEFT JOINs, COALESCE, subqueries), use column alias suffixes to override:

| Suffix | Meaning | Use When |
|--------|---------|----------|
| `!` | Force NOT NULL | COALESCE guarantees non-null but sqlx doesn't know |
| `?` | Force NULL | Column is NOT NULL but you want Option<T> |
| `: type` | Override type | Need specific Rust type mapping |

Example:
```sql
SELECT
    -- sqlx thinks this is nullable because of LEFT JOIN, but COALESCE guarantees non-null
    COALESCE(a.api_token_id IS NOT NULL, false) AS "is_admin!",
    -- Force array to be non-nullable
    COALESCE(array_agg(r.role), ARRAY['user']::text[]) AS "user_roles!"
FROM ...
```

Maps to Rust struct fields `is_admin: bool` and `user_roles: Vec<String>` (not `Option<...>`).

## API Gaps (See plan.md)

Current known gaps that may affect your work:
- Issue #1: BriefPackRef is too sparse (only has object_id)
- Issue #2: No BriefGroup fetch endpoint
- Issue #3: No batch fetch for BriefPacks
- Issue #4: Can't filter Briefs by Journey
- Issue #5: Stage-driven dispatch (future)
- Issue #6: Journey interrupt API (future)

If you encounter these limitations, document them rather than working around them incorrectly.

## Flow Logic Pattern

When implementing features that involve flow control in playbooks:

1. **Flow rules are Owned Dependents** — They belong to playbook relationships (stages/steps/tasks), not standalone
2. **No separate endpoint** — Flow rules are created/fetched as part of playbook operations
3. **Entry vs Exit phases** — Entry rules determine if/how to start; exit rules determine what happens after
4. **First-match wins** — Rules evaluated in ordinal order; first true condition determines action
5. **Default is proceed** — If no rules match, execution continues normally
6. **Branch targets must exist** — Validate at creation time that branch targets reference valid stages/steps
7. **Retry needs state** — Track attempt counts in message metadata or dispatch_task_attempts

### Valid Actions by Phase

| Action | Entry | Exit | Notes |
|--------|-------|------|-------|
| proceed | Yes | Yes | Continue normally |
| skip | Yes | No | Skip this element |
| fail | Yes | Yes | Halt with error |
| loop | No | Yes | Re-execute same element |
| retry | No | Yes | Re-execute with backoff |
| branch | No | Yes | Jump to different element |

See `docs/plans/flow-logic-tables.md` for full architecture details.

## Guide Ingestion Pattern

When implementing features that involve guides:

1. **Guides are versioned principals (Fluent-native)** -- They use the unified versioning infrastructure (`object_paths` -> `guide_path_details` -> `versioned_objects` -> `guides`). Follow the same CTE insert chain as Hats/Directives. Guides have **no `locale` field** -- the guide structure is locale-independent. All human-readable text lives in Fluent (`.ftl`) files under `locales/` in the guide directory.

2. **Chapters and blocks are owned dependents** -- They are created atomically within a transaction when a guide is inserted. No separate endpoints for chapters or blocks. No `/api/guide-chapters/` or `/api/guide-blocks/`.

3. **Tags are from a lookup table** -- `rev.guide_tags` is a seeded lookup table with referential integrity. Do not create new tags without a migration. Tag keys map to BriefUnit `unit_key` values at materialization time.

4. **Guide-derived objects are read-only via API** -- Check `rev.guide_derived_locks` before allowing PATCH/DELETE on tasks, steps, stages, playbooks, briefs, brief_groups, brief_packs, and inference_profiles. Return 409 Conflict if locked.

5. **Re-ingestion creates new versions** -- Never UPDATE existing guide data. Use the path versioning mechanism to create v0.0.2, v0.0.3, etc.

6. **Block-level provenance is mandatory** -- Every materialized object must have a `rev.guide_block_provenances` entry linking it to the source block.

7. **Playbook derivation requires explicit annotation** -- Only chapters with `:::stage` directives produce Playbook stages. Nothing is inferred from markdown structure.

8. **No tag inheritance** -- Every block declares its own tags explicitly. Do not infer tags from parent chapters or guide-level metadata.

9. **Parser lives in dapple-parser** -- The `:::` fence directive parser, SUMMARY.md parser, and transform layer are in `crates/dapple-parser/`. The ingest handler is in `crates/dapple-prompt-api/src/handler/guide_ingest.rs`.

10. **Transform layer is pure (no DB access)** -- The transform layer in `crates/dapple-parser/src/transform/` converts a `ParsedGuide` AST into an `IngestPlan` intermediate representation. It produces definition types (`PlaybookDef`, `TaskDef`, `BriefPackDef`, `InferenceProfileDef`, `RefDef`, `CardDef`, `AssetFileEntry`) and `FluentMessageSet` entries. The handler then materializes these into database entities within an atomic transaction.

11. **Nushell and ref blocks keep inline content** -- Under the Fluent-native model, `nushell` blocks retain their script content and `ref` blocks retain their reference data. All other directive types (prose, stage, step, brief_pack, inference_profile) are Fluent-backed -- their text lives in `.ftl` files, not inline.

12. **Atomic transaction materializes all objects** -- The ingest handler materializes the guide, chapters, blocks, tags, assets, cards, and derived objects in a single database transaction. Never materialize guide components piecemeal.

### Parser and Transform Pipeline

The guide system uses a three-phase pipeline implemented across two crates:

**Phase 1: Parse** (`dapple_parser::parse_guide()` -> `ParsedGuide`)
- Reads `guide.toml` (preferred) or `book.toml` (legacy fallback) for metadata
- Reads `SUMMARY.md` (guide root first, then `src/SUMMARY.md` legacy fallback)
- Reads chapter `.md` files, tokenizes `:::` fence directives, classifies blocks
- Discovers asset files (non-markdown, non-toml, non-ftl files)
- Discovers and parses Fluent `.ftl` files from `locales/` directory

**Phase 2: Plan** (`dapple_parser::transform::plan_guide()` -> `IngestPlan`)
- Pure transform with no database access
- Extracts `CardDef` from guide.toml `[card]` section
- Collects `AssetFileEntry` from discovered files (reads bytes)
- Extracts `PlaybookDef` from `:::stage`/`:::step` block hierarchy
- Extracts `TaskDef` from `:::nushell` blocks
- Extracts `BriefPackDef` from `:::brief_pack` blocks
- Extracts `InferenceProfileDef` from `:::inference_profile` blocks
- Extracts `RefDef` from `:::ref` blocks
- Builds `FluentMessageSet` entries from `locales/*.ftl` files (keyed by message name)
- Links directive nushell blocks to their target steps/stages
- Collects validation warnings

**Phase 3: Materialize** (`guide_ingest.rs` handler -> database entities)
- Uploads assets via draft/chunk/finalize pipeline
- Inserts guide (path + versioned object + chapters + blocks + tags) atomically
- Creates guide_asset associations with role assignment
- Creates guide_card if card metadata exists
- Materializes derived objects (playbooks, tasks, brief packs, inference profiles)

**How to add a new directive type:**
1. Add the directive name to `KNOWN_DIRECTIVES` in `crates/dapple-parser/src/lib.rs`
2. Create a `Def` type in `crates/dapple-parser/src/transform/mod.rs` (e.g., `NewDirectiveDef`)
3. Add a match arm in `extract_derived_from_block()` for the new directive
4. Implement an extraction function (e.g., in a new `transform/new_directive.rs` submodule)
5. Add the `Vec<NewDirectiveDef>` field to `IngestPlan`
6. Add materialization logic in `crates/dapple-prompt-api/src/handler/guide_ingest.rs`

### Shared Content Pattern

Shared content (common blocks, reference materials) is ingested separately from individual guides:

1. **Separate ingest endpoint** -- Shared content uses its own ingest path, not the guide ingest endpoint
2. **Scope visibility** -- Content is scoped: `common` (visible to all guides) or `internal` (visible to internal guides only)
3. **Reference resolution** -- Guides reference shared content via `:::ref` blocks with a `slug` attribute, or via the `ref` attribute on other directives

### Render System

The `dapple-render` crate produces PDF output from guide content, filtering by audience:

1. **Named render profiles** -- Each profile defines a `tag_filter` (set of tag keys) and `style_config` (cover page, branding, watermark). Profiles correspond to different audiences (e.g., consumer, technical, internal).

2. **Tag-based block filtering** -- A block is included if it has no tags (universal content) or any of its tags appear in the profile's `tag_filter`. Chapters with no matching blocks are excluded entirely.

3. **Render pipeline** -- Filter blocks by tag set, render cover page (title, card image, version), generate table of contents, render chapters with filtered blocks, output PDF bytes.

4. **Markdown rendering** -- Prose blocks are parsed via pulldown-cmark and converted to genpdf elements. Nushell blocks render as labeled code blocks. Other directive types render as labeled directive blocks.

See `docs/plans/guide-evolution.md` for full architecture details.
