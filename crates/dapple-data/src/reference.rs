use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;

// Re-export all consumer-facing reference types from dapple-types.
pub use dapple_types::reference::*;

// ----------------------
// Internal model references (UUID only — no BIGINT id)
// ----------------------

#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
pub struct CategoryModelReference {
    pub id: Uuid,
}

impl From<Uuid> for CategoryModelReference {
    fn from(v: Uuid) -> Self {
        CategoryModelReference { id: v }
    }
}

#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
pub struct StepModelReference {
    pub id: Uuid,
}

impl From<Uuid> for StepModelReference {
    fn from(v: Uuid) -> Self {
        StepModelReference { id: v }
    }
}

#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
pub struct TaskModelReference {
    pub id: Uuid,
}

impl From<Uuid> for TaskModelReference {
    fn from(v: Uuid) -> Self {
        TaskModelReference { id: v }
    }
}

#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
pub struct PathModelReference {
    pub id: Uuid,
}

impl From<Uuid> for PathModelReference {
    fn from(v: Uuid) -> Self {
        PathModelReference { id: v }
    }
}
