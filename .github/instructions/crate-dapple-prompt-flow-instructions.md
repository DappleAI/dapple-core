---
applyTo: "crates/dapple-prompt-api/src/**/*.rs"
title: "Dapple Prompt Flow — Guide"
description: "Concise, authoritative instructions for implementing the prompt-flow / workflow service (Nushell + dataflow-rs)."
---

## Glossary (canonical terms)

- **Canonical payload**: `data.<base>`
- **Signals**: `metadata.signals.*`
- **Validation inputs**: `metadata.validation.inputs.*`
- **Validation results**: `metadata.validation.errors` / `metadata.validation.warnings`
- **StepEngine**: per-step embedded Nushell engine + parse cache
- **Principal / Owned / Shared dependents**: follow dapple-data naming conventions (FlatXModel / InsertedXModel / PublicXModel / InsertXArgs / CreateXSchema)

## High-level goals

- Deterministic execution, strict auditability, and explicit control flow.
- No hidden state, no implicit mutations, avoid format drift.

## Canonical storage & data model

- **JSON is canonical**. Durable outputs stored via dapple-data as JSONB (e.g., `DispatchTaskAttemptEvent.context_data` / `context_metadata`).
- `data.*` holds canonical payloads (written only by Nu pipeline tasks).
- `metadata.*` holds control-plane state, signals, validation outputs, routing flags (written by `map_payload`, `verify_payload`, and Nu signal emission).
- `temp_data.*` is engine scratch only; Nu tasks must not read/write `temp_data` and must not rely on it across tasks.

## Nushell execution model

- Embedded via nu-cmd-lang, nu-engine, nu-parser, nu-protocol.
- One **StepEngine per worker / per step**. Parse cache by script SHA.
- Execution contract: Nu script **MUST return a record** `{ payload, signals }`.

## Nu script contract (handler behavior)

- **Inputs**: `script_id` (UUID), `vars_from` (map of var->dot-path), optional `env_from`, `cwd`, `write.base_path`, `write.signals_into`, `inject_metadata`.
- Handler encodes payload → `data.<base>.json` and writes provenance → `data.<base>._provenance`.
- Signals are fanned out to `metadata.signals.<task_id>.*` (small JSON-safe scalars).
- Enforce size cap when injecting `$env.metadata` (e.g., 512KB).

## Security & policy (hard rules)

- No external commands, no plugin management commands, no `$env` mutation.
- Only allow-listed plugins (global policy). Scripts must not alter plugin state.
- Reject scripts violating rules at parse time with clear `DataflowError` types.

## Task types and responsibilities

1. **Nu pipeline task (`nu`)**

   - **Writes**: `data.<base>.json`, `data.<base>._provenance`, `metadata.signals.*`
   - Must not write metadata outside designated namespaces (except signals).

2. **map_payload**

   - **Reads**: `data.*`, `metadata.*`
   - **Writes**: `metadata.*` only (populate validation.inputs, routing flags)
   - Uses JSONLogic evaluation root: `{ payload, metadata, ...context }`

3. **verify_payload**

   - **Reads**: `data.<base>.json` and `metadata.*`
   - **Writes**: `metadata.validation.<namespace>.*` with `.ok` boolean, `.errors[]`, per-rule booleans

4. **Regular built-in tasks (dataflow-rs)**

   - Avoid allowing them to mutate canonical `data.<base>.json` unless intended.

## Workflow control pattern (recommended)

- Typical loop:

  1. Optional `map_payload` (pre)
  2. `nu` task → writes canonical payload + signals
  3. `map_payload` (post) → promote values to metadata
  4. `verify_payload` → set `metadata.validation.<ns>.ok` and errors
  5. condition (reads metadata only) → choose next step or error handler

- Conditions **MUST** read metadata only. Never inspect `data.*`.

## SQL / DB / dapple-data coordination notes

- Prefer reusing SQL assets under `crates/dapple-data/assets/sql` rather than inline strings.
- When adding SQL assets, add matching functions under `crates/dapple-data/src/query/<area>.rs` and corresponding models.
- Column-wise `UNNEST` with `WITH ORDINALITY` is the canonical pattern for multi-row inserts to preserve ordering. Build `InsertXArgs` with vectors of identical lengths and `UNNEST(... ) WITH ORDINALITY`, then `ORDER BY args.ord`.
- Migrations live in `crates/dapple-data/migrations/*.sql` — edit them following `ddl.instructions.md` rules.

## Testing & lifecycle patterns

- Tests create named batches stored in `ScenarioContext`; first-batch name pattern: "{endpoint}-init".
- Wait explicitly for required shared dependents by polling `ScenarioContext` (avoid sleeping).
- Log `CreateXSchema` payload, `InsertXArgs` lengths, and SQL-returned rows when diagnosing failures.
- For any new schema/principal object: update migrations, SQL assets, dapple-data models & queries, prompt-api handlers, and lifecycle tests.

## Auditing & persistence

- Record `workflow_runs`, `task_executions` (inputs, timings, status/errors, outputs), `task_changes` (path/old/new), `run_results`, and optional `run_snapshots`.
- Every mutation produces a Change record. Store provenance alongside payloads.

## Error handling

- Return structured `DataflowError` with clear types: Validation, Execution, etc.
- Include `script_id`, `base_path`, and rule names as context in errors.
- No panics or `unwrap()` in production handlers.

## Developer & build notes (brief)

- Follow repo conventions: snake_case for functions/modules, PascalCase for types.
- Use workspace dependencies; pin versions where appropriate.
- Run `cargo fmt` and `cargo clippy`; build/test with `cargo build` / `cargo test` as usual.
- SQLx: set `SQLX_OFFLINE` for offline verification; run `cargo sqlx prepare` when changing .sql assets.

## Non-goals

- NUON persistence, external command execution, implicit dataflow between tasks, automatic schema inference, hidden side effects.

## Open items / TODOs

- Confirm frontmatter.applyTo target (prompt-flow vs prompt-api).
- Decide whether to split the Copilot-specific rules into .github/copilot-instructions.md (recommended).
- Policy: confirm `$env.metadata` size cap value and plugin allow-list mechanism.