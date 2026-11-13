# Handoff → suite team: multi-select slots, dynamic candidate sources, authored step/stage ordering

## Context

This is a write-up for the dapple-suite team. The dapple-prompt backend (branch
`multi-select-slots`) added **multi-value ("keep-N") value slots**, **dynamic
candidate-source choices** for operator picks, and now returns **`/steps` and
`/stages` in authored order** (stored ordinal). A suite built against the
pre-branch SDK keeps working (all new response fields are serde-default) but
silently ignores the new data — multi renders as single, dynamic choices come up
empty. The suite must rebuild against this branch and implement the new
behavior. A separate launch-flow point (guide↔playbook pinning) is included
because it is the most likely cause of "journey ran a stale playbook."

## Where the changes live / status

- **Branch `multi-select-slots`** (dapple-prompt), 3 commits:
  `e07abff7 multi-value select input bindings`,
  `57a3f034 More complete bindings hashing for guide versioning`,
  `1a533364 Fix ordering issues on patch ingestion`.
- **In-flight on the same branch (uncommitted):** stored authored ordinals
  (Migration 53) → `/steps` and `/stages` return authored order.
- **Action:** rebuild the suite against the updated `dapple-sdk` + `dapple-types`
  from this branch.

## Backward compatibility

All new response fields are `#[serde(default)]` / `skip_serializing_if`. An
un-rebuilt suite will NOT crash — it just won't see/use the new fields. So the
rebuild is required to *gain* multi-select + candidate-source behavior, not to
avoid a break.

## 1. Multi-value ("keep-N") slots

New fields on `ExposedSlot` (`/journeys/{id}/steps`), `InputView`
(`/brief-form`), and the `ValueSlot` DTO:
`cardinality` (`"single"` | `"multi"`), `min_selections`, `max_selections`.

- `single` (default) = exactly **1** item.
- `multi` = keep a subset; item count must be within `[min_selections,
  max_selections]`. NULL bounds = no floor / no ceiling; `min = 0` permits a
  deliberate keep-none.
- **Control:** multi slots surface `control_type = "checkbox"`. Enforce min/max
  client-side too.
- **Submission contract** (`POST /dispatch-intent-bindings`): a binding carries
  N typed items of the slot's `value_kind`. The server validates with
  `validate_binding_cardinality` (`dapple-data/src/query/dispatch_intent_bindings.rs`):
  `single` → exactly 1 item; `multi` → `min ≤ N ≤ max`, else **HTTP 400**. So for
  a multi slot the suite submits the kept subset (the binding's items array
  carries >1 item).

## 2. Dynamic candidate source (generate → pick tiles)

New fields on `ExposedSlotInput` (steps) and `InputView` (brief-form):
`candidate_source_value_slot_id`, `candidate_source_name`.

When set, the operator control's options are **not** the static `choices` — they
come from the **bound value of the named computed slot** (an array of
`{label, description, value}` tiles produced by the generate step).

Suite flow for such a control:
1. Read `candidate_source_value_slot_id` (or `candidate_source_name`).
2. Find the **computed** entry in the same journey's `exposed_slots` whose
   `value_slot_id` matches (its `producer == "computed"`).
3. Read that computed slot's bound value (the tiles) and render them as the
   choices.
4. Operator keeps a subset honoring `cardinality` / `min` / `max`.
5. Submit the kept tiles' values as the binding (per §1).

The computed source slot is produced by the server after the generate step's
task succeeds; until then it is unbound — show a waiting state, do not bind it.

## 3. Authored ordering of `/steps` and `/stages`

Both endpoints now return rows in **authored order** via a stored ordinal.
**Iterate in the returned order; do NOT re-sort by `stage_step_id` / UUID.**
Re-sorting by id is exactly the latent bug that stranded a repaired step after a
re-ingest re-mint. (`/steps` is the fine-grained "which sub-interaction is
awaiting input" signal; `/stages` is the per-pane roll-up.)

Note on pick placement (by design, NOT a bug): a stage's operator *pick* may be
exposed on a *later* step/pane than the step that generated its candidates
(offset generate→pick pipeline). The suite should render whatever slots `/steps`
reports per step, in order, and not assume a pick lives on the same pane as its
candidate generation.

## 4. Guide ↔ playbook pinning at launch (verify your flow)

`POST /journeys` requires **both** `guide_id` and `playbook_id` as explicit
version pins. The backend does **not** auto-resolve "latest"; it only validates
the pair (`guide_exposes_playbook`). There is intentionally no
`select_latest_guide_id_for_playbook`.

- **Observed symptom:** a journey created today ran a 2-day-old playbook
  structure even though the newest guide version was passed.
- **What the suite must do at launch:** fetch the *current* guide version
  (canonical pin / latest for the path) **and the playbook it exposes**, and pass
  exactly that pair — never a `(guide_id, playbook_id)` cached from an earlier
  browse or ingest. After a guide re-ingest, re-resolve both.
- **Honest caveat (open on our side):** we are still auditing backend
  guide↔playbook link maintenance across re-ingests — a guide version is
  currently linked to more playbook versions than it should be. If a rebuilt
  suite doing a fresh browse STILL pins an old playbook, that is on the backend;
  send us the journey id and we'll fix it there. Do not treat this section as
  "the backend is verified clean."

## 5. Exact DTO field additions (for client regen)

- `ExposedSlot` (`dapple-types/src/model/journeys.rs`): `+ cardinality,
  min_selections, max_selections` (alongside existing `producer`, `input`).
- `ExposedSlotInput` (same file): `+ candidate_source_value_slot_id,
  candidate_source_name`.
- `InputView` (`dapple-types/src/model/journey_brief_form.rs`): `+ cardinality,
  min_selections, max_selections, candidate_source_value_slot_id,
  candidate_source_name`.
- `ValueSlot` (`dapple-types/src/model/value_slots.rs`): `+ cardinality,
  min_selections, max_selections`.
- `CreateValueSlotSchema` / `PatchValueSlotSchema`
  (`dapple-prompt-api/src/schema/value_slots.rs`): `+ cardinality`
  (default `"single"`), `min_selections`, `max_selections`.

## Verification (suite side)

- Rebuild against `multi-select-slots`; confirm the new fields appear in the
  generated client.
- **Multi:** render checkbox, enforce min/max, submit the kept subset; confirm a
  binding with an in-range item count succeeds and an out-of-range count returns
  400.
- **Candidate source:** confirm an operator pick populates its options from the
  computed slot's tiles (not from static `choices`).
- **Ordering:** confirm `/steps` and `/stages` render in authored order without
  client-side re-sorting.
- **Launch:** confirm a fresh journey pins the newest guide + the playbook that
  guide exposes; report the journey id if it pins an older playbook.
