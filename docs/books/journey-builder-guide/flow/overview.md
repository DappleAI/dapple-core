# Flow Engine Overview

> **Audience:** Backend Developers · Platform Engineers

The Flow Engine (`dapple-prompt-flow`) is responsible for executing workflows composed of Tasks, Steps, and Stages.

> **Two engines named "Engine" — do not conflate them.** This crate uses both:
>
> 1. **`StepEngine`** — the Nushell script execution engine in this crate. Parses scripts, applies security policies, runs them, returns `{ payload, signals }`.
> 2. **`dataflow_rs::Engine`** — the workflow orchestration framework from the `dataflow-rs` crate. Processes JSONLogic-driven workflows; configured via JSON definitions; stateless and created per dispatch.
>
> Same word, different jobs. The chapters in this `flow/` directory describe both. The canonical distinction lives in [`crates/dapple-prompt-flow/AGENTS.md`](../../../../crates/dapple-prompt-flow/AGENTS.md).

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

## Execution Model

1. **Playbook is started** by a user action. The runtime creates a Playbook instance and executes Stages in order.
2. **For each Stage**, the runtime executes its Setup Steps. Each Step runs its Tasks in the declared order as one **fail-together** unit — the Step stops on the first task failure (later Tasks do not run; it is not log-and-continue). A Task may **veto** the Step via `signals.unsafe = true`, ending the dispatch in the terminal `unsafe` phase (do-not-refire).
3. **Tasks execute via the StepEngine**. The runtime provides environment variables, input data, and metadata, then collects artifacts and logs after each Task completes.
4. **After a Step completes coherently**, the runtime publishes the Step's computed outputs once, together (a failed or vetoed Step publishes none), and may create `checkpoint.json` and surface a UI action (for human review) if configured. Retry is re-dispatch of the whole Step (latest-wins).

## Key Components

| Component | Purpose |
|-----------|---------|
| StepEngine | Executes Nushell Tasks in sandboxed environment |
| NuExecutor | Parses and runs Nushell scripts |
| DataflowManager | Coordinates data flow between tasks |
| ArtifactStore | Manages task outputs and inputs |

## Crate Structure

```
dapple-prompt-flow/
├── src/
│   ├── engine/
│   │   ├── mod.rs          # StepEngine
│   │   ├── executor.rs     # NuExecutor
│   │   └── security.rs     # Security policies
│   ├── dataflow/
│   │   └── mod.rs          # Dataflow integration
│   └── lib.rs
```

## Next Steps

- [Nushell Execution](./nushell-execution.md) — Script contract and data flow
- [StepEngine Internals](./step-engine.md) — Parser, cache, validation
- [Security Model](./security-model.md) — Sandboxing and allowed commands
