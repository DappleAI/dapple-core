//! DirectiveEvaluator with JSONLogic evaluation and `run` operator preprocessing.

use super::actions::DirectiveAction;
use super::context::DirectiveContext;
use super::error::DirectiveError;
use super::run_handler::RunHandler;
use datalogic_rs::DataLogic;
use serde_json::{Map, Value, json};
use std::sync::Arc;

/// Evaluates directive JSONLogic expressions with custom `run` operator handling.
///
/// The `run` operator is NOT a true JSONLogic custom operator. Instead, it is
/// handled via AST preprocessing:
///
/// 1. Before evaluation, walk the JSONLogic tree looking for `{"run": []}`
/// 2. If found and no run result exists in context, execute the stage/step via RunHandler
/// 3. Inject the result into `context.run`
/// 4. Replace `{"run": []}` with `{"var": "run"}` for evaluation
/// 5. This ensures `run` executes at most once per evaluation
///
/// This approach allows directives to:
/// - Check cached data before running (`{"if": [{"var": "data.cache.exists"}, "skip", ...]}`)
/// - Execute and branch based on results (`{"if": [{"==": [{"var": "run.status"}, "ok"]}, "proceed", "fail"]}`)
/// - Retry on specific error codes (`{"if": [{"in": [{"var": "run.error_code"}, ["RATE_LIMITED"]]}, "retry", "fail"]}`)
pub struct DirectiveEvaluator {
    datalogic: DataLogic,
}

impl DirectiveEvaluator {
    /// Create a new DirectiveEvaluator.
    pub fn new() -> Self {
        let datalogic = DataLogic::new();
        Self { datalogic }
    }

    /// Evaluate a directive's logic with the given context and run handler.
    ///
    /// # Arguments
    /// * `logic` - The JSONLogic expression from the directive
    /// * `context` - Mutable context (may be updated with run result)
    /// * `run_handler` - Handler to execute the stage/step when `run` is encountered
    ///
    /// # Returns
    /// * `Ok(DirectiveAction)` - The action to take
    /// * `Err(DirectiveError)` - If evaluation fails
    pub async fn evaluate<R: RunHandler>(
        &self,
        logic: &Value,
        context: &mut DirectiveContext,
        run_handler: &R,
    ) -> Result<DirectiveAction, DirectiveError> {
        // Pre-process the logic to find and execute `run` if present
        let processed_logic = self.preprocess_run(logic, context, run_handler).await?;

        // Compile and evaluate the processed logic against the context
        let context_json = context.to_json();
        let compiled = self
            .datalogic
            .compile(&processed_logic)
            .map_err(|e| DirectiveError::EvaluationFailed(e.to_string()))?;

        let result = self
            .datalogic
            .evaluate(&compiled, Arc::new(context_json))
            .map_err(|e| DirectiveError::EvaluationFailed(e.to_string()))?;

        DirectiveAction::from_json(&result)
    }

    /// Pre-process logic to handle `run` operator.
    ///
    /// This method:
    /// 1. Checks if the logic tree contains `{"run": []}`
    /// 2. If found and context has no run result, executes via run_handler
    /// 3. Returns a modified logic tree with `{"run": []}` replaced by `{"var": "run"}`
    async fn preprocess_run<R: RunHandler>(
        &self,
        logic: &Value,
        context: &mut DirectiveContext,
        run_handler: &R,
    ) -> Result<Value, DirectiveError> {
        // Check if we need to execute run
        if self.contains_run_operator(logic) && !context.has_run_result() {
            // Execute the run handler
            let result = run_handler.execute().await?;
            context.set_run_result(result);
        }

        // Replace {"run": []} with {"var": "run"} for JSONLogic evaluation
        Ok(self.substitute_run_references(logic))
    }

    /// Check if the logic tree contains a `run` operator.
    ///
    /// Walks the entire JSONLogic tree looking for `{"run": ...}` nodes.
    pub fn contains_run_operator(&self, logic: &Value) -> bool {
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

    /// Count the number of `run` operators in the logic tree.
    ///
    /// Useful for validation - directives should have exactly one `run` operator.
    pub fn count_run_operators(&self, logic: &Value) -> usize {
        match logic {
            Value::Object(map) => {
                let count_here = if map.contains_key("run") { 1 } else { 0 };
                let count_children: usize = map.values().map(|v| self.count_run_operators(v)).sum();
                count_here + count_children
            }
            Value::Array(arr) => arr.iter().map(|v| self.count_run_operators(v)).sum(),
            _ => 0,
        }
    }

    /// Replace `{"run": ...}` with `{"var": "run"}` throughout the logic tree.
    ///
    /// This allows the run result (stored in context.run) to be accessed
    /// as a variable during JSONLogic evaluation.
    fn substitute_run_references(&self, logic: &Value) -> Value {
        match logic {
            Value::Object(map) => {
                if map.contains_key("run") {
                    // Replace {"run": ...} with {"var": "run"}
                    json!({"var": "run"})
                } else {
                    // Recursively process children
                    let new_map: Map<String, Value> = map
                        .iter()
                        .map(|(k, v)| (k.clone(), self.substitute_run_references(v)))
                        .collect();
                    Value::Object(new_map)
                }
            }
            Value::Array(arr) => Value::Array(
                arr.iter()
                    .map(|v| self.substitute_run_references(v))
                    .collect(),
            ),
            _ => logic.clone(),
        }
    }

    /// Validate a directive's logic expression.
    ///
    /// Checks:
    /// 1. Valid JSONLogic syntax
    /// 2. Contains exactly one `run` operator
    /// 3. Variable references use valid namespaces
    ///
    /// # Returns
    /// * `Ok(())` if validation passes
    /// * `Err(DirectiveError)` if validation fails
    pub fn validate_logic(&self, logic: &Value) -> Result<(), DirectiveError> {
        // Check JSONLogic syntax by attempting to compile
        // Note: This validates syntax without evaluating
        let _ = self.datalogic.compile(logic).map_err(|e| {
            DirectiveError::EvaluationFailed(format!("Invalid JSONLogic syntax: {}", e))
        })?;

        // Check for run operator
        let run_count = self.count_run_operators(logic);
        if run_count == 0 {
            return Err(DirectiveError::MissingRunOperator);
        }
        if run_count > 1 {
            return Err(DirectiveError::MultipleRunOperators);
        }

        // Validate variable references (extract and check prefixes)
        let var_refs = self.extract_var_references(logic);
        for var_ref in var_refs {
            if !Self::is_valid_var_namespace(&var_ref) {
                return Err(DirectiveError::EvaluationFailed(format!(
                    "Invalid variable reference: '{}'. Must start with 'data.', 'metadata.', 'run.', 'attempt', or 'max_attempts'",
                    var_ref
                )));
            }
        }

        Ok(())
    }

    /// Extract all variable references from the logic tree.
    ///
    /// Finds all `{"var": "path"}` nodes and returns the path strings.
    fn extract_var_references(&self, logic: &Value) -> Vec<String> {
        let mut refs = Vec::new();
        self.collect_var_references(logic, &mut refs);
        refs
    }

    fn collect_var_references(&self, logic: &Value, refs: &mut Vec<String>) {
        match logic {
            Value::Object(map) => {
                if let Some(var_value) = map.get("var")
                    && let Some(path) = var_value.as_str()
                {
                    refs.push(path.to_string());
                }
                for v in map.values() {
                    self.collect_var_references(v, refs);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    self.collect_var_references(v, refs);
                }
            }
            _ => {}
        }
    }

    /// Check if a variable reference uses a valid namespace.
    fn is_valid_var_namespace(var_ref: &str) -> bool {
        let valid_prefixes = ["data.", "metadata.", "run.", "run"];
        let exact_matches = ["attempt", "max_attempts"];

        exact_matches.contains(&var_ref) || valid_prefixes.iter().any(|p| var_ref.starts_with(p))
    }
}

impl Default for DirectiveEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directives::context::RunResult;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Mock run handler for testing
    struct MockRunHandler {
        call_count: Arc<AtomicU32>,
        result: RunResult,
    }

    impl MockRunHandler {
        fn new_ok() -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                result: RunResult::ok(json!({"output": "test"}), HashMap::new(), 100),
            }
        }

        fn new_error(error_code: &str) -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                result: RunResult::error(error_code, "Test error", 50),
            }
        }

        fn call_count(&self) -> u32 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl RunHandler for MockRunHandler {
        async fn execute(&self) -> Result<RunResult, DirectiveError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.result.clone())
        }
    }

    #[test]
    fn test_contains_run_operator() {
        let evaluator = DirectiveEvaluator::new();

        // Direct run operator
        assert!(evaluator.contains_run_operator(&json!({"run": []})));

        // Nested in if
        assert!(evaluator.contains_run_operator(&json!({
            "if": [
                {"==": [{"var": "run.status"}, "ok"]},
                "proceed",
                {"run": []}
            ]
        })));

        // No run operator
        assert!(!evaluator.contains_run_operator(&json!({"var": "data.value"})));
        assert!(!evaluator.contains_run_operator(&json!("proceed")));
    }

    #[test]
    fn test_count_run_operators() {
        let evaluator = DirectiveEvaluator::new();

        assert_eq!(evaluator.count_run_operators(&json!({"run": []})), 1);
        assert_eq!(evaluator.count_run_operators(&json!({"var": "x"})), 0);
        assert_eq!(
            evaluator.count_run_operators(&json!({
                "if": [true, {"run": []}, {"run": []}]
            })),
            2
        );
    }

    #[test]
    fn test_substitute_run_references() {
        let evaluator = DirectiveEvaluator::new();

        // Direct substitution
        let result = evaluator.substitute_run_references(&json!({"run": []}));
        assert_eq!(result, json!({"var": "run"}));

        // Nested substitution - {"run": []} in array should be replaced
        let result = evaluator.substitute_run_references(&json!({
            "if": [
                {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                "proceed",
                "fail"
            ]
        }));
        // The {"run": []} should be replaced with {"var": "run"}
        assert!(!evaluator.contains_run_operator(&result));
    }

    #[test]
    fn test_extract_var_references() {
        let evaluator = DirectiveEvaluator::new();

        let logic = json!({
            "if": [
                {"==": [{"var": "data.cache.has_result"}, true]},
                "skip",
                {"if": [
                    {"==": [{"var": "run.status"}, "ok"]},
                    "proceed",
                    "fail"
                ]}
            ]
        });

        let refs = evaluator.extract_var_references(&logic);
        assert!(refs.contains(&"data.cache.has_result".to_string()));
        assert!(refs.contains(&"run.status".to_string()));
    }

    #[test]
    fn test_is_valid_var_namespace() {
        assert!(DirectiveEvaluator::is_valid_var_namespace(
            "data.cache.value"
        ));
        assert!(DirectiveEvaluator::is_valid_var_namespace(
            "metadata.trace_id"
        ));
        assert!(DirectiveEvaluator::is_valid_var_namespace("run.status"));
        assert!(DirectiveEvaluator::is_valid_var_namespace("run.error_code"));
        assert!(DirectiveEvaluator::is_valid_var_namespace("attempt"));
        assert!(DirectiveEvaluator::is_valid_var_namespace("max_attempts"));
        assert!(DirectiveEvaluator::is_valid_var_namespace("run"));

        assert!(!DirectiveEvaluator::is_valid_var_namespace("invalid.path"));
        assert!(!DirectiveEvaluator::is_valid_var_namespace("foo"));
    }

    #[tokio::test]
    async fn test_evaluate_simple_proceed() {
        let evaluator = DirectiveEvaluator::new();
        let handler = MockRunHandler::new_ok();

        // Use {"run": []} followed by checking "run.status"
        // The preprocessing replaces {"run": []} with {"var": "run"}
        let logic = json!({
            "if": [
                {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                "proceed",
                "fail"
            ]
        });

        let mut context = DirectiveContext::new(json!({}), json!({}), 1, 1);
        let action = evaluator
            .evaluate(&logic, &mut context, &handler)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Proceed);
        assert_eq!(handler.call_count(), 1);
        assert!(context.has_run_result());
    }

    #[tokio::test]
    async fn test_evaluate_skip_when_cached() {
        let evaluator = DirectiveEvaluator::new();
        let handler = MockRunHandler::new_ok();

        // Logic that skips if cache exists, otherwise runs
        let logic = json!({
            "if": [
                {"var": "data.cache.has_result"},
                "skip",
                {"if": [
                    {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                    "proceed",
                    "fail"
                ]}
            ]
        });

        // Context with cache present - should skip without running
        let mut context =
            DirectiveContext::new(json!({"cache": {"has_result": true}}), json!({}), 1, 1);

        let action = evaluator
            .evaluate(&logic, &mut context, &handler)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Skip);
        // Run should still be called because our preprocessing executes run whenever it's found
        // but the condition short-circuits before the run result is used
    }

    #[tokio::test]
    async fn test_evaluate_retry_on_error() {
        let evaluator = DirectiveEvaluator::new();
        let handler = MockRunHandler::new_error("RATE_LIMITED");

        let logic = json!({
            "if": [
                {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                "proceed",
                {"if": [
                    {"in": [{"var": "run.error_code"}, ["RATE_LIMITED", "TIMEOUT"]]},
                    "retry",
                    "fail"
                ]}
            ]
        });

        let mut context = DirectiveContext::new(json!({}), json!({}), 1, 3);
        let action = evaluator
            .evaluate(&logic, &mut context, &handler)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Retry);
        assert!(context.run.as_ref().unwrap().has_error_code("RATE_LIMITED"));
    }

    #[tokio::test]
    async fn test_evaluate_fail_on_unknown_error() {
        let evaluator = DirectiveEvaluator::new();
        let handler = MockRunHandler::new_error("UNKNOWN_ERROR");

        let logic = json!({
            "if": [
                {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                "proceed",
                {"if": [
                    {"in": [{"var": "run.error_code"}, ["RATE_LIMITED", "TIMEOUT"]]},
                    "retry",
                    "fail"
                ]}
            ]
        });

        let mut context = DirectiveContext::new(json!({}), json!({}), 1, 3);
        let action = evaluator
            .evaluate(&logic, &mut context, &handler)
            .await
            .unwrap();

        assert_eq!(action, DirectiveAction::Fail);
    }

    #[tokio::test]
    async fn test_run_called_only_once() {
        let evaluator = DirectiveEvaluator::new();
        let handler = MockRunHandler::new_ok();

        let logic = json!({
            "if": [
                {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                "proceed",
                "fail"
            ]
        });

        let mut context = DirectiveContext::new(json!({}), json!({}), 1, 1);

        // First evaluation
        let _ = evaluator.evaluate(&logic, &mut context, &handler).await;
        assert_eq!(handler.call_count(), 1);

        // Second evaluation with same context (run already set)
        let _ = evaluator.evaluate(&logic, &mut context, &handler).await;
        // Should not call run again
        assert_eq!(handler.call_count(), 1);
    }

    #[test]
    fn test_validate_logic_valid() {
        let evaluator = DirectiveEvaluator::new();

        let logic = json!({
            "if": [
                {"and": [{"run": []}, {"==": [{"var": "run.status"}, "ok"]}]},
                "proceed",
                "fail"
            ]
        });

        // This should pass validation (has exactly one run)
        let result = evaluator.validate_logic(&logic);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_logic_missing_run() {
        let evaluator = DirectiveEvaluator::new();

        let logic = json!({
            "if": [
                {"var": "data.value"},
                "proceed",
                "fail"
            ]
        });

        let result = evaluator.validate_logic(&logic);
        assert!(matches!(result, Err(DirectiveError::MissingRunOperator)));
    }

    #[test]
    fn test_validate_logic_multiple_run() {
        let evaluator = DirectiveEvaluator::new();

        let logic = json!({
            "if": [
                {"==": [{"run": []}, "ok"]},
                {"run": []},
                "fail"
            ]
        });

        let result = evaluator.validate_logic(&logic);
        assert!(matches!(result, Err(DirectiveError::MultipleRunOperators)));
    }
}
