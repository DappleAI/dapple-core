//! RunHandler trait and implementations for executing stages/steps.

use super::context::{RunResult, RunStatus};
use super::error::DirectiveError;
use async_trait::async_trait;
use dataflow_rs::Engine;
use dataflow_rs::engine::message::Message;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Handler for executing the actual stage/step when `run` is invoked.
///
/// Implementations of this trait are responsible for:
/// 1. Executing the stage or step's tasks
/// 2. Measuring execution duration
/// 3. Categorizing any errors
/// 4. Returning a RunResult with status, data, and signals
#[async_trait]
pub trait RunHandler: Send + Sync {
    /// Execute the stage or step and return the result.
    async fn execute(&self) -> Result<RunResult, DirectiveError>;
}

/// A no-op run handler for testing or when execution should be skipped.
pub struct NoOpRunHandler;

#[async_trait]
impl RunHandler for NoOpRunHandler {
    async fn execute(&self) -> Result<RunResult, DirectiveError> {
        Ok(RunResult::ok(Value::Null, HashMap::new(), 0))
    }
}

/// Run handler that executes via a dataflow Engine.
///
/// This handler wraps dataflow-rs Engine execution and converts the results
/// to a RunResult suitable for directive evaluation.
pub struct EngineRunHandler {
    engine: Engine,
    initial_payload: Value,
}

impl EngineRunHandler {
    /// Create a new engine run handler.
    ///
    /// # Arguments
    /// * `engine` - The dataflow Engine to execute
    /// * `initial_payload` - The initial payload to process
    pub fn new(engine: Engine, initial_payload: Value) -> Self {
        Self {
            engine,
            initial_payload,
        }
    }
}

#[async_trait]
impl RunHandler for EngineRunHandler {
    async fn execute(&self) -> Result<RunResult, DirectiveError> {
        let start = Instant::now();

        let payload = Arc::new(self.initial_payload.clone());
        let mut message = Message::new(payload);

        match self.engine.process_message(&mut message).await {
            Ok(_) => {
                let duration_ms = start.elapsed().as_millis() as u64;

                // Extract signals from message metadata if present
                let signals = extract_signals_from_message(&message);

                Ok(RunResult::ok(message.data().clone(), signals, duration_ms))
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                let (error_code, error_message) = categorize_error(&e);
                Ok(RunResult {
                    status: RunStatus::Error,
                    error_code: Some(error_code),
                    error_message: Some(error_message),
                    data: Value::Null,
                    signals: HashMap::new(),
                    duration_ms,
                })
            }
        }
    }
}

/// Run handler that executes a closure.
///
/// This is useful for testing or for simple execution scenarios.
pub struct ClosureRunHandler<F>
where
    F: Fn() -> Result<(Value, HashMap<String, Value>), String> + Send + Sync,
{
    closure: F,
}

impl<F> ClosureRunHandler<F>
where
    F: Fn() -> Result<(Value, HashMap<String, Value>), String> + Send + Sync,
{
    pub fn new(closure: F) -> Self {
        Self { closure }
    }
}

#[async_trait]
impl<F> RunHandler for ClosureRunHandler<F>
where
    F: Fn() -> Result<(Value, HashMap<String, Value>), String> + Send + Sync,
{
    async fn execute(&self) -> Result<RunResult, DirectiveError> {
        let start = Instant::now();

        match (self.closure)() {
            Ok((data, signals)) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                Ok(RunResult::ok(data, signals, duration_ms))
            }
            Err(msg) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                Ok(RunResult::error("EXECUTION_ERROR", msg, duration_ms))
            }
        }
    }
}

/// Extract signals from a dataflow message's metadata.
fn extract_signals_from_message(message: &Message) -> HashMap<String, Value> {
    let metadata = message.metadata();

    // Look for signals in metadata.signals or the entire metadata
    if let Some(signals) = metadata.get("signals")
        && let Some(obj) = signals.as_object()
    {
        return obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    }

    HashMap::new()
}

/// Categorize an error into error_code and error_message.
///
/// This function attempts to classify errors into known categories:
/// - RATE_LIMITED - Rate limiting or throttling errors
/// - TIMEOUT - Timeout errors
/// - VALIDATION_ERROR - Input validation failures
/// - NETWORK_ERROR - Network connectivity issues
/// - INTERNAL_ERROR - Internal system errors
fn categorize_error(error: &dyn std::error::Error) -> (String, String) {
    let error_string = error.to_string().to_lowercase();

    let error_code = if error_string.contains("rate limit")
        || error_string.contains("too many requests")
        || error_string.contains("429")
    {
        "RATE_LIMITED"
    } else if error_string.contains("timeout") || error_string.contains("timed out") {
        "TIMEOUT"
    } else if error_string.contains("validation")
        || error_string.contains("invalid")
        || error_string.contains("400")
    {
        "VALIDATION_ERROR"
    } else if error_string.contains("network")
        || error_string.contains("connection")
        || error_string.contains("dns")
    {
        "NETWORK_ERROR"
    } else {
        "INTERNAL_ERROR"
    };

    (error_code.to_string(), error.to_string())
}

/// Known error codes for directive evaluation.
pub mod error_codes {
    /// Rate limiting or throttling error.
    pub const RATE_LIMITED: &str = "RATE_LIMITED";
    /// Timeout error.
    pub const TIMEOUT: &str = "TIMEOUT";
    /// Input validation failure.
    pub const VALIDATION_ERROR: &str = "VALIDATION_ERROR";
    /// Network connectivity issue.
    pub const NETWORK_ERROR: &str = "NETWORK_ERROR";
    /// Internal system error.
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
    /// Execution was canceled.
    pub const CANCELED: &str = "CANCELED";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_noop_handler() {
        let handler = NoOpRunHandler;
        let result = handler.execute().await.unwrap();

        assert!(result.is_ok());
        assert_eq!(result.duration_ms, 0);
    }

    #[tokio::test]
    async fn test_closure_handler_success() {
        let handler = ClosureRunHandler::new(|| {
            let mut signals = HashMap::new();
            signals.insert("processed".to_string(), json!(true));
            Ok((json!({"result": 42}), signals))
        });

        let result = handler.execute().await.unwrap();

        assert!(result.is_ok());
        assert_eq!(result.data, json!({"result": 42}));
        assert_eq!(result.signals.get("processed"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn test_closure_handler_error() {
        let handler = ClosureRunHandler::new(|| Err("Something went wrong".to_string()));

        let result = handler.execute().await.unwrap();

        assert!(!result.is_ok());
        assert_eq!(result.error_code, Some("EXECUTION_ERROR".to_string()));
        assert_eq!(
            result.error_message,
            Some("Something went wrong".to_string())
        );
    }

    #[test]
    fn test_categorize_error_rate_limit() {
        struct RateLimitError;
        impl std::fmt::Display for RateLimitError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "Rate limit exceeded")
            }
        }
        impl std::fmt::Debug for RateLimitError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "RateLimitError")
            }
        }
        impl std::error::Error for RateLimitError {}

        let (code, _) = categorize_error(&RateLimitError);
        assert_eq!(code, "RATE_LIMITED");
    }

    #[test]
    fn test_categorize_error_timeout() {
        struct TimeoutError;
        impl std::fmt::Display for TimeoutError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "Request timed out")
            }
        }
        impl std::fmt::Debug for TimeoutError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "TimeoutError")
            }
        }
        impl std::error::Error for TimeoutError {}

        let (code, _) = categorize_error(&TimeoutError);
        assert_eq!(code, "TIMEOUT");
    }

    #[test]
    fn test_categorize_error_validation() {
        struct ValidationError;
        impl std::fmt::Display for ValidationError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "Invalid input data")
            }
        }
        impl std::fmt::Debug for ValidationError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "ValidationError")
            }
        }
        impl std::error::Error for ValidationError {}

        let (code, _) = categorize_error(&ValidationError);
        assert_eq!(code, "VALIDATION_ERROR");
    }

    #[test]
    fn test_categorize_error_network() {
        struct NetworkError;
        impl std::fmt::Display for NetworkError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "Connection refused")
            }
        }
        impl std::fmt::Debug for NetworkError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "NetworkError")
            }
        }
        impl std::error::Error for NetworkError {}

        let (code, _) = categorize_error(&NetworkError);
        assert_eq!(code, "NETWORK_ERROR");
    }

    #[test]
    fn test_categorize_error_internal() {
        struct InternalError;
        impl std::fmt::Display for InternalError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "Unexpected error occurred")
            }
        }
        impl std::fmt::Debug for InternalError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "InternalError")
            }
        }
        impl std::error::Error for InternalError {}

        let (code, _) = categorize_error(&InternalError);
        assert_eq!(code, "INTERNAL_ERROR");
    }
}
