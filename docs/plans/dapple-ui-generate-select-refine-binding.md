# Guides-side companion → dapple-ui `/steps` authoring contract

> **Audience:** guide authors (dapple-guides / dapple-prompt) + anyone syncing the
> generate→pick→refine work.
> **Authoritative UI contract:** `dapple-ui/docs/steps-authoring-notes.md`
> (companions: `stage-completion-overlays-handoff.md`,
> `value-slots-architecture-handoff.md`). **When this doc and that one disagree,
> theirs wins** — dapple-ui owns the `/steps` wire format and the UI behavior.

This file records what the **guides** do to honor that contract, plus the open
items between us. It supersedes the earlier draft of this doc (which predated the
`/steps` model and incorrectly described the pick as a new dispatch intent and the
binding endpoint as `/api/dispatch_intent_bindings`).

## What the guides author (to honor the contract)

For each generate→pick→refine chapter, **three steps in order: describe → generate
→ refine** (generate is always authored before refine — the UI scans for the first
not-done step's first unbound exposed slot):

- **describe step** exposes its brief-input slot via `expose` on the step. The slot
  is collected via `/brief-form`, so the describe task gates until the operator
  submits input. **The describe prompt must interpolate that slot as
  `{{<input_slot>}}`** or the task runs blind (missing Handlebars vars render empty).
- **generate step** exposes nothing → `done: true` → auto-runs after describe. Its
  task returns options as `{ payload: { choices: [{label, description, value}] },
  signals: {} }`.
- **refine step** exposes **exactly one bare value-slot** (the pick target, no
  brief-input) via `expose: ["<chapter>_selected_*"]`. The refine prompt consumes it
  as a top-level `{{<chapter>_selected_*}}`, and the refine task pipes `$in` so the
  bound value is in context.
- **Pick slots are `kind: "text"`** and each choice `value` is a **text** payload —
  matching the slot's `value_kind`. Every guide's pick is a prose direction /
  strategy / option, never a structured object, so text is correct across all 5.

## Confirmed by the dapple-ui docs (no longer open)

- **expose lives on the step** (per-step gating); all 5 guides are normalized to this.
- A **pick is a binding to the refine step's bare exposed slot**, not a new dispatch
  intent. The unbound exposed slot *is* the gate; the refine dispatch idles until
  bound, then auto-runs. Whoever fills it — UI tile-click **or** an external API
  consumer (possibly hours/days later) — writes the binding.
- **Discovery is `GET /api/journeys/{id}/steps`** (per-step `done` + `exposed_slots[]`
  with `value_slot_id`, `name`, `value_kind`, `bound`). Not `/brief-form` —
  `/brief-form` carries only authored operator inputs and is overlay-filtered.
- **Binding writes are typed:** the writer picks the `*_items` Vec by the slot's
  `value_kind` (fetched separately from `/api/value-slots?query=by_object_ids&ids=…`),
  and POSTs to `/api/dispatch-intent-bindings`. Guides don't author this; it's the
  filler's job. We just declare the bare slot and consume it.

## Open items (guides ↔ dapple-ui / backend)

1. **`payload.choices` vs `context_data.choices`.** Our generate tasks return
   `{ payload: { choices }, signals }`. The UI reads choices from the task's
   `context_data` under `choices` (top-level **or** nested under `nu_output`; keys
   checked in order: `choices`, `topics`, `items`, `options`). **Confirm the nu task
   `payload` surfaces where the fetcher looks** so tiles render with no per-guide UI
   work. This is the one thing a single pilot ingest settles.
2. **One brief-pack per stage / stage-order alignment.** Our chapters put briefs only
   on the describe (first) stage; generate/refine carry none. Confirm this satisfies
   the brief-pack↔stage ordinal mapping (should be fine — a single input stage, first
   in order, one pack).

## Reference example (the pilot)

`hype-vs-reality` / `evidence-architecture`:
- **describe step** exposes `evidence_architecture_define_claims_and_research_constraints`
  (brief input); the define prompt interpolates `{{…}}` and emits
  `claims_to_investigate`, `access_level`, `time_constraints`, `confidence_requirements`.
- **generate step** exposes nothing; returns `choices` (each `value` is text).
- **refine step** exposes the bare `evidence_architecture_selected_evidence_strategy`;
  the prompt consumes `{{evidence_architecture_selected_evidence_strategy}}` plus the
  upstream keys above.
