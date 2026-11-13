use serde::{Deserialize, Serialize};
use sqlx::types::{JsonValue, Uuid};
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::dispatch_task_attempts::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DispatchTaskAttemptModel {
    pub id: Uuid,
    pub dispatch_step_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub context_data: JsonValue,
    pub context_metadata: JsonValue,
    pub error_detail: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<DispatchTaskAttemptModel> for DispatchTaskAttempt {
    fn from(m: DispatchTaskAttemptModel) -> Self {
        DispatchTaskAttempt {
            id: m.id,
            dispatch_step_id: m.dispatch_step_id,
            task_id: m.task_id,
            status: m.status,
            context_data: m.context_data,
            context_metadata: m.context_metadata,
            error_detail: m.error_detail,
            created_at: m.created_at.into(),
        }
    }
}

/// Context data fetched for prompt execution
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AttemptContext {
    pub context_data: JsonValue,
    pub context_metadata: JsonValue,
    pub task_id: Uuid,
}
