# Dispatch start gate deadlocks on exposed computed/output slots

- **SHA-1**: builds from `5314cca`
- **Status**: Complete
- **Created**: 2026-06-03
- **Author**: Kenny Flegal

## Summary

A dispatch whose step **exposes a computed/output value slot** (a
`:::brief-output { source_kind: "computed" }`) could never start: the start gate
demanded that slot be bound, but a computed output is *produced by the task* and
has no brief-input a caller can bind, so the requirement was unsatisfiable. The
dispatch sat `status: null` with zero task attempts forever. The fix narrows the
start gate to **bindable-input** exposed slots only; the done/completion rollups
are unchanged and still count computed outputs.

## Current state (the bug)

`crates/dapple-data/assets/.sql/journeys/is_step_fully_bound.sql` is the dispatch
**start gate** — `dapple_data::query::journeys::dispatch_ready`
(`journeys.rs:676`) is its only caller, and both the internal
(`dapple-prompt-api`) and external (`dapple-prompt-external-api`)
`POST /api/dispatches` + `POST /api/dispatch-intent-bindings` handlers skip
auto-spawn when it returns false.

Its `required_paths` CTE took **every** overlay slot with `visibility = 'expose'`:

```sql
required_paths AS (
    SELECT DISTINCT value_slot_path_id
    FROM ranked
    WHERE visibility = 'expose'
)
```

A step exposing a computed output therefore put that slot in the required set.
The binding-existence check then demanded some dispatch in the journey chain had
bound it — but a computed/output slot has **no `brief_unit_input`** pinning it
(it is produced, not operator-bound), so nothing on the internal *or* external
surface could ever satisfy it. → never ready → never spawned → task `status:
null`, `[]` attempts, indefinitely.

This was surfaced by an external consumer (acme guide
`esboçar-o-restaurante`, chapter `o-sítio`, exposing `o-sítio-synthesis`), but it
is a **shared data-layer defect**: `dapple-ui` hits the identical deadlock on any
internal guide that exposes a computed output.

### Root insight: "expose" drives two gates; the computed output belongs to one

| Gate | Query | Semantics | Computed output |
|------|-------|-----------|-----------------|
| **Start** | `is_step_fully_bound` → `dispatch_ready` | "what must be bound before the step can RUN" | **excluded** — requiring an output before the task that produces it is circular |
| **Done** | `select_step_binding_status_for_journey` (`/steps`), `select_stage_binding_status_for_journey` (`/stages`) | "what must be bound before the step is DONE" | **included** — that is how step/stage completion becomes observable |

Exposing the computed output is the *correct* authoring pattern (it makes
completion observable). The bug was that the start gate treated "exposed" as
"must-be-bound-to-start."

## Desired state (the fix)

`required_paths` requires only exposed slots a brief-input unit pins, mirroring
the `journey_slot` join in `select_step_binding_status_for_journey.sql`:

```sql
required_paths AS (
    SELECT DISTINCT r.value_slot_path_id
    FROM ranked r
    WHERE r.visibility = 'expose'
      AND EXISTS (
          SELECT 1
          FROM rev.brief_unit_input bui
          JOIN rev.value_slots vs
              ON bui.control_spec ? 'value_slot_id'
             AND vs.id = (bui.control_spec ->> 'value_slot_id')::UUID
          JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
          WHERE vo.path_id = r.value_slot_path_id
      )
)
```

Computed/output slots (no `brief_unit_input`) drop out of the start gate;
input/files-sourced outputs drop out too (chosen over a narrow
`source_kind != 'computed'` filter so no produced-output kind can re-introduce
the deadlock). The global (not journey-scoped) `EXISTS` answers "is this slot
bindable from a brief form at all" — journey scoping stays in the
binding-existence check below it. No new `:bind()` parameters → `$N` order
unchanged → no Rust call-site change.

### Explicitly out of scope

- **`is_stage_fully_bound.sql` is NOT touched.** It is a done-semantics helper
  (no production caller; only `select_stage_binding_status_for_journey` carries
  the `/stages` done view) — applying the exclusion there would mark stages
  `done` before their outputs exist, contradicting the `/steps` rollup.
- **All done-rollup queries are unchanged.**

## Files modified

| File | Change |
|------|--------|
| `crates/dapple-data/assets/sqlc/journeys/is_step_fully_bound.sqlc` | Narrow `required_paths` to bindable inputs; header documents start-vs-done asymmetry |
| `crates/dapple-data/assets/.sql/journeys/is_step_fully_bound.sql` | Regenerated via `cargo sqlc compose` |
| `crates/dapple-data/.sqlx/` (+ workspace `.sqlx/`) | Regenerated via `cargo sqlx prepare --workspace` (old query hash dropped, new added) |
| `crates/dapple-prompt-api/tests/guide_ingest_test.rs` | New fixture `create_overlay_computed_output_guide_tarball` + test `test_dispatch_ready_ignores_computed_output_in_expose` |

## Verification

```bash
nu -c 'cd crates/dapple-data; cargo sqlc compose --verify --source assets/sqlc/ --target assets/.sql/'
nu -c 'cargo test -p dapple-prompt-api --test guide_ingest_test test_dispatch_ready'   # new + existing readiness tests
nu -c 'cargo test -p dapple-prompt-external-api'                                        # happy_path_full_dispatch_execution stays green
nu -c 'cargo clippy -p dapple-data -- -D warnings'
```

The regression test exposes both a bindable input (`slot-a`) and a computed
output (`synth`), binds only the input, then asserts the **start gate opens**
(`dispatch_ready == true`) while the **done rollup still reports the step not
`done`** (computed output unbound, `value_slot_id` is `None`) — the asymmetry in
one test. Pre-fix, `synth` sat in `required_paths` unbound, so
`is_step_fully_bound` returned false and the start-gate assertion fails.
