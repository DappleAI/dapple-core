# Plan: Normalization of Nushell Command in dapple-prompt-flow

**SHA-1**: `35701c7`
**Status**: Complete
**Created**: 2026-01-16
**Updated**: 2026-01-16

### Implementation Notes

**Phase 1 - Core Implementation (Complete):**
- Added nu-engine, nu-protocol, nu-parser, nu-cmd-lang, sha2, thiserror, base64 dependencies
- Created `engine/mod.rs` with `StepEngine` struct, parse caching, JSON/Nu conversion
- Created `engine/security.rs` with `SecurityPolicy` and AST validation
- Refactored `nu_handler.rs` to use `StepEngine` with `Arc<RwLock<>>` for shared access
- Updated integration tests with new `{ payload, signals }` contract

**Phase 2 - Follow-up (Complete):**
- Added `SecurityMode` enum with `Permissive` and `Allowlist` variants
- Changed default security policy to permissive (allows all internal commands)
- Added `error_cache` to `StepEngine` to cache parse errors
- Created `engine/plugins.rs` with documentation for plugin support
- All 10 unit tests pass

---

## Summary

Replace the stub `NuFunction` implementation with real Nushell engine integration using `nu-engine`, `nu-protocol`, `nu-parser`, and `nu-cmd-lang` crates. Implement the `StepEngine` pattern with parse caching and enforce the security model defined in project specifications.

---

## Current State

### Architecture Overview

The workflow engine uses `dataflow-rs` with custom function handlers:

```
Engine (dataflow-rs)
  └── Workflow[]
       └── Task[]
            └── FunctionConfig { name: "nu", input: {...} }
                     │
                     ▼
            AsyncFunctionHandler::execute()
```

### Key Files

**`crates/dapple-prompt-flow/src/func/nu_handler.rs`** (113 lines)

Current stub implementation that only mocks basic commands:

```rust
pub struct NuFunction {
    pool: Option<DbPool>,
}

impl NuFunction {
    // Stub pipeline executor - only supports echo, str collect, trim
    fn run_pipeline(&self, script: &str, input: &str) -> String {
        let parts: Vec<&str> = script.split('|').map(|s| s.trim()).collect();
        let mut current = input.to_string();
        for p in parts.iter() {
            if p.starts_with("echo ") {
                let rest = p.trim_start_matches("echo ").trim();
                let trimmed = rest.trim_matches('\'').trim_matches('"');
                current = trimmed.to_string();
            } else if p.starts_with("str collect") {
                // noop
            } else if p == &"trim" {
                current = current.trim().to_string();
            }
        }
        current
    }
}
```

**`crates/dapple-prompt-flow/src/func/handler_adapter.rs`** (32 lines)

Bridges Arc-based function maps to Box-based engine API.

**`crates/dapple-prompt-flow/src/dispatch_runner.rs`** (142 lines)

Orchestrates execution: creates engine per task, calls `engine.process_message()`.

**`crates/dapple-prompt-flow/tests/dispatch_nushell_pipeline_integration.rs`** (254 lines)

Integration test that:
1. Creates tasks with `function: "nu"` and JSON `function_input`
2. Registers `NuFunction` in function map
3. Runs dispatch via `run_dispatch_from_dto()`
4. Verifies output in DB events

### Current Limitations

1. **No real Nushell execution** - just string parsing for `echo`, `str collect`, `trim`
2. **No parse caching** - scripts parsed every execution
3. **No security enforcement** - no command allowlist, no `$env` protection
4. **No script contract** - doesn't enforce `{ payload, signals }` return format
5. **Only `data.*` namespace** - doesn't support `metadata.*` or `temp_data.*` properly

---

## Desired State

### Architecture

```
StepEngine (per worker/step)
  ├── nu_engine::Engine
  ├── ParseCache (by script SHA-256)
  └── SecurityPolicy
       └── CommandAllowlist

NuFunction::execute()
  │
  ├── 1. Validate script against SecurityPolicy
  ├── 2. Check ParseCache or parse script
  ├── 3. Inject variables ($INPUT, $env.metadata)
  ├── 4. Execute via nu_engine
  ├── 5. Extract { payload, signals } from result
  └── 6. Write to message namespaces
```

### New Components

**`StepEngine`** - Owns Nushell engine instance and parse cache:

```rust
pub struct StepEngine {
    engine_state: Arc<nu_protocol::engine::EngineState>,
    parse_cache: HashMap<[u8; 32], Arc<nu_protocol::ast::Block>>,
    security_policy: SecurityPolicy,
}

impl StepEngine {
    pub fn new() -> Self;
    pub fn execute_script(
        &mut self,
        script: &str,
        input: Value,
        metadata: Option<Value>,
    ) -> Result<NuOutput, NuError>;
}
```

**`NuOutput`** - Enforced return contract:

```rust
pub struct NuOutput {
    pub payload: Value,
    pub signals: HashMap<String, Value>,
}
```

**`SecurityPolicy`** - Command validation:

```rust
pub struct SecurityPolicy {
    allowed_commands: HashSet<String>,
    max_metadata_size: usize, // e.g., 512KB
}

impl SecurityPolicy {
    pub fn validate_ast(&self, block: &Block) -> Result<(), SecurityError>;
}
```

### Script Contract

Scripts MUST return a record with this shape:
```nushell
{
    payload: <any>,
    signals: { key1: value1, key2: value2, ... }
}
```

Handler extracts and writes:
- `payload` → `data.<base>.json`
- `signals` → `metadata.signals.<task_id>.*`

---

## Implementation Steps

### Phase 1: Add Dependencies

1. Add nu-* crates to `crates/dapple-prompt-flow/Cargo.toml`:
   ```toml
   [dependencies]
   nu-engine = "0.100"      # or latest compatible
   nu-protocol = "0.100"
   nu-parser = "0.100"
   nu-cmd-lang = "0.100"
   sha2 = "0.10"            # for parse cache keys
   ```

### Phase 2: Implement StepEngine

2. Create `crates/dapple-prompt-flow/src/engine/mod.rs`:
   - `StepEngine` struct with engine state and parse cache
   - `execute_script()` method
   - Parse cache using SHA-256 of script text

3. Create `crates/dapple-prompt-flow/src/engine/security.rs`:
   - `SecurityPolicy` struct
   - `validate_ast()` for command allowlist checking
   - Default policy with safe commands only

### Phase 3: Update NuFunction

4. Refactor `crates/dapple-prompt-flow/src/func/nu_handler.rs`:
   - Replace `run_pipeline()` stub with `StepEngine::execute_script()`
   - Add `NuOutput` struct for return contract
   - Extract `payload` and `signals` from script result
   - Write to appropriate message namespaces

### Phase 4: Update Dispatch Runner

5. Modify `crates/dapple-prompt-flow/src/dispatch_runner.rs`:
   - Create `StepEngine` per worker (not per task)
   - Pass engine reference to `NuFunction`
   - Handle `NuError` types properly

### Phase 5: Tests

6. Update `tests/dispatch_nushell_pipeline_integration.rs`:
   - Change test scripts to return `{ payload, signals }` format
   - Add tests for security policy violations
   - Add tests for parse cache hits

7. Add unit tests for:
   - `StepEngine` parse caching
   - `SecurityPolicy` validation
   - `NuOutput` extraction

---

## Files to Modify

| File | Action |
|------|--------|
| `crates/dapple-prompt-flow/Cargo.toml` | Add nu-*, sha2 dependencies |
| `crates/dapple-prompt-flow/src/lib.rs` | Add `engine` module export |
| `crates/dapple-prompt-flow/src/engine/mod.rs` | **NEW** - StepEngine impl |
| `crates/dapple-prompt-flow/src/engine/security.rs` | **NEW** - SecurityPolicy |
| `crates/dapple-prompt-flow/src/func/nu_handler.rs` | Replace stub with real impl |
| `crates/dapple-prompt-flow/src/dispatch_runner.rs` | Use StepEngine per worker |
| `tests/dispatch_nushell_pipeline_integration.rs` | Update test scripts |

---

## Verification

1. **Build**: `cargo build -p dapple-prompt-flow`

2. **Unit tests**: `cargo test -p dapple-prompt-flow`

3. **Integration test**:
   ```bash
   cargo test --test dispatch_nushell_pipeline_integration
   ```

4. **Manual verification**:
   - Script returning `{ payload: "hello", signals: { done: true } }` should:
     - Write `"hello"` to `data.<output_path>`
     - Write `true` to `metadata.signals.<task_id>.done`

5. **Security test**:
   - Script with `exec ls` should fail at parse time with `SecurityError`

---

## Open Questions (All Resolved)

1. **Nu version**: Latest (0.100) - RESOLVED
2. **Command allowlist**: Permissive mode (allow all internal commands) - RESOLVED
3. **Metadata size cap**: 512KB - RESOLVED
4. **Error recovery**: Parse errors are now cached - RESOLVED
5. **Plugin support**: Documentation added, no plugins enabled by default - RESOLVED
