# Downstream consumer impact — journey `guide_id` read-path + association POST responses

**Date:** 2026-05-29
**Scope:** API-observable changes from the data-layer fixes landed on `fluent-path-fixes`.
**TL;DR:** Both changes are **additive / corrective**. **No consumer is required to
change anything to keep working.** There are a couple of optional simplifications,
and one read-now-possible value (`journey.guide_id`).

---

## What changed server-side (the only two consumer-observable changes)

### 1. Journey GET responses now surface `guide_id` (and `predecessor_journey_id`)

A journey has always *stored* its pinned `guide_id` (Migration 44) and *returned*
it on the **POST** response. But the **read** path was half-built: every journey
`SELECT` dropped the column and the fold hardcoded `None`, so `guide_id` /
`predecessor_journey_id` were **never present** in any GET/list response (the DTO
uses `#[serde(skip_serializing_if = "Option::is_none")]`, so the keys were simply
absent). This is now completed: the journey read path returns the real pin.

- **Endpoints affected (internal `api.dapple.x` AND external `external-api.dapple.x`):**
  - `GET /api/journeys` (list)
  - `GET /api/journeys/{id}` (single)
- **Wire effect:** for a pinned journey, the JSON now includes
  `"guide_id": "<uuid>"` (and `"predecessor_journey_id": "<uuid>"` for chained
  journeys). For unpinned/legacy journeys the keys remain absent (unchanged).
- **Breaking?** No. Purely additive — a previously-absent optional key now appears.
  Any tolerant JSON/DTO parser is unaffected.

### 2. Association POST endpoints now return their full child set

Four "create with children" endpoints were silently returning an **empty/partial**
child list in the POST response (a Postgres data-modifying-CTE visibility bug — the
rows were *persisted* correctly, just omitted from the immediate response). Fixed:

- `POST /api/steps` → `step.tasks` now populated
- `POST /api/stages` → `stage.steps` now populated
- `POST /api/prompts` → prompt↔fluent-message links now populated
- `POST /api/tasks` → task↔prompt links now populated
- **Tier:** these are **internal-only** (`dapple-prompt-api`). The external surface
  exposes none of them.
- **Breaking?** No. The data was always persisted; a follow-up `GET` already
  returned the children correctly. Only the POST *response body* was incomplete.

---

## dapple-ui packages

**Required changes: none.**

Optional / opportunities:
- **Journey views:** `GET /api/journeys` and `GET /api/journeys/{id}` now include
  `guide_id`. If the UI wants to show "pinned to guide version X", drive b-side /
  experiment display, or cache-key by guide version, it can now read
  `journey.guide_id` straight from the read response instead of tracking it
  client-side from the create call. `predecessor_journey_id` is likewise available
  for chain visualization.
- **Builder/editor flows that POST `/api/steps` / `/api/stages` / `/api/playbooks`
  / `/api/tasks` / `/api/prompts` directly:** if any flow previously worked around
  the empty-children response by issuing a follow-up `GET` (or by ignoring the
  returned children), it can now trust the POST response directly and drop the
  re-fetch. Verify any code that asserted/relied on the response children being
  empty — that was the bug, and such code should be updated to expect the real set.

No DTO/schema regeneration is strictly required (the OpenAPI types are unchanged;
only previously-omitted optional fields now appear), but regenerating the client
types is harmless and will document `guide_id` as a now-populated field.

---

## dapple-suite

**Required changes: none.**

Same two opportunities as dapple-ui:
- If dapple-suite reads journeys it created (chained guides, experiment routing),
  it can now read `journey.guide_id` / `predecessor_journey_id` from GET responses.
  This is especially relevant for chained-journey suites: the chain pin is now
  visible on read, not just inferable from the create payload.
- If dapple-suite authors playbooks/steps/tasks via the internal REST association
  endpoints (rather than guide ingest), the POST responses are now complete — any
  re-GET workaround can be removed.

**Guide authors note (applies to dapple-suite + external authors):** stage- and
step-level **flow-control directives** (skip/fail/retry/loop/branch) are a *known,
not-yet-implemented* gap — the runtime evaluator exists but nothing loads/populates
stage/step directives yet, so they are currently a no-op. Do not author guides that
depend on stage/step directive behavior; it will silently do nothing until the
wiring lands.

---

## External API consumers (e.g. acme, `external-api.dapple.x`)

**Required changes: none.**

- `GET /api/journeys` and `GET /api/journeys/{id}` now include `guide_id` (and
  `predecessor_journey_id` for chained journeys) when the journey is pinned. This
  lets a customer confirm *which guide version* a journey is running against —
  directly useful for the b-side / experiment story (the external contract already
  expects experiment-aware clients to send `guide_id` on create; they can now read
  it back). Additive, non-breaking.
- The association POST endpoints (steps/stages/prompts/tasks) are **not exposed**
  on the external surface, so the response-completeness fix has no external impact.

**Related, unchanged (not part of this work — listed only so the picture is
complete):** `POST /api/journeys` already requires a resolvable pin — supply
`guide_id` explicitly, or omit it only when a guide exposes the target playbook
(else `400 NO_GUIDE_FOR_PLAYBOOK`). That contract predates these fixes; nothing
here changes it. Customers already sending `guide_id` on create are unaffected.

---

## Verification reference

Server-side suites are green after the change: `user_lifecycle` 46/46,
`external_lifecycle` 108/108 (exercises external journey GET/list), `journey_chain_test`
22/22, `guide_ingest_test` 33/33, `guide-lifecycle` 39/39, `dapple-data` all.
A pinned journey now round-trips its `guide_id` through create→GET (previously the
GET dropped it), which is what `external_lifecycle` and `user_lifecycle` now assert.
