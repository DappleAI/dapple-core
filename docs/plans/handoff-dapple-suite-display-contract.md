# Handoff → dapple-suite: typed brief-output display contract

> **Audience:** the dapple-suite runner team (and anyone rendering journeys off
> the internal API / `dapple-sdk`).
> **Status:** landed in `dapple-prompt` (uncommitted on branch
> `image-generation` at time of writing). Backend + SDK + external surface all
> carry it; verified by unit, serde, and DB-backed lifecycle tests.
> **Why you're reading this:** computed brief-outputs (the structured
> json/asset values a guide produces — image, find-list, shot list, etc.) now
> ship a **typed, authored instruction for how to render them**. You no longer
> guess per-slot. This is the contract.

## TL;DR

1. A computed slot now carries a closed-enum **`display`** hint (plus `reveal`
   and `title`). Render the bound value according to `display.kind`.
2. The hint lives on the **cached, structural** surface you already poll
   (`/steps` `ExposedSlot.display`). It is static and version-stable.
3. The **value** does *not* ride that DTO. It stays on the **live bindings
   read**, and you join it to the slot by **`value_slot_id`** — the exact join
   you already do for operator inputs. (It has always been there and typed; this
   is the half the earlier handoff under-documented.)

So rendering a computed slot is two reads joined on `value_slot_id`: the
**structure+hint** (cached) and the **value** (live).

---

## 1. The vocabulary

`BriefOutputDisplay` — a **closed** enum (`dapple_types::model::briefs`,
re-exported by the SDK). Wire form is `snake_case`:

| `display.kind` | render as | value shape it expects |
|---|---|---|
| `image` | `<img>` from the asset content endpoint | `value_kind: asset` |
| `text` (default) | plain text | scalar |
| `markdown` | rendered markdown | string |
| `list` | bulleted list | `string[]` (json) |
| `checklist` | tickable list (ticks are local UI only, never posted) | `string[]` (json) |
| `table` | columns = keys, one row per item | `object[]` of uniform scalar fields (json) |
| `fields` | label/value pairs | `object {k: v}` (json) |
| `hidden` | **do not render** — data-flow slot only | n/a |

Because it is a real closed enum, your schema codegen emits a closed union; an
unrecognized value is a backend/version-skew bug, not something to guess around.
The default for an output with no authored `display` is `text`.

Two modifiers travel with it:

- **`reveal: bool`** — collapse the block behind a "Reveal" spoiler toggle
  (e.g. a puzzle solution). Default `false`.
- **`title`** — optional heading label for the block.

`hidden` is the typed opt-out: a computed slot that exists only so a value flows
between steps/stages (it must ride a `:::brief-output` to be produced and bound)
but is never shown to the operator. Skip it in the UI.

---

## 2. Where the hint appears (cached, value-free)

### `GET /api/journeys/{id}/steps` → `ExposedSlot.display`  ← your primary surface

`ExposedSlot` (`dapple_types::model::journeys`) is what you already read to learn
which sub-interaction a step is waiting on. It gained one field:

```jsonc
{
  "value_slot_id": "…",          // the journey-pinned bind target / join key
  "name": "puzzle_image",
  "value_kind": "asset",
  "bound": false,                 // live: has the value been produced yet?
  "producer": "computed",         // discriminator: "operator" | "computed"
  "input": null,                  // render spec for OPERATOR slots (control_type, label, choices, …)
  "display": {                    // render spec for COMPUTED slots (NEW)
    "kind": "image",
    "title": "Your spread",
    "reveal": false
  }
}
```

- `display` is **`Some` only for computed slots** (`producer == "computed"`).
  Operator slots carry `input` and `display: null`.
- It is a **static authored render hint**, hash-folded into the brief version. It
  is safe to cache. **It never carries the value.**

### `BriefOutput.display` (on the full brief structure)

The same four fields (`display`, `reveal`, `title_text`,
`title_fluent_message_id`) are on `BriefOutput` if you read full briefs. For the
runner, `/steps` `ExposedSlot.display` is the surface you want — it is per-slot
and already in your poll loop.

---

## 3. Where the value comes from (live, joined by `value_slot_id`)

The resolved value has always come through typed — read it from the **effective
bindings** for the journey, never from the structural DTO.

```
GET /api/dispatch-intent-bindings?query=effective_by_journey_id&journey_id={journey_id}
→ { "objs": [ DispatchIntentBinding, … ] }   // one per value_slot_id, chain-effective, latest-wins
```

`DispatchIntentBinding` (`dapple_types::model::dispatch_intent_bindings`):

```jsonc
{
  "dispatch_id": "…",
  "value_slot_id": "…",            // ← JOIN KEY back to the ExposedSlot
  "value_slot_name": "puzzle_image",
  "value_kind": "asset",
  "text_items":   [{ "id": "…", "value": "…" }],
  "int_items":    [{ "id": "…", "value": 0 }],
  "number_items": [{ "id": "…", "value": "0" }],   // Decimal as string
  "bool_items":   [{ "id": "…", "value": true }],
  "json_items":   [{ "id": "…", "value": <any json> }],
  "asset_items":  [{ "id": "…", "asset_id": "…" }],
  "value_meta":   [{ "id": "…", "ui_spec": {…}, "producer": "computed" }]
}
```

Read the value out of the vector matching `value_kind`:

| `value_kind` | read | then |
|---|---|---|
| `text` | `text_items[0].value` | string |
| `int` | `int_items[0].value` | i64 |
| `number` | `number_items[0].value` | decimal (string) |
| `bool` | `bool_items[0].value` | bool |
| `json` | `json_items[0].value` | the array/object — render per `display.kind` |
| `asset` | `asset_items[0].asset_id` | then `GET /api/assets/{asset_id}/content` for bytes |

(For `multi`-cardinality slots the vector holds N items; for `single` it holds
one. Computed display outputs are `single`.)

> Per-dispatch alternative: `GET /api/dispatches/{id}/briefs` exposes the same
> values scoped to one dispatch. The `effective_by_journey_id` read above is the
> journey-wide, order-independent, latest-wins resolver and is the one to default
> to for rendering a journey's current state.

---

## 4. The render algorithm (per computed slot)

```
for slot in /steps[].exposed_slots where slot.producer == "computed":
    if slot.display.kind == "hidden":           # data-flow only
        continue                                # never render
    render the block frame (slot.display.title, slot.display.reveal)
    if not slot.bound:
        show a waiting / progress state         # server is still producing it
        continue
    binding = effective_bindings[value_slot_id == slot.value_slot_id]
    value   = extract(binding, slot.value_kind) # table in §3
    render value according to slot.display.kind  # table in §1
```

Operator slots (`producer == "operator"`) are unchanged: render the control from
`slot.input`, POST a `dispatch-intent-binding`. `display` is for the computed
half only.

---

## 5. Worked example (the image-guide that triggered this)

A guide's render step produces these computed outputs. The author tags each with
a `display`; you map `kind → renderer` with zero guide-specific knowledge:

| slot | `value_kind` | `display.kind` | modifiers | you render |
|---|---|---|---|---|
| `puzzle_image` | asset | `image` | `title: "Your spread"` | `<img src=/api/assets/{asset_id}/content>` |
| `find_list` | json | `checklist` | `title: "Find these"` | tickable list from `string[]` |
| `solution` | json | `table` | `reveal`, `title: "Solution"` | table behind a spoiler |
| `shotlist` | json | `hidden` | — | nothing (render input for the step) |
| `shotlist_overview` | json | `markdown` | `title: "Shot list"` | rendered markdown |
| `shotlist_shots` | json | `table` | `title: "Shots"` | table |

Note `shotlist` is `hidden`: the structured plan crosses a stage boundary as a
binding but is never shown; the author flattened the human-readable view into the
sibling `markdown`/`table` slots. You do not infer any of this — every slot you
display tells you its `kind` explicitly.

---

## 6. SDK quick reference (`dapple-sdk`)

Same `dapple-types` structs, so the field names above are identical in Rust.

- `client.journeys().steps(journey_id)` → `Vec<JourneyStepStatus>` → each
  `.exposed_slots: Vec<ExposedSlot>` with `.display: Option<ExposedSlotDisplay>`.
- `client.dispatch_intent_bindings()` with the `effective_by_journey_id` query →
  `Vec<DispatchIntentBinding>`.
- `client.assets()` content endpoint for asset bytes.
- Types: `BriefOutputDisplay`, `ExposedSlotDisplay { kind, title, reveal }`,
  `ExposedSlot`, `DispatchIntentBinding`, `BindingJsonItem { id, value }`,
  `BindingAssetItem { id, asset_id }`.

(The SDK no longer generates types from an OpenAPI snapshot; it depends on the
shared `dapple-types` crate, so client and server cannot drift. The stale
`openapi/internal-api.json` snapshot was removed.)

---

## 7. What did NOT change / gotchas

- **The value wire is unchanged.** `DispatchIntentBinding` already carried typed
  items. Nothing new to read there; just join by `value_slot_id`.
- **No new endpoint.** Everything above is on surfaces you already poll.
- **Do not expect the value on `ExposedSlot`/`BriefOutput`.** Live per-dispatch
  values are deliberately kept off cached/structural DTOs (caching them would
  poison the cache). Hint on the structure, value on the bindings read — joined
  by `value_slot_id`.
- **`bound: false` + `display` present** means "produced-by-the-server, not ready
  yet" — show progress, don't try to bind it. If a computed slot stays unbound,
  check `GET /api/dispatches/{id}/task_attempts` (`computed_outputs[].outcome`)
  to see whether the producer ran but couldn't extract it (`source_path_missing`
  / `type_mismatch`) versus still working.

---

## 8. Code + test pointers (in `dapple-prompt`)

- Enum + DTO: `crates/dapple-types/src/model/briefs.rs` (`BriefOutputDisplay`,
  `BriefOutput`), `crates/dapple-types/src/model/journeys.rs`
  (`ExposedSlot.display`, `ExposedSlotDisplay`).
- Data-layer fold that builds `display` for computed slots:
  `crates/dapple-data/src/query/journeys.rs`
  (`select_step_binding_status_for_journey`).
- Bindings read DTO: `crates/dapple-types/src/model/dispatch_intent_bindings.rs`;
  query variants: `crates/dapple-prompt-api/src/schema/dispatch_intent_bindings.rs`.
- Authoring/materialization + hash fold: `crates/dapple-prompt-api/src/handler/guide_ingest.rs`.
- Contract tests to read for the exact shapes:
  - SDK serde: `crates/dapple-sdk/src/types.rs` (`display_contract_tests`).
  - External wire round-trip: `crates/dapple-prompt-external-api/tests/external_lifecycle.rs`
    (`computed_output_reaches_done_after_dispatch`).
  - DB-backed ingest/persist/hash:
    `crates/dapple-prompt-api/tests/guide_ingest_test.rs`
    (`test_brief_output_display_*`).
- Design rationale + rejected alternatives: the Part C/D sections of the
  approved plan (`so-i-believe-we-serene-fiddle.md`).
