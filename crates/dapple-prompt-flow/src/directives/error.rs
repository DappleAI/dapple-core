//! Error types for directive evaluation and execution.

use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur during directive evaluation and execution.
#[derive(Debug, Error)]
pub enum DirectiveError {
    /// Invalid action string returned from JSONLogic evaluation.
    #[error("Invalid action string: {0}")]
    InvalidAction(String),

    /// Action must be a string, but got a different type.
    #[error("Action must be a string, got different type")]
    InvalidActionType,

    /// JSONLogic evaluation failed.
    #[error("JSONLogic evaluation failed: {0}")]
    EvaluationFailed(String),

    /// Run handler execution failed.
    #[error("Run handler execution failed: {0}")]
    ExecutionFailed(String),

    /// Branch target stage/step not found.
    #[error("Branch target not found: {0}")]
    BranchTargetNotFound(Uuid),

    /// Step directive attempted to branch outside its stage.
    #[error("Branch target out of scope: step directives can only branch within same stage")]
    BranchOutOfScope,

    /// Database error during directive operations.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Data layer error.
    #[error("Data error: {0}")]
    Data(#[from] dapple_data::error::DataError),

    /// Directive logic is missing the required `run` operator.
    #[error("Directive has no `run` operator")]
    MissingRunOperator,

    /// Directive has multiple `run` operators (only one is allowed).
    #[error("Directive has multiple `run` operators; only one is allowed")]
    MultipleRunOperators,

    /// Maximum attempts exhausted without success.
    #[error("Maximum attempts ({0}) exhausted")]
    MaxAttemptsExhausted(u32),
}
