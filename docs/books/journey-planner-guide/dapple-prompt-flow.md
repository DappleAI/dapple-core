# Task Execution Runtime

> **Audience:** :wrench: Prompt Engineers
>
> *Product readers can skip this chapter.*

This chapter explains the runtime flow authors can expect when their Tasks, Steps, and Playbooks run under the dapple runtime.

---

## From Guide Directives to Runtime Tasks

When a guide is ingested, `:::nushell` directive blocks become Task entities. At dispatch time, the runtime executes these scripts. The script body from the directive block is stored in the task's `function_inputs` and loaded by the engine at execution time.

For example, this guide directive:

```markdown
:::nushell { name: "collect-data" }
let input = $in
{ payload: ($input | select name email), signals: {} }
:::
```

becomes a Task named `collect-data` in the database. When the corresponding Step runs during a dispatch, the engine retrieves the script, validates it against the security policy, and executes it.

---

## High-level Flow

```mermaid
flowchart TD
    A[User starts Playbook] --> B[Runtime creates instance]
    B --> C[Execute Stage 1]
    C --> D[Execute Step 1.1]
    D --> E[Run Task 1.1.1]
    E --> F[Run Task 1.1.2]
    F --> G{More Steps?}
    G --|Yes|> D
    G --|No|> H{Human Review?}
    H --|Yes|> I[Wait for approval]
    I --> J{Approved?}
    J --|Yes|> K{More Stages?}
    J --|No|> L[Mark failed]
    H --|No|> K
    K --|Yes|> C
    K --|No|> M[Playbook complete]
```

1. **Playbook is started** by a user action. The runtime creates a Playbook instance and executes Stages in order.
2. **For each Stage**, the runtime executes its Setup Steps. Each Step runs Tasks in the declared order.
3. **Tasks execute** with environment variables, input data, and metadata. The runtime collects output and logs after each Task completes.
4. **After a Step completes**, the runtime may surface a UI action (for human review) if configured.

---

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

---

## Data Namespaces

Scripts and workflows operate on three namespaces:

| Namespace | Purpose | Persistence |
|-----------|---------|-------------|
| `data.*` | Canonical payloads from tasks | Persisted to JSONB |
| `metadata.*` | Control-plane state, signals | Persisted to JSONB |
| `temp_data.*` | Engine scratch space | Not persisted |

---

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
| `DAPPLE_METADATA` | Runtime metadata with `conduit_url` | `{"conduit_url":"http://..."}` |

**Always write artifacts to `OUT_DIR`** -- the runtime will not find them elsewhere.

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

---

## Security Policy

The runtime enforces security policies to prevent dangerous operations.

### Security Modes

| Mode | Description |
|------|-------------|
| **Permissive** (default) | Allows all internal Nushell commands; blocks external commands |
| **Allowlist** | Only allows explicitly listed commands |

### Always Blocked

Regardless of mode, these are always blocked:

| Operation | Error |
|-----------|-------|
| External commands (`^curl`, `^ls`) | `ExternalCommandNotAllowed` |
| Environment mutation | `EnvMutationNotAllowed` |
| Plugin commands | `PluginNotAllowed` |

### Allowlist Commands (Standard)

When using allowlist mode, these commands are permitted:

**Core Language:**
- `if`, `else`, `match`, `for`, `while`, `loop`
- `break`, `continue`, `return`, `try`, `catch`
- `def`, `let`, `mut`, `const`

**Data Manipulation:**
- `echo`, `print`, `get`, `select`, `reject`, `where`, `filter`
- `each`, `par-each`, `reduce`, `fold`, `flatten`, `transpose`
- `group-by`, `sort-by`, `uniq`, `uniq-by`
- `first`, `last`, `take`, `skip`, `drop`, `reverse`
- `length`, `range`, `enumerate`, `zip`, `merge`
- `insert`, `update`, `upsert`, `append`, `prepend`
- `wrap`, `unwrap`, `default`, `compact`

**String Operations:**
- `str`, `str join`, `str trim`, `str length`, `str replace`
- `str substring`, `str contains`, `str starts-with`, `str ends-with`
- `str index-of`, `str upcase`, `str downcase`, `str capitalize`
- `split`, `split row`, `split column`, `lines`, `parse`, `format`, `fill`

**Type Conversions:**
- `into`, `into string`, `into int`, `into float`, `into bool`
- `into record`, `into binary`

**JSON/Data Format:**
- `from json`, `to json`, `from csv`, `to csv`
- `from yaml`, `to yaml`, `from toml`, `to toml`

**Math:**
- `math`, `math abs`, `math ceil`, `math floor`, `math round`
- `math sqrt`, `math sum`, `math avg`, `math min`, `math max`

**Comparison/Logic:**
- `is-empty`, `is-not-empty`, `all`, `any`, `not`

**Date/Time:**
- `date now`, `date format`, `date to-record`

**Utility:**
- `describe`, `metadata`, `debug`, `error make`, `null`, `ignore`, `collect`
- `headers`, `values`, `columns`, `items`

**Dapple Commands:**
- `prompt`, `prompt send`, `prompt retry`, `prompt reset`
- `madlib`, `summarize`, `json-schema`

---

## Custom Commands

The runtime provides custom Nushell commands beyond standard Nushell. See [Custom Nushell Commands](./custom-nu-commands.md) for detailed documentation.

### Prompt Commands

| Command | Description |
|---------|-------------|
| `prompt send` | Send a prompt to the conduit service |
| `prompt retry` | Retry a failed prompt with same context |
| `prompt reset` | Reset prompt conversation state |

### AI-Powered Commands

| Command | Description |
|---------|-------------|
| `madlib` | Fill `[MASK]` tokens using ModernBERT |
| `summarize` | Summarize text using T5 model |
| `json-schema` | Infer JSON Schema from data (local, no backend) |

---

## Input/Output Data Flow

### Input Methods

Tasks receive input in multiple ways:

```nushell
# Method 1: Pipeline input (recommended)
$in | each { |item| ... }

# Method 2: Bind the pipeline once, then reference the binding
# (required when the input is read more than once)
let input = $in

# Method 3: Previous artifact file
let prev = open $env.IN_ARTIFACT
let data = open $prev.path
```

### Output Methods

Tasks produce output via the contract return value:

```nushell
def main [] {
    let result = $in | process_data

    {
        payload: $result,
        signals: {
            status: "complete"
        }
    }
}
```

---

## Error Handling

### Script Errors

| Error Type | Cause | Recovery |
|------------|-------|----------|
| `Parse` | Syntax error in script | Fix script syntax |
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
        {
            message: $err.msg,
            code: 500,
            details: ($err | to json)
        } | to json | save $"($env.OUT_DIR)/error.json"

        { payload: null, signals: { error: true } }
    }
}
```

---

## Failure Semantics

### Task Failure

If a Task exits non-zero or throws an error:

1. Runtime captures `error.json` (if present)
2. Step is marked as failed
3. Playbook may retry or halt depending on directive configuration

### Retry Behavior

The runtime may retry failed Tasks depending on Step configuration. Design Tasks to be:

- **Idempotent** -- Safe to re-run with same inputs
- **Resumable** -- Can detect partial work and continue

### Long-running Tasks

If a Task may run for more than a few seconds:

- Write periodic log entries to indicate liveness
- Consider writing a `pending.json` for async operations
- Avoid blocking indefinitely without output

---

## Human Review Hooks

Authors can configure a Step to pause after completion and wait for human approval:

1. Runtime writes `checkpoint.json` with Step results
2. UI surfaces approve/reject controls
3. User reviews and approves
4. Runtime writes approval to `checkpoint.json`
5. Playbook proceeds to next Stage

```json
{
  "status": "pending_review",
  "step": "build-resume-step",
  "artifacts": ["resume.md", "resume.pdf"],
  "requested_at": "2024-01-15T10:30:00Z"
}
```

After approval:

```json
{
  "status": "approved",
  "step": "build-resume-step",
  "reviewer": "user@example.com",
  "approved_at": "2024-01-15T10:35:00Z",
  "notes": "Looks good, proceed."
}
```

---

## Best Practices

### Script Design

1. **Always return the contract** -- `{ payload, signals }`
2. **Use signals for metadata** -- Don't embed control data in payload
3. **Handle errors gracefully** -- Write `error.json` before failing
4. **Keep scripts focused** -- One task per script

### Performance

1. **Avoid external commands** -- Use Nushell built-ins
2. **Stream large data** -- Don't load everything into memory

### Debugging

1. **Use `debug` command** -- Print intermediate values
2. **Check `DAPPLE_METADATA`** -- Contains runtime context
3. **Write to log.jsonl** -- Structured logs for tracing
