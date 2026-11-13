//! Client for the `/api/dispatch-task-attempts/` endpoints.
//!
//! Dispatch task attempts are read-only and queried by dispatch or step.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{DispatchTaskAttempt, GetDispatchTaskAttemptSetSchema};
use uuid::Uuid;

/// Client for the `/api/dispatch-task-attempts/` endpoints.
pub struct DispatchTaskAttemptsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> DispatchTaskAttemptsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// List dispatch task attempts by dispatch object IDs.
    pub async fn list_by_dispatch_ids(
        &self,
        dispatch_ids: Vec<Uuid>,
    ) -> Result<Vec<DispatchTaskAttempt>, SdkError> {
        let query = GetDispatchTaskAttemptSetSchema::ByDispatchIds { dispatch_ids };
        self.client
            .get_many("api/dispatch-task-attempts/", &query)
            .await
    }

    /// List dispatch task attempts by dispatch step object IDs.
    pub async fn list_by_step_ids(
        &self,
        step_ids: Vec<Uuid>,
    ) -> Result<Vec<DispatchTaskAttempt>, SdkError> {
        let query = GetDispatchTaskAttemptSetSchema::ByStepIds { step_ids };
        self.client
            .get_many("api/dispatch-task-attempts/", &query)
            .await
    }
}
