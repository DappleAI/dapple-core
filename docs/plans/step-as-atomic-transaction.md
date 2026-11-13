# Step as an ordered, fail-together task-set — Design

> Status: **design settled** (2026-06-16), implementation in progress. Supersedes
> an earlier draft that wrongly framed a step as an atomic DB transaction with
> rollback / binding-withhold — that framing is retracted; see "What this is NOT".

## The model

A **step is a coherence + retry boundary, not a database transaction.** The reason
the step abstraction exists is to group tasks that must **not dangle** — e.g.
`get → retrieve → mangle → check-safety` — so they run in **guaranteed order** and
**fail together**: the step is one unit of success-or-failure, never a set of
independently-completing pieces.

Consequences:

- **Multiple tasks per step is the norm**, not an edge case.
- **Data retrieval is a *task inside* a step**, never its own step. A guide that
  makes retrieval its own step (or splits one coherent sequence across several
  steps) has lost the no-dangle guarantee — that is an **authoring** error to
  correct, not something the backend should paper over.
- Steps remain a real shape for pane-level operator interaction
  (generate → pick → refine); each such step is itself an ordered task-set. The
  distinction is "is this a unit that must complete together," not "is this one
  task."

### What this is NOT

Nothing rolls back. The tasks run for seconds to minutes (LLM calls, retrieval),
so they could never share an open Postgres transaction, and we do not try. Every
task attempt, payload (`context_data`), and asset that gets produced **stays
recorded as history**, regardless of whether the step ultimately coheres. "No
partials" is a statement about the step's **answer** (its value-slot bindings) and
its **completion signal**, not about erasing what happened.

## Guarantees

1. **Ordered.** A step's tasks run sequentially in `step_tasks.ord` order. The
   dispatch concurrency knob may parallelize across independent steps/dispatches
   but must never reorder or parallelize tasks *within* a step.
2. **Fail together.** On the first task failure the step **stops** — later tasks
   in the step do not run (they would consume a missing predecessor anyway).
3. **One derived verdict per step, worst-wins:** any `unsafe` → unsafe; else any
   `failed` → failed; else all `ok` → done. Derived from the step's task-attempt
   phases — **no new schema column**.
4. **Answer published per-step at coherent completion.** A step's computed-output
   bindings are produced only when the whole step succeeds. A failed/vetoed step
   publishes **no** bindings, so its (possibly poisoned) value is invisible
   *everywhere consistently* — the next step's `$in.bindings.*`, chained-journey
   inheritance, pollers, and the done rollup. The produced value is still
   inspectable in the task attempt's `context_data` (no audit loss).
5. **Retry is a re-dispatch.** A failed step leaves its slots unbound → it shows
   not-done → the consumer re-dispatches; latest-wins resolves the answer
   (`UNIQUE(dispatch_id, value_slot_id)` write-once per dispatch; `depth ASC,
   created_at DESC` latest across dispatches). No auto-retry loop in the runner.
6. **`unsafe` is terminal and non-retryable.** It marks purposeful poisoning — a
   case you might never retry — distinct from a transient `failed` (freely
   retryable) and from `canceled` (deliberately stopped).

### Why publish-per-step, not write-as-you-go (the decisive point)

A binding row in `rev.dispatch_intent_bindings` does not only drive the done
checkbox. The same rows feed the next step's `$in.bindings.*`, chained-journey
inheritance (`select_effective_bindings_for_journey`), and anything polling
`/brief-form` / effective bindings.

So if `mangle` wrote its value the instant it ran and `check-safety` then vetoed,
that value would be **live to every one of those consumers** — the verdict could
hide the checkbox but not stop the poisoned value from flowing into the next step
or a successor journey, and since `unsafe` may never be retried it would stay live
indefinitely. Producing the step's outputs only at coherent completion contains
the value end-to-end. (This is why "gate done on the verdict but keep per-task
writes" was rejected: it fixes the display, not the leak, and it would have forced
a verdict-aware change into the otherwise-unchanged rollup queries.)

## Current state (grounded)

| Layer | Status | Where |
|---|---|---|
| Storage | ✅ N ordered tasks/step | `rev.step_tasks` assoc + `ord` (migration 53) |
| Read | ✅ returns all tasks by `ord` | the 4 select bodies (`select_steps_body`, `select_playbooks_body`, `select_playbooks_by_object_ids_by_user`, `stages/stage_steps/select_stage_steps_by_stage_ids`); fold → `StepModel.tasks: Vec` / playbook DTO |
| Rollups + start gate | ✅ task-count-agnostic, **zero change** | `is_step_fully_bound`, `select_stage/step_binding_status_for_journey`, `select_computed_outputs_for_step` — all aggregate overlays across every `step_task` and dedup; keyed per-step, never per-task |
| Authoring | ❌ one task per step | `StepDef.task_ref: Option<String>` (`crates/dapple-parser/src/transform/mod.rs`), read at `transform/playbook.rs` |
| Hashing | ❌ folds one task hash | `hash_step(...)` (`crates/dapple-hash/src/lib.rs:160`) |
| Materialize | ❌ authors one `step_task` | `guide_ingest.rs` new-step arm (`task_ids: vec![oid]`) + patch-step arm |
| Runtime | ❌ no order/fail-together guarantee | `execute_dispatch` (`dispatch_runner.rs`): loops tasks but logs-and-continues on failure (`~:755`) and binds per-task (`~:703`); `accumulated_data` is dispatch-scoped (`~:364`) |

Storage + read + rollups are ready. Authoring, hashing, materialize, and the
runtime guarantee are the work.

## Design

### 1. Authoring (parser) — a step lists ordered tasks
- `:::step` accepts `tasks: ["get","retrieve","mangle","check-safety"]` (ordered).
  Keep `task: "x"` as sugar for a single-element list. **List-attr only** to start
  (nested task child-blocks deferred).
- `StepDef.task_ref: Option<String>` → `StepDef.task_refs: Vec<String>` (ordered;
  empty allowed). Update `extract_step` and `test_step_with_task_ref`.

### 2. Hashing — Merkle over the ordered task set
- `hash_step(display, flow_control, task_content_hashes: &[&str], overlay_fp)`
  folds the **ordered** task hashes. Reordering or changing any task re-versions
  the step. Update `playbook_root_hash` and the `hash_step` call sites in
  `guide_ingest.rs`.
- **Back-compat is critical:** the N=1 encoding MUST be byte-identical to today's
  single-task `hash_step`, or every existing single-task step re-versions on next
  ingest (mass churn). For a 1-element slice, emit exactly the current framing;
  only N>1 extends it. Pin it with a unit test:
  `hash_step(.., &[h], ..) == legacy hash_step(.., h, ..)`.

### 3. Materialize — author N step_tasks in order
- Build the ordered `task_ids` Vec from `task_refs` (each name via
  `task_name_to_id`) for BOTH the new-step and patch-step arms; drop the
  single-element cap. `insert_step_tasks` already inserts N with `ord` via
  `UNNEST WITH ORDINALITY`.

### 4. Read — done
Returns all tasks ordered by `step_tasks.ord` (post-unwind). The external-caller
variants compose the same bodies, so both surfaces are covered.

### 5. Runtime — ordered + fail-together (`dispatch_runner::execute_dispatch`)
Live path is `execute_dispatch` (both surfaces auto-spawn it via `tokio::spawn`;
`run_dispatch_from_dto` and the directive machinery are test-only — out of scope).
- Run the step's tasks sequentially in `ord` order; pass data task→task in memory
  (`accumulated_data`). Ensure concurrency never reorders within a step.
- **Stop on first failure** — replace today's log-and-continue so a task error
  halts the step; later tasks don't run.
- **Unsafe veto** — a task signals it via `signals.unsafe = true` (+ optional
  `reason`); that attempt ends in the `unsafe` terminal phase, an ordinary error
  in `failed`.
- **Derive the step verdict** (worst-wins) from its task attempts; surface it
  where consumers decide whether to refire (e.g. `/journeys/{id}/steps`; the
  `unsafe` phase already appears on `/dispatches/{id}/task_attempts`).
- **Produce computed outputs per-step at completion** — move
  `produce_computed_output_bindings` off the per-task `Ok` arm to after the step's
  last task succeeds, sourcing from the step's accumulated payload. Produce nothing
  on failed/unsafe. (`accumulated_data` is dispatch-scoped today; snapshot or scope
  it per-step for the production source.)
- **Audit rows stay per-task, outside any tx** (already so) — a failed/vetoed task
  remains observable (which task, why).
- Preserves write-once-per-dispatch and latest-wins-across-dispatches.

### 6. Done / rollup semantics — no change
A step is done when every exposed slot is bound; a failed step bound nothing, so it
is not-done automatically. The rollup/gate queries are already task-count-agnostic
and need no change (see Current state).

### 7. New terminal phase
Add `('unsafe', true)` to `rev.dispatch_phases` via a seed migration (terminals
today: `failed`, `ok`, `canceled`). Seed data lives in migrations.

### 8. Guide-authoring guidance (to teach the authoring agents)
Document in the journey-planner book:
- A sequence that must not leave dangling/partial state = **one step** with ordered
  tasks.
- Retrieval / fetch / lookup = a **task**, never its own step.
- Use separate steps only for genuine pane-level operator interaction
  (generate → pick → refine) where each step is independently committable.

## Migration / back-compat
- **No schema migration for tasks** — `rev.step_tasks` already has `ord`. One seed
  migration for the `unsafe` phase.
- Existing single-task guides become 1-element lists. With the §2 byte-identical
  N=1 hash they do **not** re-version.
- Existing mis-authored guides with stacked/cross-wired tasks (e.g. shaggy-dog's
  `plant-the-running-gags`) now surface **all** their tasks honestly; correcting
  them is a re-author + re-ingest, not a backend heal.

## Open / minor decisions
1. Authoring syntax: list-attr only (recommended) vs. also nested task
   child-blocks.
2. Unsafe signal key: `signals.unsafe = true` (+ optional `reason`) (recommended).
3. Unsafe terminal: new `('unsafe', true)` row (recommended) vs. overload
   `canceled`.

## Verification plan
- **Parser unit test:** `:::step { tasks: [a,b,c] }` → `task_refs == [a,b,c]`;
  `task: "x"` → `["x"]`.
- **Hash unit test:** N=1 equals legacy `hash_step`; reorder/change any task
  changes it.
- **Materialize + read integration:** ingest a guide with a 3-task step; assert
  `select_steps` / `select_playbooks` return the 3 tasks in `ord` order.
- **Runtime (corrupt-then-verify):** a 3-task step whose 2nd task fails → assert
  task 3 has no attempt row, step verdict `failed`, ZERO computed-output bindings
  for the step, prior task payloads still recorded; an `unsafe` veto → attempt
  ends `unsafe`, verdict unsafe, no bindings; a clean run → all tasks ran in `ord`
  order, verdict `ok`, bindings produced once.
