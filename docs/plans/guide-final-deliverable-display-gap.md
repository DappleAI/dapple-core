# Platform gap: guide final-deliverable display relies on an undocumented UI fallback

**Date:** 2026-06-17
**From:** Shaggy Dog Joke Builder investigation
**Status:** Needs a platform/UI decision before the guide-side issue can be closed
**Supersedes:** any earlier "just change the guide prompt" framing — that is now downgraded to symptom relief (see end)

## TL;DR

A guide's **final deliverable does not render through the brief system at all.** A computed
`:::brief-output` only *binds a value into a value-slot* — it never becomes a display unit, and
no UI path renders it. Operators see a deliverable only because of a **separate task-result
fallback envelope** in dapple-ui that dumps the producing task's raw `context_data`.

Therefore:

- The dapple-guides README contract ("name it + list it in the brief's `units` + expose the slot
  → it renders / reaches the operator UI") is **mechanically inaccurate**. Those steps bind the
  value; they do not display it.
- Every interactive guide (`shaggy-dog-joke-builder`, `assessing-risks`, `hype-vs-reality`, ...)
  depends on the task-result fallback. None displays via briefs.
- Deliverable presentation is **whatever `formatContextData` does to the raw task payload** —
  including dumping arrays-of-objects as raw JSON.

**Decision needed:** is the task-result envelope the sanctioned display path (-> document it,
rewrite the README contract, author task payloads for clean rendering), or should the platform
grow a real "display a bound value" brief unit (-> ingest + UI work)?

## Symptom

At the end of the Shaggy Dog journey the final screen showed:

    Results completed:
    ### Intro        ...
    ### Title        ...
    ### Closer       ...
    ### Authors Note ...
    ### Groan Factor ...
    ### Body Sections
    1. {"prose":"...","detour_title":"...","gags_in_play":[...]}
    2. {"prose":"...","detour_title":"...","gags_in_play":[...]}

Expected: one continuous prose story. The assemble step ran correctly — the problem is how the
deliverable is shaped and displayed.

## Backend DB observation (correct)

> The assembled story exists only as a brief_output. There is no display brief_unit for it —
> 0 rows across all 7 brief versions, while the brief_output exists in all 7.

This is designed behavior, not a missing row.

## Root cause — two independent render paths

### Path 1 — brief units (does NOT show deliverables)

A brief's `units: [...]` array mixes markdown / input / output names; at ingest they go to
different tables:

- `dapple-prompt/crates/dapple-prompt-api/src/handler/guide_ingest.rs:4061-4246`
  (`materialize_brief_system`). Output-named units branch at 4176-4208 ->
  `insert_brief_outputs_in_tx` (4244), and are deliberately not pushed onto the brief-unit
  vectors. Comment: "Output unit -- these are inserted via insert_brief_outputs, not
  insert_brief_units."
- `crates/dapple-data/migrations/00000000000008_brief_system.sql:33-63` — `rev.brief_units` and
  `rev.brief_outputs` are sibling tables. Brief-unit subtypes: markdown / notice / input / media.
  There is no `brief_unit_output` table.
- Display query never joins outputs:
  `crates/dapple-data/assets/.sql/briefs/select_brief_units_for_briefs.sql:29-31`.
- Vestigial: `crates/dapple-data/migrations/00000000000002_lookup_tables.sql:98-99` seeds an
  `'output'` brief_unit_type, but nothing wires it (no FK, no subtype; `InsertBriefUnitSubtypeArgs`
  in `crates/dapple-data/src/model/brief_units.rs:246-252` has no `Output` variant). Suggests a
  display-output unit was intended and never built.
- UI mirrors this: `dapple-ui/packages/ui/lib/briefEnvelopeGenerator.ts:262-323`
  (`buildBriefBlocks`) renders only input / markdown / notice; `BriefUnitView`
  (`types/internal-api/types.gen.ts`) is `{ input?, notice?, markdown? }`.

Verdict: a computed brief-output is purely a value binding. Listing it in `units` wires the
binding (the unwired-output guardrail `crates/dapple-parser/src/transform/mod.rs:711-734` — whose
tests use `land-the-worst-pun-brief` + `assembled-story-output` — only checks that wiring). It
contributes nothing renderable.

### Path 2 — task-result envelope (what actually shows the deliverable)

- `dapple-ui/packages/ui/lib/dispatchEnvelopeResolver.ts:562-599`: when the step's tasks
  complete, the resolver takes the last task attempt's raw `context_data` (unwrapping `nu_output`)
  and calls `generateTaskResultsEnvelope("Results", payload, dispatchId)` — independent of briefs.
- `dispatchEnvelopeGenerator.ts:53-81` renders "**Results** completed:" + `formatContextData(payload)`.
- `formatContextData` (`dispatchEnvelopeGenerator.ts:440-462`): single key -> value printed
  verbatim; multiple keys -> one `### Heading` per key; arrays of objects matching no heuristic ->
  `JSON.stringify(item)` (line 520). That is the dump above.

This fallback is the de facto deliverable display for all guides; it keys off the task payload,
not the brief.

## Reconciliation

- "No display brief_unit -> the story never appears" is correct for Path 1.
- The operator still saw the story (as a field dump) via Path 2, which the brief model doesn't
  account for. Both views are right; they describe different paths.

## Scope

Not guide-specific. `assessing-risks` and `hype-vs-reality` have structurally identical final
briefs (computed deliverable brief-output in `units`, no display unit) and rely on the same
fallback. `dapple-guides/README.md:705-732` tells authors the brief-output-in-`units` step makes
the value "reach the operator UI / render," which the ingest code contradicts. The outcome
(operator sees something) holds only because Path 2 exists.

## Decision needed

1. Is the task-result envelope the intended deliverable display path?
   - Yes -> document it as such; rewrite the README contract to stop attributing display to
     brief-output; give authors guidance to shape the final task payload for clean rendering
     (a single prose field renders cleanly; arrays-of-objects dump as JSON).
   - No -> build a real display unit: a `brief_unit_output` / `display` subtype on ingest (the
     `'output'` seed already exists) + a `BriefUnitView` display variant + a render branch in
     `buildBriefBlocks`, so deliverables present deterministically via briefs.

## Open questions

- Does Path 2 fire reliably for every guide's final step (multi-task steps; payloads containing a
  `choices` array)? Any shape where a completed journey shows nothing?
- What is brief-output's real role for display vs. the stage `done` rollup? The README claims
  that without it a completed journey "reads as still working" — does that come from exposed-slot
  / done gating, independent of Path 2 rendering?
- Was a display-output brief unit intended (per the vestigial `'output'` seed) and dropped? Why?

## Interim state (symptom relief only — NOT a resolution)

A guide-side prompt change is parked in
`dapple-guides/public/dapple/shaggy-dog-joke-builder/prompts/shaggy-dog-assemble-story/en.md`:
the assemble task now returns `{ "story": "<one continuous prose story>" }` instead of the
6-field object. Under Path 2 this renders as clean prose (single key -> verbatim, no headings, no
JSON). It is symptom relief riding the fallback, not a fix for the display gap, and it is on hold
pending the decision above. (`AGENTS.md` data-flow note updated to match.)
