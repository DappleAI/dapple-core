# dapple-prompt-flow

Workflow engine for Dapple Prompt, providing Nushell-based task execution, directive-driven flow control, and security policies.

## Overview

The `dapple-prompt-flow` crate implements a secure workflow execution engine that:
- Executes Nushell scripts as workflow tasks
- Evaluates directives for conditional execution, retry, and branching
- Integrates with `dataflow-rs` for workflow orchestration
- Enforces security policies to prevent unauthorized operations
- Provides JSON-to-Nushell value conversion

## Key Components

### StepEngine (Nushell Execution)

The core execution engine for Nushell scripts. Each worker/step gets its own `StepEngine` instance with script caching.

```rust
use dapple_prompt_flow::engine::StepEngine;

let mut engine = StepEngine::new();

let result = engine.execute_script(
    r#"{ payload: ($in.name | str upcase), signals: {} }"#,
    serde_json::json!({ "name": "test" }),
    None,
)?;

println!("Payload: {:?}", result.payload);   // "TEST"
println!("Signals: {:?}", result.signals);   // {}
```

### dataflow Engine (Workflow Orchestration)

The crate also uses `dataflow_rs::Engine` for workflow orchestration with built-in tasks (map, validate) and custom function handlers. Note: This is a different "Engine" from StepEngine.

```rust
use dapple_prompt_flow::Step;
use dataflow_rs::Workflow;

let step = Step {
    workflows: vec![workflow1, workflow2],
};

// Get a dataflow engine with default custom functions
let engine = step.engine();

// Or with custom function handlers
let engine = step.engine_with_functions(custom_handlers);
```

### Script Contract

**All scripts MUST return a record with `payload` and `signals` fields:**

```nushell
# Valid script
{ payload: "result data", signals: { completed: true } }

# Also valid - signals can be empty
{ payload: $in | process, signals: {} }

# INVALID - missing required fields
"just a string"
```

- **`payload`**: The task output (any JSON-serializable value)
- **`signals`**: Control-plane metadata (HashMap<String, JsonValue>)

### Security Policy

Scripts run in a sandboxed environment with configurable security policies.

```rust
use dapple_prompt_flow::engine::{StepEngine, SecurityPolicy};

// Permissive mode (default) - allows all internal Nushell commands
let engine = StepEngine::new();

// Standard allowlist - curated set of safe commands
let engine = StepEngine::with_policy(SecurityPolicy::new_with_standard_allowlist());

// Custom allowlist
let mut allowed = HashSet::new();
allowed.insert("echo".to_string());
allowed.insert("str".to_string());
let engine = StepEngine::with_policy(
    SecurityPolicy::new_with_allowlist(allowed, 512 * 1024)
);
```

**Security Rules:**
- No external commands (shell out, exec)
- No environment mutation
- No plugin management
- Metadata size limits enforced
- Parse-time validation with cached errors

## Directives

Directives provide conditional execution, retry, and branching logic for playbook stages and steps using JSONLogic expressions.

### The `run` Operator

Every directive contains a `{"run": []}` operator that executes the stage/step:

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "ok"] },
    "proceed",
    "fail"
  ]
}
```

The `run` operator returns:
```json
{
  "status": "ok" | "error" | "timeout" | "canceled",
  "error_code": "RATE_LIMITED" | "TIMEOUT" | null,
  "data": { ... },
  "signals": { ... },
  "duration_ms": 1234
}
```

### Action Values

| Action | Description |
|--------|-------------|
| `"proceed"` | Continue to next stage/step |
| `"skip"` | Skip without execution |
| `"fail"` | Halt with error |
| `"retry"` | Re-execute after `retry_delay_ms` |
| `"loop"` | Re-execute immediately |
| `"{uuid}"` | Branch to stage/step with that object_id |

### Example: Retry on Transient Failure

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "ok"] },
    "proceed",
    { "if": [
      { "in": [{ "var": "run.error_code" }, ["RATE_LIMITED", "TIMEOUT"]] },
      "retry",
      "fail"
    ]}
  ]
}
```

See `docs/plans/flow-logic-dapple-flow.md` for full directive architecture.

## Data Namespaces

Scripts and directives work with three data namespaces:

| Namespace | Purpose | Persistence |
|-----------|---------|-------------|
| `data.*` | Canonical payloads from tasks | Persisted to JSONB |
| `metadata.*` | Control-plane state, signals | Persisted to JSONB |
| `temp_data.*` | Engine scratch space | Not persisted |

## Task Types

1. **Nu pipeline task** - Executes Nushell script, writes to `data.<base>.json`
2. **map_payload** - JSONLogic evaluation, reads/writes `metadata.*`
3. **verify_payload** - Validation, writes to `metadata.validation.*`
4. **Built-in tasks** - Avoid mutating `data.<base>.json`

### Control Flow Pattern

```
map_payload (pre) -> nu task -> map_payload (post) -> verify_payload -> condition
```

## Input/Output

Scripts receive input via pipeline (`$in`) and environment variables:

```nushell
# Access input via pipeline
let data = $in
let name = $data.user.name

# Bind the pipeline input once, then reference the binding
let input = $in
let meta = $env.DAPPLE_METADATA
```

## Module Structure

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

## Error Types

```rust
pub enum NuError {
    Security(SecurityError),    // Policy violation
    Parse(String),              // Nushell parse error (cached)
    Runtime(String),            // Execution error
    ContractViolation(String),  // Missing payload/signals
}

pub enum DirectiveError {
    InvalidAction(String),      // Unknown action string
    EvaluationFailed(String),   // JSONLogic evaluation error
    ExecutionFailed(String),    // Run handler failed
    BranchTargetNotFound(Uuid), // Branch target doesn't exist
    BranchOutOfScope,           // Step tried to branch outside stage
}
```

## Performance

- **Script caching**: Parse errors cached by SHA-256 hash
- **One engine per worker**: Avoid shared state between concurrent executions
- **Delta merging**: Closures and blocks properly handled via working set merges
- **Directive memoization**: `run` executes at most once per evaluation

## Custom Nushell Commands

The crate provides custom Nushell commands for prompt workflows:

| Command | Description |
|---------|-------------|
| `prompt` | Parent command for prompt operations |
| `prompt send` | Send a prompt to the conduit service |
| `prompt retry` | Retry a failed prompt |
| `prompt reset` | Reset prompt state |
| `summarize` | Summarize text content |
| `madlib` | Fill in template placeholders |
| `json-schema` | Generate JSON schema from data |

## Dependencies

Key dependencies:
- `nu-protocol`, `nu-parser`, `nu-engine`, `nu-command`, `nu-cmd-lang` - Nushell runtime
- `dataflow-rs` - Workflow orchestration
- `datalogic-rs` - JSONLogic evaluation
- `dapple-data` - Database queries and models
- `reqwest` - HTTP client for conduit service
- `serde_json` - JSON handling
- `sha2` - Script hashing
- `handlebars` - Template rendering

## Testing

```bash
# Run unit tests
cargo test -p dapple-prompt-flow

# Run with logging
RUST_LOG=debug cargo test -p dapple-prompt-flow
```

## References

- [AGENTS.md](./AGENTS.md) - Agent-specific instructions for this crate
- [CLAUDE.md](../../CLAUDE.md) - "Workflow / Prompt Flow" section
- [Flow Logic Plan](../../docs/plans/flow-logic-dapple-flow.md) - Directive architecture
- [Nushell Documentation](https://www.nushell.sh/book/)
- [dataflow-rs](https://docs.rs/dataflow-rs/)
- [JSONLogic](https://jsonlogic.com/)
