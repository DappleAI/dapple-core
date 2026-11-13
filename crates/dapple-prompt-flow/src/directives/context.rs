//! DirectiveContext and RunResult types for directive evaluation.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Status of a run execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Execution completed successfully.
    Ok,
    /// Execution failed with an error.
    Error,
    /// Execution timed out.
    Timeout,
    /// Execution was canceled.
    Canceled,
}

impl RunStatus {
    /// Check if this status represents a successful execution.
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    /// Check if this status represents a failure.
    pub fn is_error(&self) -> bool {
        !self.is_ok()
    }

    /// Convert to a string for JSONLogic evaluation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Timeout => "timeout",
            Self::Canceled => "canceled",
        }
    }
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Result of executing a stage or step via the `run` primitive.
///
/// This structure captures all information needed for directive evaluation
/// after execution completes. It is injected into the `DirectiveContext.run`
/// field and accessed via `{"var": "run.status"}` etc. in JSONLogic.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunResult {
    /// Execution status (ok, error, timeout, canceled).
    pub status: RunStatus,
    /// Error code for categorizing failures (e.g., "RATE_LIMITED", "TIMEOUT", "VALIDATION_ERROR").
    /// None if execution was successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Error message providing details about the failure.
    /// None if execution was successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Data payload from the execution.
    pub data: Value,
    /// Control-plane signals from the execution.
    pub signals: HashMap<String, Value>,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
}

impl RunResult {
    /// Create a successful run result.
    pub fn ok(data: Value, signals: HashMap<String, Value>, duration_ms: u64) -> Self {
        Self {
            status: RunStatus::Ok,
            error_code: None,
            error_message: None,
            data,
            signals,
            duration_ms,
        }
    }

    /// Create an error run result.
    pub fn error(
        error_code: impl Into<String>,
        error_message: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            status: RunStatus::Error,
            error_code: Some(error_code.into()),
            error_message: Some(error_message.into()),
            data: Value::Null,
            signals: HashMap::new(),
            duration_ms,
        }
    }

    /// Create a timeout run result.
    pub fn timeout(duration_ms: u64) -> Self {
        Self {
            status: RunStatus::Timeout,
            error_code: Some("TIMEOUT".to_string()),
            error_message: Some("Execution timed out".to_string()),
            data: Value::Null,
            signals: HashMap::new(),
            duration_ms,
        }
    }

    /// Create a canceled run result.
    pub fn canceled(duration_ms: u64) -> Self {
        Self {
            status: RunStatus::Canceled,
            error_code: Some("CANCELED".to_string()),
            error_message: Some("Execution was canceled".to_string()),
            data: Value::Null,
            signals: HashMap::new(),
            duration_ms,
        }
    }

    /// Check if the execution was successful.
    pub fn is_ok(&self) -> bool {
        self.status.is_ok()
    }

    /// Check if the error code matches a specific value.
    pub fn has_error_code(&self, code: &str) -> bool {
        self.error_code.as_deref() == Some(code)
    }
}

/// Context for directive evaluation.
///
/// This structure provides all data needed for JSONLogic evaluation:
/// - `data` - Canonical data from previous executions
/// - `metadata` - Control-plane metadata
/// - `attempt` - Current attempt number (1-indexed)
/// - `max_attempts` - Maximum allowed attempts
/// - `run` - Result from `run` execution (populated lazily)
///
/// When converted to JSON, this becomes the "data" argument to JSONLogic.apply().
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirectiveContext {
    /// Canonical data from previous executions.
    pub data: Value,
    /// Control-plane metadata.
    pub metadata: Value,
    /// Current attempt number (1-indexed).
    pub attempt: u32,
    /// Maximum allowed attempts.
    pub max_attempts: u32,
    /// Result from `run` execution (populated lazily when `{"run": []}` is evaluated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<RunResult>,
}

impl DirectiveContext {
    /// Create a new directive context.
    ///
    /// # Arguments
    /// * `data` - Canonical data from previous executions
    /// * `metadata` - Control-plane metadata
    /// * `attempt` - Current attempt number (1-indexed)
    /// * `max_attempts` - Maximum allowed attempts
    pub fn new(data: Value, metadata: Value, attempt: u32, max_attempts: u32) -> Self {
        Self {
            data,
            metadata,
            attempt,
            max_attempts,
            run: None,
        }
    }

    /// Set the run result.
    pub fn set_run_result(&mut self, result: RunResult) {
        self.run = Some(result);
    }

    /// Check if a run result has been captured.
    pub fn has_run_result(&self) -> bool {
        self.run.is_some()
    }

    /// Convert to JSON Value for JSONLogic evaluation.
    ///
    /// This produces a structure like:
    /// ```json
    /// {
    ///   "data": { ... },
    ///   "metadata": { ... },
    ///   "attempt": 1,
    ///   "max_attempts": 3,
    ///   "run": {
    ///     "status": "ok",
    ///     "data": { ... },
    ///     "signals": { ... },
    ///     "duration_ms": 1234
    ///   }
    /// }
    /// ```
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

impl Default for DirectiveContext {
    fn default() -> Self {
        Self::new(
            Value::Object(Default::default()),
            Value::Object(Default::default()),
            1,
            1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_run_status_is_ok() {
        assert!(RunStatus::Ok.is_ok());
        assert!(!RunStatus::Error.is_ok());
        assert!(!RunStatus::Timeout.is_ok());
        assert!(!RunStatus::Canceled.is_ok());
    }

    #[test]
    fn test_run_status_serialization() {
        assert_eq!(serde_json::to_string(&RunStatus::Ok).unwrap(), "\"ok\"");
        assert_eq!(
            serde_json::to_string(&RunStatus::Error).unwrap(),
            "\"error\""
        );
        assert_eq!(
            serde_json::to_string(&RunStatus::Timeout).unwrap(),
            "\"timeout\""
        );
        assert_eq!(
            serde_json::to_string(&RunStatus::Canceled).unwrap(),
            "\"canceled\""
        );
    }

    #[test]
    fn test_run_result_ok() {
        let mut signals = HashMap::new();
        signals.insert("completed".to_string(), json!(true));

        let result = RunResult::ok(json!({"value": 42}), signals, 100);

        assert!(result.is_ok());
        assert_eq!(result.status, RunStatus::Ok);
        assert!(result.error_code.is_none());
        assert_eq!(result.data, json!({"value": 42}));
        assert_eq!(result.duration_ms, 100);
    }

    #[test]
    fn test_run_result_error() {
        let result = RunResult::error("RATE_LIMITED", "Too many requests", 50);

        assert!(!result.is_ok());
        assert_eq!(result.status, RunStatus::Error);
        assert!(result.has_error_code("RATE_LIMITED"));
        assert!(!result.has_error_code("TIMEOUT"));
        assert_eq!(result.error_message, Some("Too many requests".to_string()));
        assert_eq!(result.duration_ms, 50);
    }

    #[test]
    fn test_run_result_timeout() {
        let result = RunResult::timeout(5000);

        assert!(!result.is_ok());
        assert_eq!(result.status, RunStatus::Timeout);
        assert!(result.has_error_code("TIMEOUT"));
        assert_eq!(result.duration_ms, 5000);
    }

    #[test]
    fn test_run_result_canceled() {
        let result = RunResult::canceled(250);

        assert!(!result.is_ok());
        assert_eq!(result.status, RunStatus::Canceled);
        assert!(result.has_error_code("CANCELED"));
        assert_eq!(result.duration_ms, 250);
    }

    #[test]
    fn test_directive_context_creation() {
        let ctx = DirectiveContext::new(
            json!({"cache": {"has_result": true}}),
            json!({"step_id": "abc123"}),
            1,
            3,
        );

        assert_eq!(ctx.attempt, 1);
        assert_eq!(ctx.max_attempts, 3);
        assert!(!ctx.has_run_result());
    }

    #[test]
    fn test_directive_context_with_run_result() {
        let mut ctx = DirectiveContext::new(json!({}), json!({}), 2, 5);

        let result = RunResult::ok(json!({"output": "test"}), HashMap::new(), 150);
        ctx.set_run_result(result);

        assert!(ctx.has_run_result());
        assert!(ctx.run.as_ref().unwrap().is_ok());
    }

    #[test]
    fn test_directive_context_to_json() {
        let mut ctx =
            DirectiveContext::new(json!({"input": "data"}), json!({"trace_id": "xyz"}), 1, 3);
        ctx.set_run_result(RunResult::ok(
            json!({"result": "success"}),
            HashMap::new(),
            100,
        ));

        let json = ctx.to_json();

        assert_eq!(json["attempt"], 1);
        assert_eq!(json["max_attempts"], 3);
        assert_eq!(json["data"]["input"], "data");
        assert_eq!(json["metadata"]["trace_id"], "xyz");
        assert_eq!(json["run"]["status"], "ok");
        assert_eq!(json["run"]["data"]["result"], "success");
        assert_eq!(json["run"]["duration_ms"], 100);
    }

    #[test]
    fn test_directive_context_json_without_run() {
        let ctx = DirectiveContext::new(json!({}), json!({}), 1, 1);
        let json = ctx.to_json();

        // run should not be present when None
        assert!(json.get("run").is_none());
    }
}
