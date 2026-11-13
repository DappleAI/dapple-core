use dapple_data::model::dispatch_task_attempts::AttemptContext;
use dapple_data::query::dispatch_task_attempts::select_context_by_attempt_id;
use dapple_data::DbPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::ConduitError;

/// Service for fetching context data from dispatch task attempts
pub struct ContextService {
    pool: Arc<DbPool>,
}

impl ContextService {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }

    /// Fetch context data for a dispatch task attempt
    pub async fn fetch_context_for_attempt(
        &self,
        attempt_id: Uuid,
    ) -> Result<AttemptContext, ConduitError> {
        select_context_by_attempt_id(&self.pool, attempt_id)
            .await
            .map_err(|e| {
                log::error!("Failed to fetch context for attempt {}: {}", attempt_id, e);
                ConduitError::NotFound(format!("Attempt {} not found", attempt_id))
            })
    }
}
