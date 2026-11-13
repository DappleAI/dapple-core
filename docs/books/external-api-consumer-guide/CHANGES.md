# Changes

> **:robot: Agent Note:** Skip this chapter. Changes documentation is for human readers tracking version-to-version differences. Agents should read the actual content chapters directly.

This chapter helps human readers understand what changed between versions so you don't have to reread the entire guide. Each version links to the new or updated sections.

---

## [Version 0.3.1](#version-031) (Current)

**Released:** 2026-08-03

Author-set stage icons. Additive — existing clients that ignore the new field keep working.

### Changed

- **[Endpoint Reference → `GET /api/journeys/{id}/steps` and `/stages`](./endpoint-reference.md)** — rows now also carry **`stage_icon`** (`icon` on `/stages`): an ordered array of up to five author-set icon names, repeated per row like the other stage identity fields. It distinguishes sibling stages that share a `kind`. **Its vocabulary is open** — it belongs to whichever client draws it, so expect unrecognized names and fall back to your `kind` glyph; that is the normal state, not skew, and never an error. Absent and `[]` mean the same thing. If you do not draw icons, ignore the field.

---

## [Version 0.3.0](#version-030)

**Released:** 2026-08-03

Stage identity on the journey status endpoints. Additive — existing clients that ignore the new fields keep working.

### Changed

- **[Endpoint Reference → `GET /api/journeys/{id}/steps` and `/stages`](./endpoint-reference.md)** — rows now carry the owning stage's authored identity: `stage_name` / `stage_description` / optional `stage_kind` on `/steps` (repeated identically across a stage's rows; fold first-seen-wins), and `name` / `description` / optional `kind` on `/stages`. The kind is a closed, author-set vocabulary (`intake`, `fetch`, `analyze`, `decide`, `review`, `compare`, `assemble`, `deliver`) describing what kind of pane a stage is. Absent when the guide authored none; treat unknown or absent values as your neutral default, never an error.

---

## [Version 0.2.0](#version-020)

**Released:** 2026-06-06

Self-describing value slots on `/steps`, and queryable computed-output results on `/task_attempts`.

### Changed

- **[Endpoint Reference → `GET /api/journeys/{id}/steps`](./endpoint-reference.md)** — each `exposed_slots` entry now carries an explicit **`producer`** (`"operator"` vs `"computed"`) and, for operator slots, an **`input`** render spec (`control_type`, label/help/placeholder, `choices`). You can now render a generate→pick→refine wizard from `/steps` alone — no second join to `/brief-form`, and no rendering a `select` as a raw text box. `value_slot_id` is now populated for **computed** slots too; branch on `producer`, not on a missing id.
- **[Endpoint Reference → `GET /api/dispatches/{id}/task_attempts`](./endpoint-reference.md)** — each attempt now carries **`computed_outputs`** (`{ value_slot_id, name, outcome }`, where `outcome` ∈ `produced` | `source_path_missing` | `type_mismatch` | `unsupported_kind`), so you can tell a customer *why* a computed slot is stuck instead of leaving the chapter unexplained-pending. The authoring note was corrected: the surface drops the internal `context_metadata` and exposes only `context_data`, `signals`, and `computed_outputs`.

All changes are additive — existing clients that ignore the new fields keep working.

---

## [Version 0.1.0](#version-010)

**Released:** 2026-06-05

The first release of the External API Consumer Guide. It documents the V1 external surface (`external-api.dapple.x`, served by the `dapple-prompt-external-api` crate) for customer companies, previously documented only inside `crates/dapple-prompt-external-api/AGENTS.md`.

### What's Included

- **[Authentication](./getting-started/authentication.md)** — the Ed25519 signed-token → JWT exchange, JWT claims (`iss = external-api.dapple.x`, `token_use = external_service`, `sub = company UUID`, `principal_id`), token lifetime, onboarding, and revocation. Includes the canonical external-service-token reference.
- **[Core Concepts](./getting-started/concepts.md)** — per-company isolation, the journey vocabulary, the generate → pick → refine step model, and automatic execution.
- **[Endpoint Reference](./endpoint-reference.md)** — the full V1 surface (15 endpoints), with request/response field summaries, company scoping, the no-leak 404 semantics, and the read-only `task_attempts` endpoint.
- **[Guide Ingest](./guide-ingest.md)** — POST and PATCH upsert, the multipart `file` field, namespace gating, and the `refreshed` / `new_version` outcomes.
- **[Customer Runtime Flow](./runtime-operations.md)** — create journey → fetch briefs → submit bindings → automatic execution → poll status. No manual task driving.
- **[Error Handling](./error-handling.md)** — `401` / `403` / `404` (including the deliberate no-leak 404), `400 GUIDE_PLAYBOOK_MISMATCH`, and `503` for a missing signing key.

### Surface Posture Documented

- Cryptographic isolation from the internal surface.
- Per-company data isolation; no "list all" endpoints; hard-`'external'` scope.
- Automatic execution; journey completion is a UI signal, not a gate.
- Plain field naming (`id`, `playbook_id`, `journey_id`, `guide_id`, `value_slot_id`, `dispatch_id`, `brief_pack_ids`, `inference_profile_ids`).

---

## Future Versions

### Planned

- [ ] A thin external SDK client reference, once `dapple-external-sdk` lands.
- [ ] Per-namespace asset isolation for guide ingest (current V1 limitation: assets land in the dapple namespace).
- [ ] A `POST /api/companies` self-service onboarding endpoint, replacing the manual V1 flow.
