# Dataflow Integration

> **Audience:** Backend Developers · Platform Engineers

The Flow Engine integrates with `dataflow-rs` for managing data flow between tasks and steps.

> **Runtime reality:** The live in-process auto-spawn path (`execute_dispatch`) runs a Step's Tasks **sequentially in authored (`step_tasks.ord`) order** and **stops on the first failure** — it does not run independent tasks concurrently or continue past a failed task. The graph/parallel/per-task-retry capabilities of `dataflow-rs` described below are framework features; the directive machinery that would drive them (`run_dispatch_from_dto`, `DirectiveRunner`) is currently exercised only by tests, not the live path.

## Overview

Dataflow-rs provides:

- **Graph-based execution** — Tasks as nodes, data as edges
- **Dependency resolution** — Automatic ordering based on data dependencies
- **Parallel execution** — Independent tasks can run concurrently (framework capability; the live path runs Tasks sequentially)
- **Error propagation** — Failures cascade correctly through the graph

## Data Namespaces

The flow engine uses namespaced data to separate concerns:

| Namespace | Purpose | Written By |
|-----------|---------|------------|
| `data.*` | Canonical payloads | Nu tasks |
| `metadata.*` | Control-plane state, signals, validation | Engine |
| `temp_data.*` | Engine scratch space | Engine (not persisted) |

### Namespace Conventions

```
data.resume.json           # Task output
data.resume._provenance    # Where the data came from
metadata.signals.*         # Task signals
metadata.validation.*      # Validation results
temp_data.scratch.*        # Temporary processing data
```

## Input Passing Patterns

### Simple Handoff

For sequential Tasks, the runtime sets `IN_ARTIFACT` to the path from the previous `artifact.json`:

```nushell
let prev = open $env.IN_ARTIFACT
let input_data = open $prev.path
```

### Multi-artifact Steps

For Steps that need multiple inputs, the runtime may provide a `step-context.json`:

```json
{
  "artifacts": [
    { "task": "task-1", "path": "/tmp/.../task-1/out/artifact.json" },
    { "task": "task-2", "path": "/tmp/.../task-2/out/artifact.json" }
  ]
}
```

## Task Types

### Nu Pipeline Task

Standard Nushell task that processes data:

```nushell
# Reads input, produces output
$in | transform | {
    payload: $in,
    signals: {}
}
```

Writes:
- `data.<base>.json` — Primary output
- `data.<base>._provenance` — Data lineage
- `metadata.signals.*` — Control signals

### map_payload Task

Reads data/metadata namespaces, writes metadata:

```rust
// Example: JSONLogic evaluation
let result = evaluate_jsonlogic(&rule, &payload);
metadata.set("evaluation_result", result);
```

Used for:
- Conditional branching
- Data transformation rules
- State machine transitions

### verify_payload Task

Validates data against schema or rules:

```rust
let validation = validate_schema(&payload, &schema);
metadata.set("validation.result", validation);
```

Writes:
- `metadata.validation.<namespace>.*` — Validation results

## Control Flow Pattern

Typical execution flow:

```
map_payload (pre)     → Evaluate pre-conditions
        ↓
    nu task           → Execute main logic
        ↓
map_payload (post)    → Process signals
        ↓
verify_payload        → Validate output
        ↓
    condition         → Branch decision
```

## Integration Points

### With StepEngine

```rust
// StepEngine receives input from dataflow
let input = dataflow.get_input(task_id)?;

// Execute task
let output = step_engine.execute(&script, input)?;

// Write output back to dataflow
dataflow.set_output(task_id, output.payload)?;
dataflow.set_signals(task_id, output.signals)?;
```

### With Conduit

```rust
// Task calls conduit via prompt commands
// Conduit URL comes from metadata
let conduit_url = metadata.get("conduit_url")?;
let response = prompt_send(&conduit_url, &prompt)?;
```

## Error Handling

In the live runtime a Step's Tasks **fail together**:

1. **Task failure** — Marks the task attempt as failed and **stops the Step on the first failure** (later Tasks do not run — not log-and-continue)
2. **Computed outputs** — Not published for a failed (or vetoed) Step; they publish once, together, only when the whole Step coheres
3. **Unsafe veto** — A Task emitting `signals.unsafe = true` puts the dispatch in the terminal `unsafe` phase (do-not-refire, non-retryable), distinct from a retryable `failed`
4. **Recovery** — Retry means **re-dispatch of the whole Step** (latest-wins), not per-Task retry

## Configuration

A Step groups one or more Tasks that run sequentially in authored order; there is no per-Task `on_failure: warn` escape — the first failure stops the Step:

```yaml
step:
  name: process-data
  tasks:        # run in order; fail together (first failure stops the step)
    - id: extract
      script: extract.nu
    - id: transform
      script: transform.nu
    - id: validate
      script: validate.nu
```
