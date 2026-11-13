# Flow Logic Engine Architecture Plan

**Status:** Draft
**Created:** 2026-01-22
**Last Updated:** 2026-01-22
**Target Version:** 0.0.3
**Dependencies:**
- `docs/plans/flow-logic-tables.md` (database schema and API)
- Existing `dapple-prompt-flow` crate infrastructure
- dataflow-rs and datalogic-rs integrations

## Executive Summary

This plan defines the runtime implementation of flow logic (directives) within the `dapple-prompt-flow` crate. Building on the database schema from `flow-logic-tables.md`, this plan covers:

- **Directive evaluation** using JSONLogic with a custom `run` operator
- **Integration with dispatch_runner** for stage/step execution with directives
- **The `run` primitive** implementation that executes tasks and captures results
- **Action handling** for proceed, skip, fail, retry, loop, and branch operations

The directive system enables conditional execution, retry logic, polling patterns, and branching without modifying playbook structure. This transforms the flow engine from a linear executor to a dynamic, condition-driven orchestrator.

## Context and Background

### Current State

The `dapple-prompt-flow` crate currently provides:

1. **StepEngine** (`engine/mod.rs`) - Nushell script execution with security policies
2. **dispatch_runner** (`dispatch_runner.rs`) - Linear execution of dispatch stages/steps/tasks
3. **dataflow-rs integration** - Workflow engine with built-in functions (map, validate)
4. **Custom Nushell commands** - `prompt send`, `prompt retry`, `summarize`, `madlib`, etc.

The current `dispatch_runner.run_dispatch_from_dto` executes tasks linearly:
```
for stage in stages:
    for step in steps:
        for task in tasks:
            execute(task)
```

### Target State

After this implementation:
```
for stage in stages:
    action = evaluate_directive(stage.directive)
    if action == "skip": continue
    if action == branch_uuid: jump_to(branch_uuid)

    for step in steps:
        action = evaluate_directive(step.directive)
        // similar handling

        for task in tasks:
            execute(task)
```

### Related Patterns

The directive system leverages:
- **JSONLogic** (via `datalogic-rs`) for expression evaluation
- **The existing `map` task type** in dataflow-rs which also uses JSONLogic
- **Versioned object pattern** for directives (from dapple-data)

## Integrity Check: Documentation Issues

### README.md Issues

1. **Module structure mismatch** - The README lists:
   ```
   src/
   ├── func/
   │   ├── nu_handler.rs   # Nushell handler adapter
   │   └── nu_pipeline.rs  # Pipeline execution
   ```
   But actual structure also includes `handler_adapter.rs`, `prompt.rs`, and `test_handler.rs`.

2. **Missing conduit_client** - The README does not mention `conduit_client.rs` which handles HTTP communication with the conduit service.

3. **Missing commands module** - The README does not document the `commands/` directory with custom Nushell commands (`prompt`, `prompt_send`, `madlib`, `json_schema`, `summarize`).

4. **StepEngine vs dataflow Engine confusion** - The README conflates two different "Engine" concepts:
   - `StepEngine` in `engine/mod.rs` - Nushell script execution
   - `dataflow_rs::Engine` - Workflow orchestration

   This distinction needs clarification.

### AGENTS.md Missing

The `crates/dapple-prompt-flow/AGENTS.md` file does not exist. This is a significant gap - implementation agents will lack crate-specific guidance.

### Recommendations

1. **Create AGENTS.md** for dapple-prompt-flow with directive-specific guidance
2. **Update README.md** to reflect actual module structure
3. **Clarify Engine terminology** (StepEngine vs dataflow Engine)

## Architecture Overview

### Component Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                        dapple-prompt-flow                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────────────┐     ┌──────────────────┐                     │
│  │  dispatch_runner │────▶│ DirectiveRunner  │                     │
│  │  (orchestration) │     │ (new component)  │                     │
│  └──────────────────┘     └────────┬─────────┘                     │
│           │                        │                                │
│           ▼                        ▼                                │
│  ┌──────────────────┐     ┌──────────────────┐                     │
│  │  dataflow Engine │     │ DirectiveEvaluator│                    │
│  │  (task execution)│     │ (JSONLogic + run) │                    │
│  └──────────────────┘     └────────┬─────────┘                     │
│           │                        │                                │
│           ▼                        ▼                                │
│  ┌──────────────────┐     ┌──────────────────┐                     │
│  │   StepEngine     │     │   RunHandler     │                     │
│  │  (Nushell exec)  │     │ (task executor)  │                     │
│  └──────────────────┘     └──────────────────┘                     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### New Modules

```
crates/dapple-prompt-flow/
└── src/
    ├── directives/
    │   ├── mod.rs           # Module exports and DirectiveRunner
    │   ├── evaluator.rs     # DirectiveEvaluator with custom `run` operator
    │   ├── actions.rs       # DirectiveAction enum and parsing
    │   ├── context.rs       # DirectiveContext for evaluation data
    │   └── run_handler.rs   # RunHandler trait and implementations
    ├── dispatch_runner.rs   # Updated to use DirectiveRunner
    └── lib.rs               # Export directives module
```

## Object Model

### DirectiveAction

The result of evaluating a directive's JSONLogic expression.

```rust
/// Actions that a directive can return after evaluation
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectiveAction {
    /// Continue to the next stage/step (default success)
    Proceed,
    /// Skip this stage/step without execution
    Skip,
    /// Halt execution with an error
    Fail,
    /// Re-execute after retry_delay_ms (respects max_attempts)
    Retry,
    /// Re-execute immediately without delay (respects max_attempts)
    Loop,
    /// Branch to the stage/step with the given object_id
    Branch(Uuid),
}

impl DirectiveAction {
    /// Parse a JSONLogic result into a DirectiveAction
    pub fn from_json(value: &Value) -> Result<Self, DirectiveError> {
        match value {
            Value::String(s) => match s.as_str() {
                "proceed" => Ok(Self::Proceed),
                "skip" => Ok(Self::Skip),
                "fail" => Ok(Self::Fail),
                "retry" => Ok(Self::Retry),
                "loop" => Ok(Self::Loop),
                uuid_str => {
                    // Attempt to parse as UUID for branch target
                    Uuid::parse_str(uuid_str)
                        .map(Self::Branch)
                        .map_err(|_| DirectiveError::InvalidAction(s.clone()))
                }
            },
            _ => Err(DirectiveError::InvalidActionType),
        }
    }
}
```

### RunResult

The result captured from executing a stage/step via the `run` primitive.

```rust
/// Result of executing a stage or step via the `run` primitive
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunResult {
    /// Execution status
    pub status: RunStatus,
    /// Error code for categorizing failures (e.g., "RATE_LIMITED", "TIMEOUT")
    pub error_code: Option<String>,
    /// Data payload from the execution
    pub data: Value,
    /// Control-plane signals from the execution
    pub signals: HashMap<String, Value>,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Ok,
    Error,
    Timeout,
    Canceled,
}
```

### DirectiveContext

The context provided to JSONLogic evaluation.

```rust
/// Context for directive evaluation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirectiveContext {
    /// Canonical data from previous executions
    pub data: Value,
    /// Control-plane metadata
    pub metadata: Value,
    /// Current attempt number (1-indexed)
    pub attempt: u32,
    /// Maximum allowed attempts
    pub max_attempts: u32,
    /// Result from `run` execution (populated lazily)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<RunResult>,
}

impl DirectiveContext {
    pub fn new(data: Value, metadata: Value, attempt: u32, max_attempts: u32) -> Self {
        Self {
            data,
            metadata,
            attempt,
            max_attempts,
            run: None,
        }
    }

    /// Convert to JSON Value for JSONLogic evaluation
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}
```

### DirectiveEvaluator

The core evaluation engine that processes JSONLogic expressions with the custom `run` operator.

```rust
/// Evaluates directive JSONLogic expressions with custom `run` operator
pub struct DirectiveEvaluator {
    datalogic: DataLogic,
}

impl DirectiveEvaluator {
    pub fn new() -> Self {
        let mut datalogic = DataLogic::new();
        // Register custom `run` operator
        // Note: The actual `run` execution is handled via context injection,
        // not as a true custom operator. See evaluation strategy below.
        Self { datalogic }
    }

    /// Evaluate a directive's logic with the given context and run handler
    pub async fn evaluate<R: RunHandler>(
        &self,
        logic: &Value,
        context: &mut DirectiveContext,
        run_handler: &R,
    ) -> Result<DirectiveAction, DirectiveError> {
        // Pre-process the logic to find and execute `run` if present
        let (processed_logic, run_executed) = self.preprocess_run(logic, context, run_handler).await?;

        // Evaluate the processed logic
        let result = self.datalogic.apply(&processed_logic, &context.to_json())
            .map_err(|e| DirectiveError::EvaluationFailed(e.to_string()))?;

        DirectiveAction::from_json(&result)
    }

    /// Pre-process logic to handle `run` operator
    /// Returns (processed_logic, was_run_executed)
    async fn preprocess_run<R: RunHandler>(
        &self,
        logic: &Value,
        context: &mut DirectiveContext,
        run_handler: &R,
    ) -> Result<(Value, bool), DirectiveError> {
        // Walk the JSONLogic tree looking for {"run": []}
        // When found, execute via run_handler and inject result into context
        // This ensures `run` is called at most once per evaluation

        if self.contains_run_operator(logic) && context.run.is_none() {
            let result = run_handler.execute().await?;
            context.run = Some(result);
        }

        // Replace {"run": []} with {"var": "run"} for evaluation
        let processed = self.substitute_run_references(logic);

        Ok((processed, context.run.is_some()))
    }

    fn contains_run_operator(&self, logic: &Value) -> bool {
        match logic {
            Value::Object(map) => {
                if map.contains_key("run") {
                    return true;
                }
                map.values().any(|v| self.contains_run_operator(v))
            }
            Value::Array(arr) => arr.iter().any(|v| self.contains_run_operator(v)),
            _ => false,
        }
    }

    fn substitute_run_references(&self, logic: &Value) -> Value {
        match logic {
            Value::Object(map) => {
                if map.contains_key("run") {
                    // Replace {"run": []} with a reference that accesses context.run
                    // This allows subsequent {"var": "run.status"} to work
                    json!({"var": "run"})
                } else {
                    let new_map: serde_json::Map<String, Value> = map
                        .iter()
                        .map(|(k, v)| (k.clone(), self.substitute_run_references(v)))
                        .collect();
                    Value::Object(new_map)
                }
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| self.substitute_run_references(v)).collect())
            }
            _ => logic.clone(),
        }
    }
}
```

### RunHandler Trait

Abstraction for executing stages/steps.

```rust
/// Handler for executing the actual stage/step when `run` is invoked
#[async_trait]
pub trait RunHandler: Send + Sync {
    /// Execute the stage or step and return the result
    async fn execute(&self) -> Result<RunResult, DirectiveError>;
}

/// Stage execution handler
pub struct StageRunHandler<'a> {
    db: &'a DbPool,
    stage: &'a DispatchStage,
    step: &'a Step,
    functions: Option<Arc<HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>>>,
}

impl<'a> StageRunHandler<'a> {
    pub fn new(
        db: &'a DbPool,
        stage: &'a DispatchStage,
        step: &'a Step,
        functions: Option<Arc<HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>>>,
    ) -> Self {
        Self { db, stage, step, functions }
    }
}

#[async_trait]
impl<'a> RunHandler for StageRunHandler<'a> {
    async fn execute(&self) -> Result<RunResult, DirectiveError> {
        let start = std::time::Instant::now();

        // Execute all steps in the stage
        // This delegates to the existing task execution logic
        let result = execute_stage_tasks(self.db, self.stage, self.step, self.functions.clone()).await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok((data, signals)) => Ok(RunResult {
                status: RunStatus::Ok,
                error_code: None,
                data,
                signals,
                duration_ms,
            }),
            Err(e) => {
                let (status, error_code) = categorize_error(&e);
                Ok(RunResult {
                    status,
                    error_code: Some(error_code),
                    data: Value::Null,
                    signals: HashMap::new(),
                    duration_ms,
                })
            }
        }
    }
}

/// Step execution handler (similar structure)
pub struct StepRunHandler<'a> {
    db: &'a DbPool,
    step_dto: &'a DispatchStep,
    step: &'a Step,
    functions: Option<Arc<HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>>>,
}
```

### DirectiveRunner

Orchestrates directive evaluation with retry/loop logic.

```rust
/// Runs a directive for a stage or step, handling retries and loops
pub struct DirectiveRunner {
    evaluator: DirectiveEvaluator,
}

impl DirectiveRunner {
    pub fn new() -> Self {
        Self {
            evaluator: DirectiveEvaluator::new(),
        }
    }

    /// Evaluate a directive for a stage, returning the final action
    pub async fn run_stage_directive<R: RunHandler>(
        &self,
        directive: Option<&Directive>,
        context_data: Value,
        context_metadata: Value,
        run_handler: R,
        db: &DbPool,
        dispatch_attempt_object_id: Uuid,
    ) -> Result<DirectiveAction, DirectiveError> {
        // Default behavior when no directive attached
        let Some(directive) = directive else {
            let result = run_handler.execute().await?;
            return if result.status == RunStatus::Ok {
                Ok(DirectiveAction::Proceed)
            } else {
                Ok(DirectiveAction::Fail)
            };
        };

        let max_attempts = directive.max_attempts as u32;
        let retry_delay_ms = directive.retry_delay_ms.map(|d| d as u64);

        for attempt in 1..=max_attempts {
            let mut context = DirectiveContext::new(
                context_data.clone(),
                context_metadata.clone(),
                attempt,
                max_attempts,
            );

            let action = self.evaluator.evaluate(
                &directive.logic,
                &mut context,
                &run_handler,
            ).await?;

            // Record the flow decision for audit
            self.record_flow_decision(
                db,
                dispatch_attempt_object_id,
                &directive,
                attempt,
                &action,
                context.run.as_ref(),
            ).await?;

            match action {
                DirectiveAction::Retry => {
                    if attempt < max_attempts {
                        if let Some(delay) = retry_delay_ms {
                            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                        }
                        continue;
                    }
                    // Exhausted attempts
                    return Ok(DirectiveAction::Fail);
                }
                DirectiveAction::Loop => {
                    if attempt < max_attempts {
                        continue;
                    }
                    // Exhausted attempts
                    return Ok(DirectiveAction::Fail);
                }
                _ => return Ok(action),
            }
        }

        Ok(DirectiveAction::Fail)
    }

    async fn record_flow_decision(
        &self,
        db: &DbPool,
        attempt_object_id: Uuid,
        directive: &Directive,
        attempt: u32,
        action: &DirectiveAction,
        run_result: Option<&RunResult>,
    ) -> Result<(), DirectiveError> {
        let context_metadata = json!({
            "directive_object_id": directive.object_id,
            "directive_path_object_id": directive.path_object_id,
            "attempt": attempt,
            "action": format!("{:?}", action),
            "run_result": run_result,
        });

        // Use existing insert_task_attempt_event_with_attempt_object_id
        let _ = insert_task_attempt_event_with_attempt_object_id(
            db,
            attempt_object_id,
            "flow_directive",
            Some(context_metadata),
            None,
            None,
        ).await;

        Ok(())
    }
}
```

## Error Types

```rust
/// Errors that can occur during directive evaluation
#[derive(Debug, thiserror::Error)]
pub enum DirectiveError {
    #[error("Invalid action string: {0}")]
    InvalidAction(String),

    #[error("Action must be a string, got different type")]
    InvalidActionType,

    #[error("JSONLogic evaluation failed: {0}")]
    EvaluationFailed(String),

    #[error("Run handler execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Branch target not found: {0}")]
    BranchTargetNotFound(Uuid),

    #[error("Branch target out of scope: step directives can only branch within same stage")]
    BranchOutOfScope,

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Directive has no `run` operator")]
    MissingRunOperator,
}
```

## Updated dispatch_runner

The `dispatch_runner.rs` will be updated to integrate with `DirectiveRunner`:

```rust
/// Run a dispatch with directive evaluation for stages and steps
pub async fn run_dispatch_from_dto(
    db: &DbPool,
    dispatch: Dispatch,
    step: &Step,
    functions: Option<Arc<HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>>>,
    concurrency: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let sem = Arc::new(Semaphore::new(concurrency));
    let directive_runner = DirectiveRunner::new();

    let mut stage_index = 0;
    while stage_index < dispatch.stages.len() {
        let stage = &dispatch.stages[stage_index];

        // Load directive for this stage (if attached)
        let stage_directive = load_stage_directive(db, stage.object_id).await?;

        // Build context from accumulated data
        let context_data = build_context_data(db, &dispatch).await?;
        let context_metadata = build_context_metadata(&dispatch, stage).await?;

        // Create run handler for stage execution
        let run_handler = StageRunHandler::new(db, stage, step, functions.clone());

        // Evaluate directive
        let action = directive_runner.run_stage_directive(
            stage_directive.as_ref(),
            context_data,
            context_metadata,
            run_handler,
            db,
            dispatch.object_id, // or a stage-level attempt object_id
        ).await?;

        match action {
            DirectiveAction::Proceed => {
                stage_index += 1;
            }
            DirectiveAction::Skip => {
                info!(stage = %stage.object_id, "Skipping stage per directive");
                stage_index += 1;
            }
            DirectiveAction::Fail => {
                error!(stage = %stage.object_id, "Stage failed per directive");
                return Err("Directive returned fail".into());
            }
            DirectiveAction::Branch(target_uuid) => {
                // Find target stage index
                if let Some(target_idx) = find_stage_index(&dispatch.stages, target_uuid) {
                    info!(stage = %stage.object_id, target = %target_uuid, "Branching to stage");
                    stage_index = target_idx;
                } else {
                    return Err(DirectiveError::BranchTargetNotFound(target_uuid).into());
                }
            }
            DirectiveAction::Retry | DirectiveAction::Loop => {
                // These are handled internally by DirectiveRunner
                unreachable!("Retry/Loop should be handled by DirectiveRunner");
            }
        }
    }

    Ok(())
}

/// Similar logic for steps within a stage (when directive attached to step)
async fn run_stage_steps_with_directives(
    db: &DbPool,
    stage: &DispatchStage,
    // ...
) -> Result<(), DirectiveError> {
    let directive_runner = DirectiveRunner::new();

    let mut step_index = 0;
    while step_index < stage.steps.len() {
        let step_dto = &stage.steps[step_index];

        let step_directive = load_step_directive(db, step_dto.object_id).await?;

        // ... evaluate directive ...

        match action {
            DirectiveAction::Branch(target_uuid) => {
                // Step directives can only branch within the same stage
                if let Some(target_idx) = find_step_index(&stage.steps, target_uuid) {
                    step_index = target_idx;
                } else {
                    return Err(DirectiveError::BranchOutOfScope);
                }
            }
            // ... other actions ...
        }
    }

    Ok(())
}
```

## Integration with Data Layer

### Loading Directives

The flow engine needs to fetch directives attached to stages/steps:

```rust
// In dapple-prompt-flow/src/directives/mod.rs

use dapple_data::model::directives::Directive;
use dapple_data::query::directives::{
    select_directives_for_playbook_stages,
    select_directives_for_playbook_steps,
};

/// Load the directive attached to a stage (if any)
pub async fn load_stage_directive(
    db: &DbPool,
    stage_object_id: Uuid,
) -> Result<Option<Directive>, DirectiveError> {
    // Query the playbook_stage_directives relationship table
    // and join to get the full directive
    let directives = select_directives_for_playbook_stages(
        db.get_pool(),
        &[stage_object_id],
    ).await?;

    Ok(directives.into_iter().next())
}

/// Load the directive attached to a step (if any)
pub async fn load_step_directive(
    db: &DbPool,
    step_object_id: Uuid,
) -> Result<Option<Directive>, DirectiveError> {
    let directives = select_directives_for_playbook_steps(
        db.get_pool(),
        &[step_object_id],
    ).await?;

    Ok(directives.into_iter().next())
}
```

### Audit Trail

Flow decisions are recorded using the existing dispatch event infrastructure:

```rust
// Uses existing function from dapple-data
use dapple_data::query::dispatch_task_attempts::insert_task_attempt_event_with_attempt_object_id;

// Phase code for directive flow events
const PHASE_FLOW_DIRECTIVE: &str = "flow_directive";
```

## Validation Strategy

### At Directive Creation (API layer - dapple-prompt-api)

1. **Valid JSONLogic syntax** - Parse with datalogic-rs
2. **Contains `run` operator** - Walk AST to verify exactly one `{"run": []}` exists
3. **Known variable namespaces** - Validate `{"var": "..."}` uses only `data`, `metadata`, `run`, `attempt`, `max_attempts`

```rust
/// Validate a directive's logic expression
pub fn validate_directive_logic(logic: &Value) -> Result<(), ValidationError> {
    // 1. Parse as JSONLogic
    let datalogic = DataLogic::new();
    datalogic.validate(logic)
        .map_err(|e| ValidationError::InvalidJsonLogic(e.to_string()))?;

    // 2. Check for exactly one `run` operator
    let run_count = count_run_operators(logic);
    if run_count == 0 {
        return Err(ValidationError::MissingRunOperator);
    }
    if run_count > 1 {
        return Err(ValidationError::MultipleRunOperators);
    }

    // 3. Validate variable references
    let var_refs = extract_var_references(logic);
    for var_ref in var_refs {
        if !is_valid_namespace(&var_ref) {
            return Err(ValidationError::InvalidVariableReference(var_ref));
        }
    }

    Ok(())
}

fn is_valid_namespace(var_ref: &str) -> bool {
    let valid_prefixes = ["data.", "metadata.", "run.", "attempt", "max_attempts"];
    valid_prefixes.iter().any(|p| var_ref.starts_with(p) || var_ref == *p)
}
```

### At Attachment Time (API layer)

1. **Directive exists and is not deprecated**
2. **Stage/step does not already have a directive**
3. **Branch targets are valid** (for step directives, targets must be in same stage)

```rust
/// Validate branch targets in a directive before attaching to a step
pub async fn validate_step_branch_targets(
    db: &DbPool,
    directive: &Directive,
    stage_object_id: Uuid,
) -> Result<(), ValidationError> {
    let branch_targets = extract_branch_targets(&directive.logic);

    if branch_targets.is_empty() {
        return Ok(());
    }

    // Load all steps in the stage
    let steps_in_stage = load_steps_in_stage(db, stage_object_id).await?;
    let step_uuids: HashSet<Uuid> = steps_in_stage.iter().map(|s| s.object_id).collect();

    for target in branch_targets {
        if !step_uuids.contains(&target) {
            return Err(ValidationError::BranchTargetOutOfScope {
                target,
                stage: stage_object_id,
            });
        }
    }

    Ok(())
}
```

## Testing Strategy

### Unit Tests (directives module)

```rust
// tests/directives/evaluator_tests.rs

#[tokio::test]
async fn test_skip_action_parsing() {
    let action = DirectiveAction::from_json(&json!("skip")).unwrap();
    assert_eq!(action, DirectiveAction::Skip);
}

#[tokio::test]
async fn test_branch_uuid_parsing() {
    let uuid = Uuid::new_v4();
    let action = DirectiveAction::from_json(&json!(uuid.to_string())).unwrap();
    assert_eq!(action, DirectiveAction::Branch(uuid));
}

#[tokio::test]
async fn test_run_substitution() {
    let evaluator = DirectiveEvaluator::new();
    let logic = json!({
        "if": [
            {"==": [{"var": {"run": []}.status}, "ok"]},
            "proceed",
            "fail"
        ]
    });

    // After substitution, {"run": []} becomes {"var": "run"}
    let substituted = evaluator.substitute_run_references(&logic);
    assert!(!evaluator.contains_run_operator(&substituted));
}

#[tokio::test]
async fn test_directive_evaluation_skip_cached() {
    let evaluator = DirectiveEvaluator::new();

    let logic = json!({
        "if": [
            {"var": "data.cache.has_result"},
            "skip",
            {"if": [
                {"==": [{"var": {"run": []}.status}, "ok"]},
                "proceed",
                "fail"
            ]}
        ]
    });

    // Mock run handler that should NOT be called when cache.has_result is true
    let mock_handler = MockRunHandler::new();

    let mut context = DirectiveContext::new(
        json!({"cache": {"has_result": true}}),
        json!({}),
        1,
        1,
    );

    let action = evaluator.evaluate(&logic, &mut context, &mock_handler).await.unwrap();

    assert_eq!(action, DirectiveAction::Skip);
    assert!(!mock_handler.was_called());
}

#[tokio::test]
async fn test_retry_respects_max_attempts() {
    let runner = DirectiveRunner::new();

    let directive = Directive {
        logic: json!({
            "if": [
                {"==": [{"var": {"run": []}.status}, "ok"]},
                "proceed",
                "retry"
            ]
        }),
        max_attempts: 3,
        retry_delay_ms: Some(10), // Short for testing
        // ... other fields
    };

    let mut call_count = 0;
    let mock_handler = MockRunHandler::new_failing(&mut call_count);

    let action = runner.run_stage_directive(
        Some(&directive),
        json!({}),
        json!({}),
        mock_handler,
        // ... db, attempt_id
    ).await.unwrap();

    assert_eq!(action, DirectiveAction::Fail); // Exhausted attempts
    assert_eq!(call_count, 3);
}
```

### Integration Tests (with database)

```rust
// crates/dapple-prompt-api/tests/consumer/directives.rs

#[sqlx::test]
async fn test_stage_directive_skip_pattern(pool: PgPool) {
    // 1. Create a directive with skip-if-cached logic
    let directive = create_directive(&pool, json!({
        "if": [
            {"var": "data.cache.has_result"},
            "skip",
            {"if": [
                {"==": [{"var": {"run": []}.status}, "ok"]},
                "proceed",
                "fail"
            ]}
        ]
    }), 1, None).await;

    // 2. Create a playbook with a stage that has this directive attached
    let playbook = create_playbook_with_stage_directive(&pool, directive.object_id).await;

    // 3. Create a dispatch with cached data
    let dispatch = create_dispatch(&pool, playbook.object_id, json!({
        "cache": {"has_result": true}
    })).await;

    // 4. Run the dispatch
    let result = run_dispatch_from_dto(&pool, dispatch, &step, None, 1).await;

    // 5. Verify stage was skipped (no task execution)
    assert!(result.is_ok());
    let events = get_dispatch_events(&pool, dispatch.object_id).await;
    assert!(events.iter().any(|e| e.phase_code == "flow_directive" &&
        e.context_metadata.get("action") == Some(&json!("Skip"))));
}

#[sqlx::test]
async fn test_retry_on_transient_error(pool: PgPool) {
    // Test that retry action works with backoff
    // Mock a task that fails twice then succeeds
}

#[sqlx::test]
async fn test_branch_to_different_stage(pool: PgPool) {
    // Test branching from one stage to another
}

#[sqlx::test]
async fn test_step_branch_within_stage(pool: PgPool) {
    // Test that step directives can only branch within same stage
}
```

## Module Organization

### File Structure

```
crates/dapple-prompt-flow/
├── Cargo.toml                    # Add datalogic-rs dependency
├── AGENTS.md                     # NEW: Crate-specific guidance
├── README.md                     # Updated with directive documentation
└── src/
    ├── lib.rs                    # Export directives module
    ├── directives/
    │   ├── mod.rs               # DirectiveRunner, module exports
    │   ├── evaluator.rs         # DirectiveEvaluator, run operator handling
    │   ├── actions.rs           # DirectiveAction enum
    │   ├── context.rs           # DirectiveContext
    │   ├── run_handler.rs       # RunHandler trait and implementations
    │   └── error.rs             # DirectiveError
    ├── dispatch_runner.rs        # Updated with directive integration
    ├── engine/
    │   ├── mod.rs
    │   └── security.rs
    ├── func/
    │   └── ...
    ├── commands/
    │   └── ...
    └── conduit_client.rs
```

### Cargo.toml Updates

```toml
[dependencies]
# ... existing dependencies ...

# JSONLogic evaluation (already used by dataflow-rs, just make explicit)
datalogic-rs = "4.0.4"
```

## AGENTS.md for dapple-prompt-flow

Create a new file at `crates/dapple-prompt-flow/AGENTS.md`:

```markdown
# Agent Instructions: dapple-prompt-flow

This crate implements the workflow execution engine for Dapple Prompt.

## Key Concepts

### Two Types of "Engine"

This crate contains two distinct "engine" concepts:

1. **StepEngine** (`engine/mod.rs`) - Nushell script execution engine
   - Executes Nushell scripts with security policies
   - Returns `NuOutput { payload, signals }`
   - Used for tasks that need custom scripting

2. **dataflow Engine** (`dataflow_rs::Engine`) - Workflow orchestration
   - Processes workflows with tasks (map, validate, custom functions)
   - Uses JSONLogic for data transformation
   - Configured via workflow JSON definitions

### Directive System

Directives control conditional execution of stages and steps:

- **DirectiveEvaluator** - Evaluates JSONLogic with custom `run` operator
- **DirectiveRunner** - Handles retry/loop logic and action dispatch
- **RunHandler** - Executes the actual stage/step when `run` is invoked

### Execution Flow

```
Dispatch
  └── Stages (evaluated with stage directives)
        └── Steps (evaluated with step directives)
              └── Tasks (executed via dataflow Engine)
```

## Implementation Guidelines

### When Adding Custom Nushell Commands

1. Create command file in `src/commands/{command_name}.rs`
2. Implement `nu_protocol::Command` trait
3. Register in `src/commands/mod.rs` via `register_dapple_commands`
4. Add to security allowlist in `engine/security.rs` if using standard allowlist

### When Modifying Directive Evaluation

1. All changes to JSONLogic handling go in `directives/evaluator.rs`
2. Action parsing in `directives/actions.rs`
3. New error types in `directives/error.rs`
4. The `run` operator is NOT a true JSONLogic custom operator - it's handled via AST preprocessing

### When Working with dispatch_runner

1. Stage-level directives evaluated before any steps execute
2. Step-level directives evaluated before tasks in that step execute
3. Branch targets validated at attachment time, not runtime
4. Audit events recorded via `insert_task_attempt_event_with_attempt_object_id`

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
    attempt_object_id,
).await?;

match action {
    DirectiveAction::Proceed => { /* continue normally */ }
    DirectiveAction::Skip => { /* skip this element */ }
    DirectiveAction::Fail => { /* halt with error */ }
    DirectiveAction::Branch(uuid) => { /* jump to target */ }
    // Retry/Loop handled internally by DirectiveRunner
}
```

### Run Handler Implementation

```rust
#[async_trait]
impl RunHandler for MyHandler {
    async fn execute(&self) -> Result<RunResult, DirectiveError> {
        let start = std::time::Instant::now();

        // Execute the actual work
        let result = do_work().await;

        // Categorize result
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

- Unit tests for directive evaluation in `src/directives/` (use mock run handlers)
- Integration tests with database in `crates/dapple-prompt-api/tests/`
- Test retry/loop exhaustion behavior
- Test branch scope validation (step directives can only branch within stage)

## Dependencies

- `dataflow-rs` - Workflow orchestration framework
- `datalogic-rs` - JSONLogic evaluation
- `nu-*` - Nushell runtime components
- `dapple-data` - Database queries and models
```

## Implementation Guidance

### Phase 1: Core Directive Infrastructure

1. Create `directives/` module structure
2. Implement `DirectiveAction` enum with parsing
3. Implement `DirectiveError` types
4. Implement `DirectiveContext` struct
5. Add unit tests for action parsing

### Phase 2: Evaluator Implementation

1. Implement `DirectiveEvaluator` with run operator preprocessing
2. Implement `RunHandler` trait
3. Add unit tests for JSONLogic evaluation with run substitution
4. Test various directive patterns (skip, retry, branch)

### Phase 3: DirectiveRunner Integration

1. Implement `DirectiveRunner` with retry/loop logic
2. Implement `StageRunHandler` and `StepRunHandler`
3. Add audit trail recording

### Phase 4: dispatch_runner Updates

1. Update `run_dispatch_from_dto` to use `DirectiveRunner`
2. Add directive loading helpers
3. Implement stage/step branching logic
4. Add context building functions

### Phase 5: Testing and Documentation

1. Integration tests with database
2. Update README.md
3. Create AGENTS.md
4. Test edge cases (exhausted retries, invalid branches)

## Deferred to Implementation Agent

- Exact function signatures and error handling details
- Specific test data and mock implementations
- Performance optimizations
- Logging and tracing details
- Graceful degradation when directives fail to load

## Acceptance Criteria

- [ ] `DirectiveAction` enum with parsing from JSON strings
- [ ] `DirectiveEvaluator` handles `run` operator via preprocessing
- [ ] `RunHandler` trait with stage/step implementations
- [ ] `DirectiveRunner` with retry/loop support respecting max_attempts
- [ ] Updated `dispatch_runner.rs` evaluates directives before execution
- [ ] Stage directives can branch to any stage
- [ ] Step directives can only branch within same stage
- [ ] Audit trail recorded in dispatch_task_attempt_events
- [ ] Unit tests for directive evaluation patterns
- [ ] Integration tests with database
- [ ] AGENTS.md created for dapple-prompt-flow
- [ ] README.md updated with directive documentation

## Open Questions

1. **Error categorization** - What error codes should be used beyond RATE_LIMITED and TIMEOUT? Should these be configurable per playbook?

2. **Context data accumulation** - How should data from previous stages/steps be accumulated into the directive context? Should it be a full merge or selective?

3. **Concurrent step execution** - When steps within a stage can run concurrently (via semaphore), how do directives interact? Should all steps use the same directive evaluation, or individual?

4. **Directive caching** - Should directives be cached per dispatch execution, or re-fetched from DB each time? The plan assumes single fetch per stage/step.

5. **Branch cycle detection** - Should there be runtime protection against infinite branch loops? Or rely on max_attempts as the backstop?
