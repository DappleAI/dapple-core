# Changes

> **:robot: Agent Note:** Skip this chapter. Changes documentation is for human readers tracking version-to-version differences. Agents should read the actual content chapters directly.

This chapter helps human readers understand what changed between versions so you don't have to reread the entire guide. Each version links to the new or updated sections.

---

## [Version 0.4.1](#version-041) (Current)

**Released:** 2026-08-03

Author-set stage icons. Additive — existing clients that ignore the new field keep working, and a client that ignores it renders exactly what it renders today.

### Changed

- **[Endpoint Reference](./endpoint-reference.md)** — `GET /api/journeys/{id}/steps` rows now also carry **`stage_icon`**, and `GET /api/journeys/{id}/stages` entries carry the same as **`icon`**: an ordered array of up to five author-set icon names (repeated per row like the other stage identity fields). It distinguishes sibling stages that share a `kind` — a fifteen-stage guide is often nine `analyze` panes, which `kind` alone cannot tell apart. **The vocabulary is open**, unlike `kind`: it belongs to whichever client draws it, so expect names you do not recognize and degrade to your `kind` glyph on anything unfamiliar — an unknown name is the normal state, not skew, and never an error. Absent and `[]` mean the same thing. If you do not draw icons, ignore the field.

---

## [Version 0.4.0](#version-040)

**Released:** 2026-08-03

Stage identity on the journey status endpoints. Additive — existing clients that ignore the new fields keep working.

### Changed

- **[Endpoint Reference](./endpoint-reference.md)** — `GET /api/journeys/{id}/steps` rows now carry the owning stage's authored identity: **`stage_name`**, **`stage_description`**, and optional **`stage_kind`** (repeated identically across a stage's rows; fold first-seen-wins). `GET /api/journeys/{id}/stages` entries carry the same as **`name`** / **`description`** / **`kind`**. `kind`/`stage_kind` is a closed, author-set vocabulary (`intake`, `fetch`, `analyze`, `decide`, `review`, `compare`, `assemble`, `deliver`) describing what kind of pane the stage is — semantic, not pictorial. Absent means the stage authored no kind; treat unknown or absent values as your neutral default, never an error. Guides ingested before their authors add `kind:` serve it absent until re-ingest.

---

## [Version 0.3.0](#version-030)

**Released:** 2026-06-06

Self-describing value slots on `/steps`. Additive — existing clients that ignore the new fields keep working.

### Changed

- **[Core Concepts](./getting-started/concepts.md)** and **[Endpoint Reference](./endpoint-reference.md)** — each `exposed_slots` entry on `GET /api/journeys/{id}/steps` now carries an explicit **`producer`** (`"operator"` vs `"computed"`) and, for operator slots, an **`input`** render spec (`control_type`, label/help/placeholder, `choices`). Render operator controls straight from `/steps`; show a waiting state for `"computed"` slots and don't bind them. `value_slot_id` is now populated for computed slots too — branch on `producer`, not on a missing id.

---

## [Version 0.2.0](#version-020)

**Released:** 2026-06-05

This release reconciles the guide with the platform's step-based dispatch model, automatic execution, the new journey status endpoints, the required `guide_id` on journey creation, the plain field-naming convention, and the removal of shared content.

### Changed

**Dispatch model corrected to step-based ([endpoint-reference.md](./endpoint-reference.md), runtime docs):**
- A dispatch is a batch of one-or-more steps. The create body is a flat `steps[]` (each entry carries `stage_step_id` / `step_task_id`); there is no top-level `stages` array.
- Documented **automatic execution**: a dispatch runs on its own once it is `dispatch_ready` (every overlay-exposed slot it needs is bound). Journey completion is a UI signal, not an execution gate.

**Journey status endpoints documented ([endpoint-reference.md](./endpoint-reference.md), [concepts.md](./getting-started/concepts.md)):**
- Added `GET /api/journeys/{id}/steps` (per-step `done` + `exposed_slots[].bound`) and `GET /api/journeys/{id}/stages` (per-stage `done` roll-up).
- Added a "Stages, steps, and generate → pick → refine" section explaining how consumers observe and drive these sub-interactions.

**Journey creation now requires `guide_id` ([intro.md](./getting-started/intro.md), [endpoint-reference.md](./endpoint-reference.md)):**
- `POST /api/journeys/` requires both `guide_id` and `playbook_id` (specific version ids); the backend validates the pair and returns 400 `GUIDE_PLAYBOOK_MISMATCH` on a mismatch.

**Field-naming refresh (request/response examples across the guide):**
- `object_id` → `id`, `path_object_id` → `path_id`, `*_object_ids` → `*_ids` (`brief_pack_ids`, `inference_profile_ids`, `task_ids`, `playbook_ids`, `prompt_ids`).
- Query-variant names (`by_object_ids`, `by_path_ids`) and the SDK `list_by_object_ids` method are unchanged — those are query identifiers, not fields.

### Removed

**Manual task-attempt driving examples ([endpoint-reference.md](./endpoint-reference.md)):**
- The internal `/api/dispatches/{id}/task_attempt/` path (and its `events` sub-path) is now labeled internal-only and optional — execution is automatic; observe progress via the journey status endpoints instead.

**Shared content reconciled as removed/deferred ([shared-content.md](./shared-content.md), [sdk-overview.md](./sdk-overview.md), [AGENTS.md](./AGENTS.md)):**
- `POST /api/shared-content/ingest`, the SDK `.shared_content()` client, and supporting tables no longer exist. The chapter is reduced to a deferred-to-V2+ stub and inbound links were dropped or relabeled.

### Quick Links for 0.2.0

1. [Core Concepts - Stages, steps, and generate → pick → refine](./getting-started/concepts.md#stages-steps-and-generate--pick--refine)
2. [Endpoint Reference - Journey Management](./endpoint-reference.md#journey-management)
3. [Introduction - Start a Journey](./getting-started/intro.md#4-start-a-journey)

---

## [Version 0.1.0](#version-010)

**Released:** 2026-02-19

This is the first versioned release of the API Consumer Guide. It aligns response schemas with the actual platform implementation following the Guide Lifecycle Audit.

### What Changed

**Ingest Response updated ([ingest-api.md](./ingest-api.md)):**
- `derived_objects` field names corrected to match the actual `DerivedObjectRefs` schema: `playbook_object_id`, `task_object_ids`, `brief_pack_object_ids`, `inference_profile_object_ids`, `prompt_object_ids`
- New `prompt_object_ids` field documents prompts materialized during ingest
- Dry-run response `summary` fields corrected to match `IngestSummary` struct: `derived_playbook_stages`, `derived_brief_packs`, `derived_tasks`, `derived_inference_profiles`, `deprecation_level`, `deprecation_reason`

**Version tracking added:**
- Version banner added to [Preface](./preface.md)
- This CHANGES.md file created for version history tracking

### Quick Links for 0.1.0

1. [Ingest API - Ingest Response](./ingest-api.md#ingest-response) -- Corrected response schema
2. [Ingest API - Dry Run Validation](./ingest-api.md#dry-run-validation) -- Corrected summary fields

---

## Future Versions

### Planned

- [ ] Endpoint reference documentation for new resource types
- [ ] Webhook integration documentation
- [ ] Rate limiting and quota management guide
