//! DirectiveAction enum representing possible directive outcomes.

use super::error::DirectiveError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Actions that a directive can return after evaluation.
///
/// These actions control the flow of execution for stages and steps:
/// - `Proceed` - Continue to the next stage/step (default success)
/// - `Skip` - Skip this stage/step without execution
/// - `Fail` - Halt execution with an error
/// - `Retry` - Re-execute after retry_delay_ms (respects max_attempts)
/// - `Loop` - Re-execute immediately without delay (respects max_attempts)
/// - `Branch` - Jump to the stage/step with the given object_id
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveAction {
    /// Continue to the next stage/step (default success).
    Proceed,
    /// Skip this stage/step without execution.
    Skip,
    /// Halt execution with an error.
    Fail,
    /// Re-execute after retry_delay_ms (respects max_attempts).
    Retry,
    /// Re-execute immediately without delay (respects max_attempts).
    Loop,
    /// Branch to the stage/step with the given object_id.
    Branch(Uuid),
}

impl DirectiveAction {
    /// Parse a JSONLogic result into a DirectiveAction.
    ///
    /// # Arguments
    /// * `value` - The JSON value returned from JSONLogic evaluation
    ///
    /// # Returns
    /// * `Ok(DirectiveAction)` - The parsed action
    /// * `Err(DirectiveError)` - If the value is not a valid action
    ///
    /// # Examples
    /// ```ignore
    /// use dapple_prompt_flow::directives::DirectiveAction;
    /// use serde_json::json;
    ///
    /// let action = DirectiveAction::from_json(&json!("proceed")).unwrap();
    /// assert_eq!(action, DirectiveAction::Proceed);
    ///
    /// let action = DirectiveAction::from_json(&json!("skip")).unwrap();
    /// assert_eq!(action, DirectiveAction::Skip);
    /// ```
    pub fn from_json(value: &Value) -> Result<Self, DirectiveError> {
        match value {
            Value::String(s) => Self::from_str(s),
            Value::Bool(true) => Ok(Self::Proceed),
            Value::Bool(false) => Ok(Self::Fail),
            _ => Err(DirectiveError::InvalidActionType),
        }
    }

    /// Parse a string into a DirectiveAction.
    ///
    /// Known action strings: "proceed", "skip", "fail", "retry", "loop"
    /// Any other string is attempted to be parsed as a UUID for branching.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, DirectiveError> {
        match s.to_lowercase().as_str() {
            "proceed" => Ok(Self::Proceed),
            "skip" => Ok(Self::Skip),
            "fail" => Ok(Self::Fail),
            "retry" => Ok(Self::Retry),
            "loop" => Ok(Self::Loop),
            uuid_str => {
                // Attempt to parse as UUID for branch target
                Uuid::parse_str(uuid_str)
                    .map(Self::Branch)
                    .map_err(|_| DirectiveError::InvalidAction(s.to_string()))
            }
        }
    }

    /// Check if this action requires another iteration (retry or loop).
    pub fn is_iterating(&self) -> bool {
        matches!(self, Self::Retry | Self::Loop)
    }

    /// Check if this action is terminal (proceed, skip, fail, or branch).
    pub fn is_terminal(&self) -> bool {
        !self.is_iterating()
    }

    /// Convert the action to a string representation for logging/audit.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Proceed => "proceed",
            Self::Skip => "skip",
            Self::Fail => "fail",
            Self::Retry => "retry",
            Self::Loop => "loop",
            Self::Branch(_) => "branch",
        }
    }
}

impl std::fmt::Display for DirectiveAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Branch(uuid) => write!(f, "branch({})", uuid),
            _ => write!(f, "{}", self.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_proceed() {
        let action = DirectiveAction::from_json(&json!("proceed")).unwrap();
        assert_eq!(action, DirectiveAction::Proceed);
    }

    #[test]
    fn test_parse_skip() {
        let action = DirectiveAction::from_json(&json!("skip")).unwrap();
        assert_eq!(action, DirectiveAction::Skip);
    }

    #[test]
    fn test_parse_fail() {
        let action = DirectiveAction::from_json(&json!("fail")).unwrap();
        assert_eq!(action, DirectiveAction::Fail);
    }

    #[test]
    fn test_parse_retry() {
        let action = DirectiveAction::from_json(&json!("retry")).unwrap();
        assert_eq!(action, DirectiveAction::Retry);
    }

    #[test]
    fn test_parse_loop() {
        let action = DirectiveAction::from_json(&json!("loop")).unwrap();
        assert_eq!(action, DirectiveAction::Loop);
    }

    #[test]
    fn test_parse_branch_uuid() {
        let uuid = Uuid::now_v7();
        let action = DirectiveAction::from_json(&json!(uuid.to_string())).unwrap();
        assert_eq!(action, DirectiveAction::Branch(uuid));
    }

    #[test]
    fn test_parse_case_insensitive() {
        let action = DirectiveAction::from_json(&json!("PROCEED")).unwrap();
        assert_eq!(action, DirectiveAction::Proceed);

        let action = DirectiveAction::from_json(&json!("Skip")).unwrap();
        assert_eq!(action, DirectiveAction::Skip);
    }

    #[test]
    fn test_parse_bool_true_is_proceed() {
        let action = DirectiveAction::from_json(&json!(true)).unwrap();
        assert_eq!(action, DirectiveAction::Proceed);
    }

    #[test]
    fn test_parse_bool_false_is_fail() {
        let action = DirectiveAction::from_json(&json!(false)).unwrap();
        assert_eq!(action, DirectiveAction::Fail);
    }

    #[test]
    fn test_parse_invalid_string() {
        let result = DirectiveAction::from_json(&json!("invalid_action"));
        assert!(result.is_err());
        match result {
            Err(DirectiveError::InvalidAction(s)) => assert_eq!(s, "invalid_action"),
            _ => panic!("Expected InvalidAction error"),
        }
    }

    #[test]
    fn test_parse_invalid_type() {
        let result = DirectiveAction::from_json(&json!(42));
        assert!(result.is_err());
        match result {
            Err(DirectiveError::InvalidActionType) => {}
            _ => panic!("Expected InvalidActionType error"),
        }
    }

    #[test]
    fn test_is_iterating() {
        assert!(DirectiveAction::Retry.is_iterating());
        assert!(DirectiveAction::Loop.is_iterating());
        assert!(!DirectiveAction::Proceed.is_iterating());
        assert!(!DirectiveAction::Skip.is_iterating());
        assert!(!DirectiveAction::Fail.is_iterating());
        assert!(!DirectiveAction::Branch(Uuid::now_v7()).is_iterating());
    }

    #[test]
    fn test_is_terminal() {
        assert!(DirectiveAction::Proceed.is_terminal());
        assert!(DirectiveAction::Skip.is_terminal());
        assert!(DirectiveAction::Fail.is_terminal());
        assert!(DirectiveAction::Branch(Uuid::now_v7()).is_terminal());
        assert!(!DirectiveAction::Retry.is_terminal());
        assert!(!DirectiveAction::Loop.is_terminal());
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", DirectiveAction::Proceed), "proceed");
        assert_eq!(format!("{}", DirectiveAction::Skip), "skip");
        assert_eq!(format!("{}", DirectiveAction::Fail), "fail");
        assert_eq!(format!("{}", DirectiveAction::Retry), "retry");
        assert_eq!(format!("{}", DirectiveAction::Loop), "loop");

        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            format!("{}", DirectiveAction::Branch(uuid)),
            "branch(550e8400-e29b-41d4-a716-446655440000)"
        );
    }
}
