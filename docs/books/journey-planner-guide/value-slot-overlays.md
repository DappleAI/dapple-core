# Value-Slot Overlays

A **value-slot overlay** is how a stage (or step) declares *which value slots it
needs*. You author overlays with `expose` (and, rarely, `mask`) attributes on
`:::stage` / `:::step` blocks. They are the thing that makes the platform able
to answer **"is this stage done?"** — and they shape what an operator sees and
what the agent sees.

> **If your guide has stages but no overlays, it has no per-stage gating.** The
> platform treats every stage as immediately complete and shows operators the
> full, unfiltered form. That is the state most existing guides are in today.
> This page is how you fix them.

## Why overlays matter (three things they drive)

A stage's exposed slots are used, in lockstep, by three surfaces:

1. **Completion / gating.** A stage is *done* only when every value slot it
   exposes is bound (answered). `GET /api/journeys/{id}/stages` reports
   `done` per stage from exactly this set. With no overlays the exposed set is
   empty, so the stage is "done" the instant it starts — no gating.
2. **The operator form.** `GET /api/journeys/{id}/brief-form` shows only the
   inputs whose value slot the stage exposes (notice/markdown blocks always
   show). With no overlays the form falls back to showing *everything* — no
   focus, and inconsistent with a stage that already reports "done".
3. **The conduit's binding context (blinders).** When a task's prompt renders,
   the `# Dispatch Context` system message lists the journey's bound value
   slots — and overlays are the blinders on that list. If the task's
   stage→step→task cascade carries *any* overlays, the model is shown only the
   overlay-exposed slots (their values, and the assets/brief values derived
   from them); everything else in the journey pool is withheld from the
   prompt. A node with **no** overlays anywhere on its cascade keeps the full,
   unfiltered pool — blinders are opt-in per node, exactly like gating.

   Two boundaries to know: the *engine* is never blinded (a task script still
   reads every journey binding via `$in.bindings.*` — see "Binding read
   availability" in the flow crate docs), and whatever your own task script
   pipes into `prompt send` is your responsibility — blinders act on the
   platform-injected context, not on text you assemble yourself.

So overlays are not bookkeeping — they're the contract that says "this stage
needs these answers before it's finished, and these are the inputs to collect."

## Authoring overlays

Add `expose` to the stage (or step) — a list of `:::value_slot` names:

```
:::value_slot { name: "topic", kind: "text" }
:::

:::value_slot { name: "audience", kind: "text" }
:::

:::stage { name: "Draft", expose: ["topic", "audience"] }

:::step { name: "Write the hook", task: "draft-hook" }
:::

:::
```

Now the **Draft** stage needs `topic` and `audience` bound before it reports
`done`, the operator form for that stage shows only the `topic` and `audience`
inputs, and the draft task's prompt sees exactly those two slots in its
`# Dispatch Context` — any other bindings the journey holds are blinded out
of the model's view (while remaining fully readable by the task script).

> The `task: "draft-hook"` form above is single-task sugar for `tasks:
> ["draft-hook"]`, referencing a shared `:::nushell` block defined elsewhere in
> the guide. **A step is an ordered set of tasks, and multiple tasks per step is
> the norm.** The recommended way to author them is *nested* — write the
> `:::nushell` blocks as children inside the `:::step` fence (they become the
> step's private tasks, run in document order, fail together). For example:
>
> ```
> :::step { name: "Write the hook", expose: ["hook"] }
>
> :::nushell { name: "draft" }
> { payload: { ... }, signals: { } }
> :::
>
> :::nushell { name: "polish" }
> { payload: { ... }, signals: { } }
> :::
>
> :::
> ```
>
> See [Concepts → Authoring a step's tasks](./concepts.md) for the nested-vs-referenced rules.

### `expose` / `mask` attributes

| Key | Where | Description |
|-----|-------|-------------|
| `expose` | `:::stage`, `:::step` | Array of value-slot names the node needs. |
| `mask` | `:::stage`, `:::step` | Array of value-slot names to *hide* at this node, overriding an `expose` inherited from a broader scope. |

Names must match a `:::value_slot { name: … }` declared in the guide (the
`:::value-slot` hyphen spelling is also accepted). Author the slot before you
reference it.

### The cascade (stage → step → task)

Exposure resolves down the tree: a slot exposed at the **stage** applies to all
its steps/tasks; a **step** can expose extra slots, and a **`mask`** at a
narrower scope removes a slot that a broader scope exposed. The narrowest
declaration wins.

**A stage is a form / pane; its steps are the AJAXy sub-interactions on that
pane** (generate → pick → refine). Multi-step stages are the norm, so prefer
**step-level `expose`** for any slot a *specific* step needs or produces —
stage-level `expose` is for a slot the *whole* stage shares (e.g. one operator
input that every step reads). The narrower you author, the more precisely the
platform reports *which step* is awaiting input via
`GET /api/journeys/{id}/steps`.

### Computed outputs are produced by a step (not bound by an operator)

A `:::brief-output { source: "computed" }` is *produced* by a step, not bound by
an operator — the runtime writes its binding once the producing step **coheres**
(all its tasks succeed, no `unsafe` veto), extracting the value from the step
payload by `source_path`. A failed or vetoed step publishes nothing.

> **A computed output publishes only when the whole step coheres** — the binding
> is written after *every* task in the step has succeeded, not the instant the
> producer task returns. So put any validation / safety check **in the same step,
> after the producer task**. If a later task in the step fails (or vetoes with
> `signals.unsafe = true`), the step fails-together and the computed value is
> never published. Splitting the safety check into a *separate* step would let the
> producer's value go live before the check ran.

**A computed output must be wired three ways — each silent if missing:** it needs
a **`name`**; that name must be **listed in its `:::brief`'s `units`** (the
brief→output association is by name, exactly like inputs); and the **producing
step must `expose` its slot**. Miss any one and the output never materializes or
never binds — the produced value lands only in the run log and the stage reads as
stuck. Author it as a **top-level** `:::brief-output` referenced from the brief's
`units` — *not* an unnamed block nested under the step. (This matches the parser
and the `create_multistep_computed_ownership` materializer test.)

**Which step produces it?** The runtime binds a computed output from any step
that *exposes* it (via the overlay cascade) whose payload yields a value at
`source_path`. Two clean ways to point at the producing step:

- **Step-level `expose`** — put the output on the step that makes it
  (`:::step { expose: ["…"] }`). Only that step exposes it, so only it binds it.
  This is the precise, recommended form for multi-step stages.
- **Named `source_path`** — expose at the stage level but give the output a JSON
  pointer (e.g. `source_path: "/thesis"`); only the step whose payload has that
  key binds it. `source_path: "."` (the whole payload) is for a **single**
  computed output per stage; with several distinct outputs at the stage level
  with `"."`, the runtime can't tell them apart — use step-level expose or named
  paths.

**Write-once per dispatch; latest-wins across dispatches.** A binding is
immutable within a dispatch (exactly like an operator's answer), so if you fire
a whole stage as one dispatch the *first* producing step binds the slot. To
update a computed value, re-dispatch — the latest dispatch's binding wins. This
is why you have flexibility in how you batch steps into dispatches: a slow
"work-in-progress" stage can be one big dispatch, or split into interactive
batches, without changing what eventually binds.

**How producers surface to clients.** Every exposed slot on
`GET /api/journeys/{id}/steps` is self-describing, so a consumer (dapple-ui, the
dapple-suite runner, an external customer) never has to guess how a slot is
filled:

- An operator input reports `producer: "operator"` plus an `input` render spec
  (`control_type`, label/help/placeholder, `choices`) — the client renders the
  control and POSTs a binding.
- A computed output reports `producer: "computed"` and **no** `input` — the
  client shows a waiting state and never tries to bind it. Both carry the
  journey-pinned `value_slot_id`; the discriminator is `producer`, not a missing
  id.

When a computed output **can't** be produced (the task succeeded but its payload
had nothing at `source_path`, or the value didn't match the slot's kind), the
runtime records a per-output result on the task attempt instead of failing
silently. `GET /api/dispatches/{id}/task_attempts` exposes these as
`computed_outputs: [{ value_slot_id, name, outcome }]` where `outcome` is
`produced` | `source_path_missing` | `type_mismatch` | `unsupported_kind`. If a
chapter is stuck because a computed slot never bound, that is where the reason
shows — so make sure your task's `payload` actually yields a value at each
computed output's `source_path`.

```
# generate → pick → refine: each computed output exposed on its producing step
:::stage { name: "Pick a thesis" }

:::step { name: "Generate options", task: "gen-theses", expose: ["thesis-options"] }
:::

:::step { name: "Refine the pick", task: "refine-thesis", expose: ["chosen-thesis", "refined-thesis"] }
:::

:::
```

(`thesis-options` and `refined-thesis` are `source: "computed"` outputs — each a
**named** `:::brief-output` listed in its brief's `units` and exposed on its
producing step (see the three requirements above; the skeleton shows only the
`expose` placement). `chosen-thesis` is the operator's pick — a bindable input on
the refine step.)

### Multi-select picks (keep-N / curate a subset)

A pick is normally **single** — the operator chooses exactly one option. For a
**fan-out** (the operator curates *which subset* of a generated list survives —
"keep these 3 suppliers"), declare the pick slot **`cardinality: "multi"`**. The
binding then carries **N items** and the refine task receives an **array** under
`$in.bindings.<slot>` instead of a scalar.

```
# generate step — produces the candidate set as a computed json slot.
# The output is NAMED, listed in its brief's `units`, and its slot is exposed on
# the producing step (all three required — see above).
:::value_slot { name: "supplier_candidates", kind: "json" }
:::
:::brief-output { name: "supplier-candidates-output", source: "computed",
                  slot: "supplier_candidates", source_path: "/choices" }
:::
:::brief { name: "extract-suppliers-brief", units: ["supplier-candidates-output"] }
:::
:::step { name: "Extract suppliers", task: "extract-suppliers", expose: ["supplier_candidates"] }
:::

# curate step — operator keeps a subset (multi text slot, checkbox over the tiles)
:::value_slot { name: "kept_suppliers", kind: "text", cardinality: "multi", min: 0 }
:::
:::brief-input { name: "keep-suppliers-input", control: "checkbox",
                 slot: "kept_suppliers", candidate_source: "supplier_candidates" }
:::
:::brief { name: "curate-suppliers-brief", units: ["keep-suppliers-input"] }
:::
:::step { name: "Curate suppliers", expose: ["kept_suppliers"] }
:::
```

The pieces:

- **`cardinality`** on the `:::value_slot` is `"single"` (default) or `"multi"`.
  For multi you may add **`min`** and **`max`** selection bounds (`min: 0` permits
  a deliberate *keep none*; omit `max` for unbounded). The binding write rejects a
  count outside `[min, max]` with `400`.
- **`control: "checkbox"`** is the multi control. The generated tiles come from the
  `:::value_slot` named by **`candidate_source`** — a computed `kind: json` slot
  whose bound value is the `[{label, description, value}, …]` array the generate
  task produced. `GET /api/journeys/{id}/steps` self-describes the pick: its
  `input` carries `candidate_source_value_slot_id` (read the tiles from that slot)
  and the slot reports `cardinality: "multi"` with `min_selections`/`max_selections`.
- The **guardrail is unchanged** — the model still proposes explained, bounded
  options and the operator only picks among them. Only the *cardinality* relaxes
  from exactly-one to a subset; this is not open-ended prompting.

The same `cardinality: "multi"` slot also works two other ways, so there is one
model everywhere:

- a **static `checkbox`** brief-input (authored `choices`, no `candidate_source`)
  collects an authored subset;
- a **computed multi-output** (`:::brief-output { source: "computed" }` whose
  `source_path` yields an **array**) writes one item per element — a task
  producing N drafts directly into a multi slot, no operator.

### Which slots does a stage need?

The slots a stage needs are the ones its work depends on — in practice, the
value slots its **brief inputs collect** and its **task prompts consume**. A
good rule of thumb: if a `:::brief_input` in this chapter writes to slot `X` and
the stage's tasks can't proceed without `X`, the stage should `expose: ["X"]`.
The value slot is the join key — the same `X` the brief input fills, the stage
exposes, and the operator binds.

## Fixing existing guides (remediation)

Existing guides were authored before overlays existed, so they expose nothing
and get no gating. To fix one:

1. **For each stage, list the value slots it needs** (the slots its brief inputs
   collect / its tasks require to proceed).
2. **Add `expose: [...]`** to that `:::stage` (and `mask` on a child step only
   if it needs to drop one).
3. **Re-ingest the guide.**

> A **backfill alone will not do this.** Backfill re-materializes the database
> from the guide *source*; if the source has no `expose`/`mask`, there is
> nothing to populate. You must edit the source and re-ingest.

Re-ingesting with new overlays is **safe for in-flight work**: overlays are part
of the playbook's content hash, so adding them forks a *new* guide/playbook
version. Journeys already pinned to the old version keep running against it
unchanged; new journeys pick up the gated version. Nothing "shifts underfoot".

### Before / after

```
# Before — no gating, operator sees every input, stage is "done" instantly
:::stage { name: "Draft" }
:::step { name: "Write the hook", task: "draft-hook" }
:::
:::

# After — gated on `topic` + `audience`; form shows just those two inputs
:::stage { name: "Draft", expose: ["topic", "audience"] }
:::step { name: "Write the hook", task: "draft-hook" }
:::
:::
```

## Verifying

After re-ingesting and starting a journey:

- `GET /api/journeys/{id}/stages` → the stage shows `done: false` until its
  exposed slots are bound, then `done: true`.
- `GET /api/journeys/{id}/brief-form` → only the exposed inputs appear for that
  stage (plus any notice/markdown blocks).
- Bind the slots (the operator fills the form → `dispatch_intent_bindings`) and
  re-check `/stages`: the stage flips to `done`.

## Common mistakes

- **Exposing a slot the guide never declares.** `expose: ["topic"]` with no
  `:::value_slot { name: "topic" }` is silently skipped (logged as a warning at
  ingest). Declare the slot first.
- **Expecting gating without authoring.** No `expose` ⇒ no gating. The "stage is
  already done / full form" behavior is the *unconfigured* state, not a feature.
- **Relying on a backfill to add overlays.** It can't — see remediation above.
- **Over-using `mask`.** Prefer narrow `expose` at each node over broad
  `expose` + `mask`. Reach for `mask` only to drop an inherited slot.
