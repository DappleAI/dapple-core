use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::background_jobs::*;

// ============================================================================
// Background Job Models
// ============================================================================

/// Full row from `rev.background_jobs`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BackgroundJobModel {
    pub id: Uuid,
    pub job_kind: String,
    pub target_id: Uuid,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub error_message: Option<String>,
    pub scheduled_at: OffsetDateTime,
    pub claimed_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

impl From<BackgroundJobModel> for BackgroundJob {
    fn from(m: BackgroundJobModel) -> Self {
        Self {
            id: m.id,
            job_kind: m.job_kind,
            target_id: m.target_id,
            payload: m.payload,
            status: m.status,
            attempts: m.attempts,
            max_attempts: m.max_attempts,
            error_message: m.error_message,
            scheduled_at: m.scheduled_at.into(),
            claimed_at: m.claimed_at.map(Into::into),
            completed_at: m.completed_at.map(Into::into),
            created_at: m.created_at.into(),
        }
    }
}

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for background jobs.
/// All vectors MUST be the same length.
#[derive(Clone, Debug, Default)]
pub struct InsertBackgroundJobArgs {
    pub job_kinds: Vec<String>,
    pub target_ids: Vec<Uuid>,
    pub payloads: Vec<serde_json::Value>,
}
