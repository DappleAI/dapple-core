//! Flow logic directive system for conditional execution, retry, and branching.
//!
//! This module implements the directive evaluation system that controls playbook
//! stage and step execution. Directives use JSONLogic expressions with a custom
//! `run` operator to:
//!
//! - Skip stages/steps based on cached data
//! - Retry on transient errors (rate limiting, timeouts)
//! - Loop until conditions are met (polling patterns)
//! - Branch to different stages/steps based on results
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────┐     ┌──────────────────┐
//! │ DirectiveRunner  │────▶│ DirectiveEvaluator│
//! │ (orchestration)  │     │ (JSONLogic + run) │
//! └──────────────────┘     └────────┬─────────┘
//!          │                        │
//!          ▼                        ▼
//! ┌──────────────────┐     ┌──────────────────┐
//! │   dispatch_runner │     │   RunHandler     │
//! │  (integration)    │     │ (task executor)  │
//! └──────────────────┘     └──────────────────┘
//! ```
//!
//! # The `run` Operator
//!
//! The `run` operator is handled via AST preprocessing, not as a true JSONLogic
//! custom operator:
//!
//! 1. Before evaluation, walk the JSONLogic tree for `{"run": []}`
//! 2. If found, execute the stage/step via RunHandler
//! 3. Inject result into context as `context.run`
//! 4. Replace `{"run": []}` with `{"var": "run"}` for evaluation
//!
//! This ensures `run` executes at most once per evaluation.
//!
//! # Example Directive Logic
//!
//! ```json
//! {
//!   "if": [
//!     {"var": "data.cache.has_result"},
//!     "skip",
//!     {"if": [
//!       {"==": [{"var": {"run": []}.status}, "ok"]},
//!       "proceed",
//!       {"if": [
//!         {"in": [{"var": "run.error_code"}, ["RATE_LIMITED", "TIMEOUT"]]},
//!         "retry",
//!         "fail"
//!       ]}
//!     ]}
//!   ]
//! }
//! ```

pub mod actions;
pub mod context;
pub mod error;
pub mod evaluator;
pub mod run_handler;

pub use actions::DirectiveAction;
pub use context::{DirectiveContext, RunResult, RunStatus};
pub use error::DirectiveError;
pub use evaluator::DirectiveEvaluator;
pub use run_handler::{ClosureRunHandler, EngineRunHandler, NoOpRunHandler, RunHandler};

use dapple_data::DbPool;
use dapple_data::model::directives::Directive;
use dapple_data::query::dispatch_task_attempts::insert_task_attempt_event_with_attempt_object_id;
use serde_json::{Value, json};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Runs directives for stages or steps, handling retries and loops.
///
/// The DirectiveRunner orchestrates directive evaluation with:
/// - Retry/loop logic respecting max_attempts
/// - Retry delays between attempts
/// - Audit trail recording
/// - Error handling for exhausted attempts
pub struct DirectiveRunner {
    evaluator: DirectiveEvaluator,
}

impl DirectiveRunner {
    /// Create a new DirectiveRunner.
    pub fn new() -> Self {
        Self {
            evaluator: DirectiveEvaluator::new(),
        }
    }

    /// Evaluate a directive for a stage, returning the final action.
    ///
    /// This method handles:
    /// - Retry/loop logic with max_attempts enforcement
    /// - Retry delays between attempts
    /// - Audit trail recording for each decision
    /// - Default behavior when no directive is attached
    ///
    /// # Arguments
    /// * `directive` - Optional directive to evaluate (None means execute and proceed/fail)
    /// * `context_data` - Canonical data from previous executions
    /// * `context_metadata` - Control-plane metadata
    /// * `run_handler` - Handler to execute the stage/step
    /// * `db` - Database pool for audit recording
    /// * `dispatch_attempt_object_id` - Object ID for audit trail linkage
    ///
    /// # Returns
    /// * `Ok(DirectiveAction)` - The final action (Proceed, Skip, Fail, or Branch)
    /// * `Err(DirectiveError)` - If evaluation fails fatally
    ///
    /// Note: Retry and Loop actions are handled internally. The returned action
    /// will always be terminal (Proceed, Skip, Fail, or Branch).
    pub async fn run_directive<R: RunHandler>(
        &self,
        directive: Option<&Directive>,
        context_data: Value,
        context_metadata: Value,
        run_handler: &R,
        db: Option<&DbPool>,
        dispatch_attempt_object_id: Option<Uuid>,
    ) -> Result<DirectiveAction, DirectiveError> {
        // Default behavior when no directive attached
        let Some(directive) = directive else {
            debug!("No directive attached, executing with default behavior");
            let result = run_handler.execute().await?;
            return if result.is_ok() {
                Ok(DirectiveAction::Proceed)
            } else {
                Ok(DirectiveAction::Fail)
            };
        };

        let max_attempts = directive.max_attempts as u32;
        let retry_delay_ms = directive.retry_delay_ms.map(|d| d as u64);

        info!(
            directive_id = %directive.id,
            max_attempts = max_attempts,
            retry_delay_ms = ?retry_delay_ms,
            "Starting directive evaluation"
        );

        for attempt in 1..=max_attempts {
            let mut context = DirectiveContext::new(
                context_data.clone(),
                context_metadata.clone(),
                attempt,
                max_attempts,
            );

            debug!(attempt = attempt, "Evaluating directive");

            let action = self
                .evaluator
                .evaluate(&directive.logic, &mut context, run_handler)
                .await?;

            // Record the flow decision for audit
            if let (Some(db), Some(attempt_id)) = (db, dispatch_attempt_object_id) {
                let _ = self
                    .record_flow_decision(
                        db,
                        attempt_id,
                        directive,
                        attempt,
                        &action,
                        context.run.as_ref(),
                    )
                    .await;
            }

            match action {
                DirectiveAction::Retry => {
                    if attempt < max_attempts {
                        info!(
                            attempt = attempt,
                            max_attempts = max_attempts,
                            delay_ms = ?retry_delay_ms,
                            "Retrying after delay"
                        );
                        if let Some(delay) = retry_delay_ms {
                            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                        }
                        continue;
                    }
                    // Exhausted attempts
                    warn!(
                        directive_id = %directive.id,
                        max_attempts = max_attempts,
                        "Max attempts exhausted (retry)"
                    );
                    return Ok(DirectiveAction::Fail);
                }
                DirectiveAction::Loop => {
                    if attempt < max_attempts {
                        debug!(
                            attempt = attempt,
                            max_attempts = max_attempts,
                            "Looping immediately"
                        );
                        continue;
                    }
                    // Exhausted attempts
                    warn!(
                        directive_id = %directive.id,
                        max_attempts = max_attempts,
                        "Max attempts exhausted (loop)"
                    );
                    return Ok(DirectiveAction::Fail);
                }
                // Terminal actions - return immediately
                action => {
                    info!(
                        directive_id = %directive.id,
                        action = %action,
                        attempt = attempt,
                        "Directive evaluation complete"
                    );
                    return Ok(action);
                }
            }
        }

        // Should not reach here, but fail safely
        Ok(DirectiveAction::Fail)
    }

    /// Record a flow decision for audit trail.
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
            "directive_id": directive.id,
            "directive_path_id": directive.path_id,
            "directive_name": directive.name,
            "attempt": attempt,
            "max_attempts": directive.max_attempts,
            "action": action.to_string(),
            "run_result": run_result.map(|r| json!({
                "status": r.status.as_str(),
                "error_code": r.error_code,
                "duration_ms": r.duration_ms,
            })),
        });

        let _ = insert_task_attempt_event_with_attempt_object_id(
            db,
            attempt_object_id,
            "flow_directive",
            Some(context_metadata),
            None,
            None,
        )
        .await;

        Ok(())
    }
}

impl Default for DirectiveRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the index of a stage with the given object_id.
///
/// Used for branch target resolution in dispatch_runner.
pub fn find_stage_index<T>(
    stages: &[T],
    target_uuid: Uuid,
    get_object_id: impl Fn(&T) -> Uuid,
) -> Option<usize> {
    stages.iter().position(|s| get_object_id(s) == target_uuid)
}

/// Find the index of a step with the given object_id.
///
/// Used for branch target resolution in dispatch_runner.
pub fn find_step_index<T>(
    steps: &[T],
    target_uuid: Uuid,
    get_object_id: impl Fn(&T) -> Uuid,
) -> Option<usize> {
    steps.iter().position(|s| get_object_id(s) == target_uuid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    use dapple_data::model::directives::Directive;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Mock run handler that can be configured to succeed or fail
    struct ConfigurableRunHandler {
        call_count: Arc<AtomicU32>,
        results: Vec<RunResult>,
    }

    impl ConfigurableRunHandler {
        fn new(results: Vec<RunResult>) -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                results,
            }
        }

        fn call_count(&self) -> u32 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl RunHandler for ConfigurableRunHandler {
        async fn execute(&self) -> Result<RunResult, DirectiveError> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst) as usize;
            let result =
                self.results.get(idx).cloned().unwrap_or_else(|| {
                    RunResult::error("EXHAUSTED", "No more results configured", 0)
                });
            Ok(result)
        }
    }

    fn make_directive(logic: Value, max_attempts: i32, retry_delay_ms: Option<i32>) -> Directive {
        use time::OffsetDateTime;
        Directive {
            id: Uuid::now_v7(),
            path_id: Uuid::now_v7(),
            name: "test_directive".to_string(),
            content_hash: dapple_data::ContentHash::default(),
            deprecation_level: None,
            logic,
            max_attempts,
            retry_delay_ms,
            description: None,
            created_at: OffsetDateTime::now_utc().into(),
        }
    }

    #[tokio::test]
    async fn test_no_directive_success() {
        let runner = DirectiveRunner::new();
        let handler = ConfigurableRunHandler::new(vec![RunResult::ok(
            json!({"result": "success"}),
            HashMap::new(),
            100,
        )]);

        let action = runner
            .run_directive(None, json!({}), json!({}), &handler, None, None)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Proceed);
        assert_eq!(handler.call_count(), 1);
    }

    #[tokio::test]
    async fn test_no_directive_failure() {
        let runner = DirectiveRunner::new();
        let handler =
            ConfigurableRunHandler::new(vec![RunResult::error("TEST_ERROR", "Test failure", 100)]);

        let action = runner
            .run_directive(None, json!({}), json!({}), &handler, None, None)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Fail);
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        let runner = DirectiveRunner::new();
        let handler = ConfigurableRunHandler::new(vec![
            RunResult::error("RATE_LIMITED", "Too many requests", 50),
            RunResult::error("RATE_LIMITED", "Too many requests", 50),
            RunResult::error("RATE_LIMITED", "Too many requests", 50),
        ]);

        // Directive that retries on RATE_LIMITED
        // The logic: first invoke run, then check the result
        // {"run": []} will be preprocessed and result available as "run"
        let directive = make_directive(
            json!({
                "if": [
                    {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                    "proceed",
                    {"if": [
                        {"==": [{"var": "run.error_code"}, "RATE_LIMITED"]},
                        "retry",
                        "fail"
                    ]}
                ]
            }),
            3,       // max_attempts
            Some(1), // 1ms delay for fast tests
        );

        let action = runner
            .run_directive(Some(&directive), json!({}), json!({}), &handler, None, None)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Fail);
        assert_eq!(handler.call_count(), 3);
    }

    #[tokio::test]
    async fn test_retry_then_succeed() {
        let runner = DirectiveRunner::new();
        let handler = ConfigurableRunHandler::new(vec![
            RunResult::error("RATE_LIMITED", "Too many requests", 50),
            RunResult::ok(json!({"result": "success"}), HashMap::new(), 100),
        ]);

        let directive = make_directive(
            json!({
                "if": [
                    {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                    "proceed",
                    {"if": [
                        {"==": [{"var": "run.error_code"}, "RATE_LIMITED"]},
                        "retry",
                        "fail"
                    ]}
                ]
            }),
            3,
            Some(1),
        );

        let action = runner
            .run_directive(Some(&directive), json!({}), json!({}), &handler, None, None)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Proceed);
        assert_eq!(handler.call_count(), 2);
    }

    #[tokio::test]
    async fn test_skip_action() {
        let runner = DirectiveRunner::new();
        let handler = ConfigurableRunHandler::new(vec![]);

        // Directive that skips when cache exists
        let directive = make_directive(
            json!({
                "if": [
                    {"var": "data.cache.has_result"},
                    "skip",
                    {"if": [
                        {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                        "proceed",
                        "fail"
                    ]}
                ]
            }),
            1,
            None,
        );

        let action = runner
            .run_directive(
                Some(&directive),
                json!({"cache": {"has_result": true}}),
                json!({}),
                &handler,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Skip);
    }

    #[tokio::test]
    async fn test_branch_action() {
        let runner = DirectiveRunner::new();
        let target_uuid = Uuid::now_v7();
        let handler = ConfigurableRunHandler::new(vec![RunResult::ok(
            json!({"should_branch": true}),
            HashMap::new(),
            100,
        )]);

        // Directive that branches based on run result
        let directive = make_directive(
            json!({
                "if": [
                    {"and": [{"run": []}, {"==": [{"var": "run.data.should_branch"}, true]}]},
                    target_uuid.to_string(),
                    "proceed"
                ]
            }),
            1,
            None,
        );

        let action = runner
            .run_directive(Some(&directive), json!({}), json!({}), &handler, None, None)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Branch(target_uuid));
    }

    #[tokio::test]
    async fn test_loop_exhausted() {
        let runner = DirectiveRunner::new();
        let handler = ConfigurableRunHandler::new(vec![
            RunResult::ok(json!({"ready": false}), HashMap::new(), 50),
            RunResult::ok(json!({"ready": false}), HashMap::new(), 50),
            RunResult::ok(json!({"ready": false}), HashMap::new(), 50),
        ]);

        // Directive that loops until ready
        let directive = make_directive(
            json!({
                "if": [
                    {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                    {"if": [
                        {"var": "run.data.ready"},
                        "proceed",
                        "loop"
                    ]},
                    "fail"
                ]
            }),
            3,
            None,
        );

        let action = runner
            .run_directive(Some(&directive), json!({}), json!({}), &handler, None, None)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Fail);
        assert_eq!(handler.call_count(), 3);
    }

    #[tokio::test]
    async fn test_loop_then_ready() {
        let runner = DirectiveRunner::new();
        let handler = ConfigurableRunHandler::new(vec![
            RunResult::ok(json!({"ready": false}), HashMap::new(), 50),
            RunResult::ok(json!({"ready": true}), HashMap::new(), 50),
        ]);

        let directive = make_directive(
            json!({
                "if": [
                    {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                    {"if": [
                        {"var": "run.data.ready"},
                        "proceed",
                        "loop"
                    ]},
                    "fail"
                ]
            }),
            5,
            None,
        );

        let action = runner
            .run_directive(Some(&directive), json!({}), json!({}), &handler, None, None)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Proceed);
        assert_eq!(handler.call_count(), 2);
    }

    #[test]
    fn test_find_stage_index() {
        struct Stage {
            id: Uuid,
        }

        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();
        let id3 = Uuid::now_v7();

        let stages = vec![Stage { id: id1 }, Stage { id: id2 }, Stage { id: id3 }];

        assert_eq!(find_stage_index(&stages, id1, |s| s.id), Some(0));
        assert_eq!(find_stage_index(&stages, id2, |s| s.id), Some(1));
        assert_eq!(find_stage_index(&stages, id3, |s| s.id), Some(2));
        assert_eq!(find_stage_index(&stages, Uuid::now_v7(), |s| s.id), None);
    }
}
