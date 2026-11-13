# Agent Instructions: dapple-prompt-flow

This crate implements the workflow execution engine for Dapple Prompt.

> **See also:** [`docs/books/journey-builder-guide/flow/`](../../docs/books/journey-builder-guide/) for narrative documentation of the StepEngine vs dataflow Engine distinction, directive evaluation, and dispatch lifecycle.

## Key Concepts

### Two Types of "Engine"

This crate contains two distinct "engine" concepts - do not confuse them:

1. **StepEngine** (`engine/mod.rs`) - Nushell script execution engine
   - Executes Nushell scripts with security policies
   - Returns `NuOutput { payload, signals }`
   - Used for tasks that need custom scripting
   - One instance per worker/step (has parse cache)

2. **dataflow Engine** (`dataflow_rs::Engine`) - Workflow orchestration
   - Processes workflows with built-in tasks (map, validate)
   - Uses JSONLogic for data transformation via datalogic-rs
   - Configured via workflow JSON definitions
   - Stateless - created per dispatch execution

### Data Namespaces

Scripts and workflows operate on three namespaces:

| Namespace | Purpose | Persistence |
|-----------|---------|-------------|
| `data.*` | Canonical payloads from tasks | Persisted to JSONB |
| `metadata.*` | Control-plane state, signals | Persisted to JSONB |
| `temp_data.*` | Engine scratch space | Not persisted |

> **Overlay blinders are render-boundary only.** The conduit-facing task
> context (`resolve_task_context` in dapple-prompt-api) filters its binding
> list by the task's overlay cascade; that filter must NEVER migrate into this
> crate's `$in.bindings` seeding — the invariant below stands unchanged.

### Binding read availability (journey-wide, order-independent)

A running task reads **every** value-slot binding its journey and predecessor chain hold, via
`select_bindings_by_journey_of_dispatch` → `$in.bindings.<slot_name>` (and pollers via
`select_effective_bindings_for_journey`). Keyed by value-slot, latest-wins (`depth ASC,
created_at DESC`). **This does NOT depend on stage, step, dispatch, or run order** — stage C
reads stage A's and B's values whether the journey ran A→B→C or B→A→C. Per-step *dispatch* and
per-stage/per-step *DONE rollups* scope **completion/production**, never **reads**. A
cross-stage value that isn't visible is a producer/binding-not-written problem, or a *consumer*
(e.g. a UI resolver) that scoped its own read to the active stage — never a missing backend
capability. Verified in the chain-wide pool of `select_bindings_by_journey_of_dispatch.sql` /
`select_effective_bindings_for_journey.sql` (no stage/ordinal filter) + the seed in
`dispatch_runner.rs` (`load_journey_seed` + the cache-aware block in
`execute_dispatch_with_seed_cache`). Do NOT add a stage/ordinal scope to these reads, and do not
re-introduce a "binding-withhold per step" model — see `docs/plans/step-as-atomic-transaction.md`.

**Delivery shape is separable from availability.** Two mechanisms reduce the seed's *cost*
without touching the invariant above:

- **Per-drain SeedCache** (`dispatch_runner.rs`): within one `drain_ready_dispatches`, the
  expensive seed (chain-wide bindings + prior task contexts) is reused across dispatches when
  nothing changed — revalidated on every hit against the append-only
  `select_journey_seed_watermark` counters (catches out-of-process writes: operator POSTs to
  the api pod, the other pod completing a dispatch), and invalidated the moment any dispatch
  records an ok task. Retries of a failing step (which publish nothing) are the cache's
  target case — the 2026-08-02 OOMKill was 3 back-to-back full re-seeds in one drain.
- **Envelope delivery** (`constraints.delivery = "envelope"`, json slots): a per-slot AUTHORED
  opt-in that seeds `$in.bindings.<slot>` as a handle
  `{ delivery: "envelope", binding_id, value_slot_id, value_kind, cardinality }` instead of the
  payload; the task resolves the value on demand with `binding open <binding_id>`
  (`commands/binding.rs`, backed by `select_binding_value_by_id`). The dataset-envelope
  precedent generalized. The slot remains visible to every task in the chain — only the shape
  changes, and only where a guide author asked for it. Migrating a slot to envelope delivery
  REQUIRES editing every script that reads it (`$in.bindings.<slot>` → `binding open`); an
  unmigrated reader sees the handle record and fails deterministically (caught by the
  fast-stop gate, but still a stall).

### Module Structure

```
src/
├── engine/
│   ├── mod.rs          # StepEngine - Nushell execution
│   ├── security.rs     # SecurityPolicy, SecurityError
│   └── plugins.rs      # Plugin management
├── func/
│   ├── mod.rs          # Function handler modules
│   ├── prompt.rs       # PromptTextFunction (dataflow custom function)
│   ├── handler_adapter.rs  # Adapts handlers between Arc/Box
│   ├── nu_handler.rs   # Nushell-to-dataflow adapter
│   ├── nu_pipeline.rs  # Pipeline execution helpers
│   └── test_handler.rs # Test utilities
├── commands/
│   ├── mod.rs          # Custom Nushell command registration
│   ├── prompt.rs       # `prompt` parent command
│   ├── prompt_send.rs  # `prompt send` subcommand
│   ├── prompt_retry.rs # `prompt retry` subcommand
│   ├── prompt_reset.rs # `prompt reset` subcommand
│   ├── summarize.rs    # `summarize` command
│   ├── madlib.rs       # `madlib` command
│   └── json_schema.rs  # `json-schema` command
├── directives/         # Flow logic (directive evaluation)
│   ├── mod.rs          # DirectiveRunner, module exports
│   ├── evaluator.rs    # DirectiveEvaluator with `run` operator
│   ├── actions.rs      # DirectiveAction enum
│   ├── context.rs      # DirectiveContext
│   ├── run_handler.rs  # RunHandler trait
│   └── error.rs        # DirectiveError
├── dispatch_runner.rs  # Dispatch execution orchestration
├── conduit_client.rs   # HTTP client for conduit service
├── test_support.rs     # Test utilities
└── lib.rs              # Public API and Step struct
```

## Directive System

Directives provide conditional execution, retry, and branching for playbook stages and steps. See `docs/plans/flow-logic-dapple-flow.md` for full architecture.

### Key Components

- **DirectiveEvaluator** - Evaluates JSONLogic with custom `run` operator
- **DirectiveRunner** - Orchestrates retry/loop logic and action dispatch
- **RunHandler** - Executes the actual stage/step when `run` is invoked

### The `run` Operator

The `run` operator is NOT a true JSONLogic custom operator. It is handled via AST preprocessing:

1. Before evaluation, walk the JSONLogic tree for `{"run": []}`
2. If found, execute the stage/step via RunHandler
3. Inject result into context as `context.run`
4. Replace `{"run": []}` with `{"var": "run"}` for evaluation
5. This ensures `run` executes at most once per evaluation

### Action Values

| Action | Meaning |
|--------|---------|
| `"proceed"` | Continue to next stage/step |
| `"skip"` | Skip without execution |
| `"fail"` | Halt with error |
| `"retry"` | Re-execute after delay (up to max_attempts) |
| `"loop"` | Re-execute immediately (up to max_attempts) |
| `"{uuid}"` | Branch to stage/step with that `id` |

### Branch Scope Rules

- Stage directives can branch to any stage in the playbook
- Step directives can only branch to steps within the same stage

## Implementation Guidelines

### Adding Custom Nushell Commands

1. Create `src/commands/{command}.rs`
2. Implement `nu_protocol::Command` trait:
   ```rust
   impl Command for MyCommand {
       fn name(&self) -> &str { "my-command" }
       fn signature(&self) -> Signature { ... }
       fn run(&self, engine_state: &EngineState, stack: &mut Stack, call: &Call, input: PipelineData) -> Result<PipelineData, ShellError> { ... }
   }
   ```
3. Register in `commands/mod.rs`:
   ```rust
   working_set.add_decl(Box::new(my_command::MyCommand));
   ```
4. Add to security allowlist in `engine/security.rs` if using standard allowlist mode

### Adding Custom dataflow Functions

1. Create handler in `src/func/{function}.rs`
2. Implement `dataflow_rs::AsyncFunctionHandler`:
   ```rust
   #[async_trait]
   impl AsyncFunctionHandler for MyFunction {
       async fn execute(&self, message: &mut Message, config: &FunctionConfig, datalogic: Arc<DataLogic>) -> Result<(usize, Vec<Change>)> { ... }
   }
   ```
3. Register in `lib.rs` Step::engine():
   ```rust
   let functions = HashMap::from([
       ("my_function".into(), Box::new(MyFunction::default()) as Box<dyn AsyncFunctionHandler + Send + Sync>),
   ]);
   ```

### Modifying Directive Evaluation

- JSONLogic handling: `directives/evaluator.rs`
- Action parsing: `directives/actions.rs`
- New error types: `directives/error.rs`
- Run operator preprocessing: `DirectiveEvaluator::preprocess_run()`

### Working with dispatch_runner

The dispatch_runner executes dispatches with directive evaluation:

```rust
pub async fn run_dispatch_from_dto(
    db: &DbPool,
    dispatch: Dispatch,
    step: &Step,
    functions: Option<Arc<HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>>>,
    concurrency: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
```

Key behaviors:
- Stage-level directives evaluated before any steps execute
- Step-level directives evaluated before tasks in that step execute
- Branch targets validated at attachment time (API layer), not runtime
- Audit events recorded via `insert_task_attempt_event_with_attempt_id`

## Script Contract

All Nushell scripts executed via StepEngine MUST return:

```nushell
{ payload: <any>, signals: { ... } }
```

- `payload` - The task output (any JSON-serializable value)
- `signals` - Control-plane metadata (record/map). `signals.unsafe = true`
  (optionally with a `reason`) is the **safety veto**: it stops the step and ends
  the task attempt in the terminal `unsafe` phase (see "Step execution model").

Example:
```nushell
let result = $in | process-data
{ payload: $result, signals: { completed: true, items_processed: ($result | length) } }
```

### Step execution model (ordered + fail-together)

A **step is an ordered set of tasks run as one fail-together unit** by
`execute_dispatch` — the live in-process auto-spawn path both API surfaces call.
(`run_dispatch_from_dto` + the `DirectiveRunner` machinery documented below are
currently exercised **only by tests**; the live path runs steps directly without
directive evaluation.) Within a step:

- Tasks run **sequentially in `step_tasks.ord` order**; each task's `payload`
  flows to the next via `$in` (`accumulated_data`). The concurrency knob never
  reorders tasks within a step.
- **Stop on first failure.** A task error halts the step — later tasks do NOT
  run. Nothing rolls back: every task attempt, payload, and asset is recorded as
  history. A step is a **coherence + retry boundary, not a DB transaction** (the
  tasks run too long to share an open transaction anyway).
- **Unsafe veto.** A task that runs fine but judges its result poisoned returns
  `signals.unsafe = true`; its attempt ends in the terminal `unsafe` phase and
  the step stops.
- **Derived step verdict**, worst-wins across the step's task attempts: any
  `unsafe` → unsafe; else any `failed` → failed; else `ok`.
- **Retry is a re-dispatch.** A failed step leaves its slots unbound (not-done);
  re-dispatching re-runs it and latest-wins resolves the binding. No auto-retry.
- **Failure class gates auto-redispatch of pure-compute steps.** A nushell task
  failure is classified at the engine (`NuErrorClass` in `engine/mod.rs`, by
  `ShellError` variant: language/shape errors → `deterministic`; `GenericError` —
  every custom-command failure incl. conduit/EDGAR/network — and unknown shapes →
  `transient`). The class rides `message.metadata.error_class` → the failed
  event's `context_metadata.error_class` (root, NEVER inside `signals` — the
  external surface projects `signals` verbatim). The auto-dispatch retry gate
  (`select_pending_autodispatch_steps_for_journey.sqlc`) treats a deterministic
  failure like `unsafe`: **no auto-redispatch** (re-running a script error burns
  a full context seed to fail identically — a 3x tight loop of exactly this shape
  OOMed the flow pod, 2026-08-02). Transient failures keep the 3-dispatch cap.
  Recovery from a deterministic block: fix the guide script, re-ingest, then
  **manually POST a new dispatch** (that endpoint never consults the gate);
  re-ingest alone does not reset it for an existing journey. When in doubt a
  variant classifies TRANSIENT — a misclassified conduit 5xx would strand a
  healthy step, while a misclassified script bug merely burns the bounded cap.

### Computed-output production (per step, at coherent completion)

After a step **coheres** (every task succeeded, no `unsafe` veto), `execute_dispatch`
calls `produce_computed_output_bindings(db, dispatch_id, playbook_stage_id,
stage_step_id, &step_payload)` **once for the step** — not per task. A failed or
vetoed step produces **nothing**, so a poisoned/partial value never reaches the
binding layer (`$in.bindings.*`, chained-journey inheritance, pollers, the DONE
rollup) — this is why production is deferred to step end rather than each task's
`Ok` arm. For each computed `:::brief-output` (`source_kind = 'computed'`) the
step exposes, it writes a `rev.dispatch_intent_bindings` row + typed item
(`value_meta.producer = "computed"`), extracting the value from the step's
accumulated **`payload`** via the output's `source_path` (`.`/empty = the whole
payload root; otherwise an RFC6901 JSON pointer). All six `value_kind`s route
(`text`/`int`/`number`/`bool`/`json`/`asset`; an `asset` output parses the
extracted value as a UUID). The slot version it binds is the journey-pinned one
(`select_computed_outputs_for_step`), so it matches `/brief-form` and the DONE
rollups.

**It returns `Vec<ComputedOutputOutcome>`** — one `{ value_slot_id, name, outcome }`
per output it attempts (`produced` | `source_path_missing` | `type_mismatch` |
`unsupported_kind`); already-bound slots are skipped (idempotent). The caller
folds these into the step's final `ok` task-attempt event's
`context_metadata.computed_outputs` (parallel to `context_metadata.signals`), so the external
`/dispatches/{id}/task_attempts` surface projects them into a typed
`computed_outputs` field. That turns a silently-unproduced computed output (the
old warn-and-continue-to-a-log-line behavior) into a queryable signal a customer
can see — the fix for "chapter stuck pendente with no explanation". A producer
failure still never fails the dispatch.

This is the **producer half** of the start-vs-done asymmetry: the START gate
(`is_step_fully_bound`) excludes computed outputs so a dispatch can spawn on
operator inputs alone, but the DONE rollups
(`select_*_binding_status_for_journey`) count them — and nothing else writes the
produced value to the binding layer. Without the producer the value is absent
from `$in.bindings.*`, chained-journey inheritance, and the DONE rollup, so the
stage can never reach `done`. The call is idempotent (already-bound slots are
skipped) and warn-and-continue (a producer failure never fails the dispatch); if
a value genuinely can't be produced the slot stays unbound and `done` correctly
stays false.

**Consumer half of the asymmetry — the consumed-computed-dependency gate.** A
step that READS a computed output produced by an EARLIER stage (e.g. wimmelbook
render reading `wimmelbook_shotlist`) must not start before that slot is bound, or
it runs on missing input and spends tokens on a result that can't honor the
direction. `is_step_fully_bound` still excludes computed outputs (so a PRODUCER
spawns on operator inputs); the consumer gate is layered in `dispatch_ready`,
which additionally calls `select_consumed_computed_deps_for_step`: a computed slot
the step EXPOSES that an earlier-`playbook_stages.ord` step also exposes is a
consumed dep (producer = earliest exposer). It blocks the dispatch until the dep
is bound, EXCEPT when the producer step is in the SAME dispatch (in-dispatch data
flow — gating would deadlock). Detection needs the consumer to EXPOSE the slot
(authoring convention); a read-without-expose is invisible here (the guide task's
own fail-loud guard is the backstop). Because a gated dispatch would otherwise
wait forever, `execute_dispatch_and_retrigger` (what the 4 API handler spawn sites
call instead of `execute_dispatch`) runs `drain_ready_dispatches` after a dispatch
finishes: an iterative (non-recursive — recursive spawn was non-`Send`) sweep that
launches every not-yet-started journey dispatch now made ready by the bindings the
just-finished dispatch published. See `select_consumed_computed_deps_for_step.sqlc`
+ `select_unstarted_dispatch_ids_for_journey.sqlc` and the
`consumed_computed_dep_gates_then_retriggers` test.

**The runtime owns computed dispatching — single owner, no consumer opt-out.**
`ensure_pending_autodispatches` (inside every `drain_ready_dispatches` iteration)
creates a dispatch for EVERY pure-compute step whose earlier stages are done:
exposes ≥1 computed brief-output, exposes no operator input, has no dispatch yet.
That **includes a picker-feed generate** (one whose output is a brief input's
`candidate_source`). Picker feeds were once excluded here on the rationale that the
consumer dispatches them on pane open — because two owners double-ran a
non-deterministic generate and flapped the picker (two candidate arrays ~1s apart,
run-the-10k "Lay Out the Business"). The flap was real, but its cause was *two
owners*: opting the runtime out made every consumer responsible for advancing the
journey, and a consumer that merely waits (the behaviour `/steps` documents for a
`producer: "computed"` slot) then stalls with no runtime-side recovery — dapple-ui's
only advancement path was a 20s-deadline poll loop, so a 238s EDGAR pull returned
before advancing and nothing created the successor until the operator happened to
re-enter the submit path (live: 20 minutes later, long after they gave up; the pane
offers nothing to submit, so nothing prompts it). **Do NOT re-add the exclusion, and
do NOT let a consumer dispatch a pure-compute step.** Guarded by
`auto_dispatch_runs_candidate_feed_step` (asserts EXACTLY one dispatch: zero catches
the exclusion returning, two catches a consumer self-dispatching) and
`picker_feed_generate_is_pending_autodispatch`. The per-dispatch
`dispatches_in_flight` guard in `execute_dispatch` is the other half: it keeps a
poll-triggered drain from re-*executing* a dispatch a direct `/execute` is already
running (observed 2 tasks → 4 attempts), which would reproduce the same flap.

**Which step produces a computed output (write-once per dispatch).** Multi-step
stages are the norm (a stage is a static form / pane; its steps are the AJAXy
sub-interactions — generate → pick → refine — on that pane before the user
navigates to the next stage), and a **dispatch is a batch of one-or-more steps**
— you may fire a whole stage, or arbitrary batches, as a single dispatch.
`select_computed_outputs_for_step` returns every computed output the step
**exposes** (cascade), and the producer binds those whose `source_path` yields a
value from the task payload. The producing step is identified implicitly:
- **step-level `expose`** scopes naturally — only the exposing step's cascade
  returns the output, so other steps never bind it;
- a **named `source_path`** (JSON pointer) scopes by data — under a stage-level
  expose, only the step whose payload has that key binds it;
- **`source_path: "."`** (whole payload) is for a **single** computed output per
  stage; for several distinct outputs use step-level `expose` or named paths.

**Write-once per dispatch, latest-wins across dispatches.** `UNIQUE(dispatch_id,
value_slot_id)` makes a binding immutable within a dispatch (the producer
pre-filters already-bound slots — the FIRST producing step wins), exactly like
operator answers. To change a value, re-dispatch: `select_effective_bindings_for_journey`
resolves the latest binding per slot (`depth ASC, created_at DESC`). So firing a
whole stage with a stage-level computed output binds it once (first step); a
later recompute is a later dispatch.

## Dataset tasks (spreadsheet / tabular value_kind)

- A dataset is a **set of named tables**, each a set of typed columns (NOT
  "sheets" — sheets are an xlsx source detail: each xlsx worksheet materializes
  as one table; a whole CSV materializes as a single table **always named `data`**
  — permanent canonical `CSV_TABLE_NAME`, so authored scripts have a predictable
  name to read; the operator filename is display-only on
  `rev.journey_asset_uploads.file_name`). (Amended 2026-07-20: the earlier
  file-stem naming rule is dropped, not deferred.)
- A `dataset` binding PINS an extraction row by FK (no envelope copy; two
  bindings of one dataset share the row); a task/reader sees its ENVELOPE
  (`{ asset_id, tables, sample? }`), never the rows. Read full rows in a task with
  `dataset open $asset_id --wait` (optionally `--table <name>` for one table;
  Tier-B DB read + `from xlsx`/`from csv` parse); read schema-only with
  `dataset schema $asset_id`.
- LLM-exposure contract: raw rows never go into an LLM prompt by default.
  Forward aggregates/schema/explicit samples only when the use case requires it
  (`dataset open … | <aggregate> | prompt send`).
- All numeric cells parse as f64; `int` vs `number` is a best-effort inference,
  overridable via the value-slot `constraints.columns` map. **Overrides are
  per-slot at READ time** (Amended 2026-07-20): the extraction envelope is
  slot-agnostic and shared across bindings, so `constraints.columns` type
  overrides and `constraints.sample: false` are merged over the pinned envelope
  by the dapple-data binding-read query layer
  (`apply_dataset_slot_constraints`), NOT baked at extraction. `dataset schema`
  is slot-less: it returns the RAW (unmerged) inferred envelope.

See docs/plans/spreadsheet-dataset-ingest.md.

## Security Policy

StepEngine enforces security at parse time:

- **Permissive mode** (default) - Allows all internal Nushell commands
- **Allowlist mode** - Only commands in allowlist permitted
- **Always blocked**: External commands, env mutation, plugin management

When adding new commands, consider whether they should be in the standard allowlist (`engine/security.rs`).

## Workflow / Prompt Flow (dapple-prompt-flow)

### Nushell Execution Model
- **One StepEngine per worker/step** with parse cache by script SHA
- **Scripts MUST return:** `{ payload, signals }` record
- **Canonical storage:** JSON in JSONB columns (`context_data`, `context_metadata`)
- **Data namespace:** `data.*` for canonical payloads (written by Nu tasks)
- **Metadata namespace:** `metadata.*` for control-plane state, signals, validation
- **Temp namespace:** `temp_data.*` for engine scratch (not persisted)

### Task Types
1. **Nu pipeline task** — Writes `data.<base>.json`, `data.<base>._provenance`, `metadata.signals.*`
2. **map_payload** — Reads `data.*`/`metadata.*`, writes `metadata.*` (JSONLogic evaluation)
3. **verify_payload** — Reads `data.<base>.json`, writes `metadata.validation.<namespace>.*`
4. **Built-in tasks** — Avoid mutating `data.<base>.json`

### Security Rules
- No external commands or plugin management
- Only allow-listed plugins (global policy)
- No `$env` mutation
- Parse-time rejection with `DataflowError`

### Control Flow Pattern
```
map_payload (pre) → nu task → map_payload (post) → verify_payload → condition
```

## Common Patterns

### Directive Evaluation Pattern

```rust
let directive_runner = DirectiveRunner::new();
let action = directive_runner.run_stage_directive(
    directive.as_ref(),
    context_data,
    context_metadata,
    run_handler,
    db,
    attempt_id,
).await?;

match action {
    DirectiveAction::Proceed => { /* continue normally */ }
    DirectiveAction::Skip => { /* skip this element */ }
    DirectiveAction::Fail => { /* halt with error */ }
    DirectiveAction::Branch(uuid) => { /* jump to target */ }
    // Retry/Loop handled internally by DirectiveRunner
}
```

### RunHandler Implementation

```rust
#[async_trait]
impl RunHandler for MyHandler {
    async fn execute(&self) -> Result<RunResult, DirectiveError> {
        let start = std::time::Instant::now();
        let result = do_work().await;

        match result {
            Ok((data, signals)) => Ok(RunResult {
                status: RunStatus::Ok,
                error_code: None,
                data,
                signals,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(e) => Ok(RunResult {
                status: categorize_status(&e),
                error_code: Some(categorize_error_code(&e)),
                data: Value::Null,
                signals: HashMap::new(),
                duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}
```

## Testing

### Unit Tests

- Directive evaluation: Mock RunHandler, test JSONLogic patterns
- Action parsing: Test all action strings including UUID parsing
- Security policy: Test allowlist enforcement

### Integration Tests

- Located in `crates/dapple-prompt-api/tests/`
- Test with actual database using `#[sqlx::test]`
- Test retry exhaustion, branch validation, audit trail

### Running Tests

```bash
# Unit tests
cargo test -p dapple-prompt-flow

# With logging
RUST_LOG=debug cargo test -p dapple-prompt-flow

# Specific test
cargo test -p dapple-prompt-flow test_directive_evaluation
```

## Dependencies

Key dependencies and their purposes:

| Crate | Purpose |
|-------|---------|
| `dataflow-rs` | Workflow orchestration framework |
| `datalogic-rs` | JSONLogic evaluation |
| `nu-*` | Nushell runtime (nu-engine, nu-parser, nu-protocol, nu-command, nu-cmd-lang) |
| `dapple-data` | Database queries and models |
| `reqwest` | HTTP client for conduit service |
| `tokio` | Async runtime |

## Anti-Patterns

- Do NOT call `run` multiple times in a directive - it executes at most once
- Do NOT branch to steps outside the current stage from a step directive
- Do NOT expose internal lookup-table IDs in directive contexts - use UUID `id` fields only
- Do NOT bypass security policy for "convenience" - add to allowlist properly
- Do NOT mix StepEngine and dataflow Engine concepts - they serve different purposes
