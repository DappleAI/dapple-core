# Nushell Execution Model

> **Audience:** Backend Developers · Platform Engineers

This chapter explains how Nushell scripts are executed within the Flow Engine.

## Script Contract

All Nushell Tasks **must** return a record with this structure:

```nushell
{
    payload: <any>,      # Your output data (required)
    signals: { }         # Control signals (optional, defaults to empty)
}
```

### Valid Examples

```nushell
# Minimal valid output
{ payload: "done", signals: {} }

# With data payload
{
    payload: {
        name: "John Doe",
        email: "john@example.com"
    },
    signals: {}
}

# With signals
{
    payload: $result,
    signals: {
        needs_review: true,
        confidence: 0.85
    }
}
```

### Invalid Examples

```nushell
# Missing payload field
{ result: "done" }
# Error: Script must return a record with 'payload' field

# Not a record
"just a string"
# Error: Script must return a record

# signals not a record
{ payload: "ok", signals: ["a", "b"] }
# Error: 'signals' field must be a record
```

## Environment Variables

The runtime exposes these env vars to every Task:

| Variable | Description | Example |
|----------|-------------|---------|
| `OUT_DIR` | Where to write artifacts and logs | `/tmp/playbook/123/stage-1/step-1/task-1/out` |
| `IN_ARTIFACT` | Path to previous Task's artifact | `/tmp/playbook/123/.../task-0/out/artifact.json` |
| `PLAYBOOK_ID` | Current Playbook identifier | `build-resume-playbook-123` |
| `STAGE_ID` | Current Stage identifier | `stage-1` |
| `STEP_ID` | Current Step identifier | `build-resume-step` |
| `TASK_ID` | Current Task identifier | `format-resume` |
| `PROMPT_ID` | Prompt template ID (when using conduit) | `resume-data-extractor.v1` |
| `CONDUIT_OPTIONS` | JSON blob for provider/model options | `{"model":"gpt-4"}` |
| `DAPPLE_METADATA` | Runtime metadata with `conduit_url` | `{"conduit_url":"http://..."}` |

### Accessing Environment

```nushell
# Bind the pipeline input once at the top of the script
let input = $in

# Access metadata (includes conduit_url)
let meta = $env.DAPPLE_METADATA

# Access specific fields
let out_dir = $env.OUT_DIR
let task_id = $env.TASK_ID
```

## Input/Output Data Flow

### Input Methods

Tasks receive input in multiple ways:

```nushell
# Method 1: Pipeline input (recommended)
# The script receives input as pipeline data
$in | each { |item| ... }

# Method 2: Bind the pipeline once, then reference the binding
# (required when $in is read more than once — $in is consumed on first use)
let input = $in

# Method 3: Previous artifact file
let prev = open $env.IN_ARTIFACT
let data = open $prev.path
```

### Output Methods

Tasks produce output via the contract return value:

```nushell
def main [] {
    # Process input
    let result = $in | process_data

    # Return contract-compliant output
    {
        payload: $result,
        signals: {
            status: "complete"
        }
    }
}
```

## Task Invocation

What the runtime does when calling a Task:

```bash
# Runtime sets environment before calling the task
export OUT_DIR=/tmp/playbook/123/stage-1/step-1/task-1/out
export IN_ARTIFACT=/tmp/playbook/123/stage-1/step-1/task-0/out/artifact.json
export PLAYBOOK_ID=build-resume-playbook-123
export STAGE_ID=stage-1
export STEP_ID=build-resume-step
export TASK_ID=format-resume

# Runtime invokes Task via StepEngine
# Input data arrives as $in (the pipeline)
# Metadata passed via DAPPLE_METADATA

# Runtime collects:
#   - payload from script output
#   - signals from script output
#   - $OUT_DIR/artifact.json (optional, for next Task)
#   - $OUT_DIR/log.jsonl (optional, for tracing)
#   - $OUT_DIR/error.json (if task fails)
```

## Error Handling

### Script Errors

| Error Type | Cause | Recovery |
|------------|-------|----------|
| `Parse` | Syntax error in script | Fix script syntax; error cached by hash |
| `Security` | Blocked command used | Remove disallowed command |
| `Runtime` | Execution error | Check script logic |
| `ContractViolation` | Invalid output format | Return `{ payload, signals }` |

### Writing error.json

If a Task fails, write `error.json` with helpful diagnostics:

```nushell
def main [] {
    try {
        let result = do_work
        { payload: $result, signals: {} }
    } catch { |err|
        # Write error file
        {
            message: $err.msg,
            code: 500,
            details: ($err | to json)
        } | to json | save $"($env.OUT_DIR)/error.json"

        # Still need to return valid contract for clean exit
        { payload: null, signals: { error: true } }
    }
}
```

## Failure Semantics

A Step is an ordered set of Tasks executed as one **fail-together** unit (a coherence and retry boundary, not a database transaction — nothing rolls back; task attempts, payloads, and assets are always recorded as history). The Tasks of a Step run sequentially in authored order.

### Task Failure

If a Task exits non-zero or throws an error:

1. Runtime captures `error.json` (if present)
2. The Step **stops on this first failure** — later Tasks in the Step do **not** run (it is not log-and-continue)
3. The Step is marked failed and its computed outputs are **not** published (outputs publish once, together, only when the whole Step coheres)

### Unsafe Veto

A Task may **veto** the Step by emitting `signals.unsafe = true`. This puts the dispatch in the terminal `unsafe` phase (do-not-refire, non-retryable) — distinct from an ordinary `failed`, which is retryable.

### Retry Behavior

Retry is **re-dispatch of the whole Step** (latest-wins across dispatches), not per-Task retry. Because the Step re-runs from the start, design Tasks to be:

- **Idempotent** — Safe to re-run with same inputs
- **Resumable** — Can detect partial work and continue

### Long-running Tasks

If a Task may run for more than a few seconds:

- Write periodic log entries to indicate liveness
- Consider writing a `pending.json` for async operations
- Avoid blocking indefinitely without output
