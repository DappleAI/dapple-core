//! External-surface dispatches client (`external` feature gated).
//!
//! Narrow customer-facing slice of
//! [`crate::resource::dispatches::DispatchesClient`]. Operator-runner methods
//! only — list-by-* variants and write-side task-attempt methods are absent
//! by construction; the workflow engine creates attempts and events
//! server-side, and the operator-runner reads them.
//!
//! - `create(objs)` — `POST /api/dispatches` (namespace-validates every
//!   `journey_id` and auto-spawns in-process execution for any dispatch
//!   whose journey is already fully bound).
//! - `get(id)` — `GET /api/dispatches/{id}` (poll dispatch status —
//!   stages/steps/tasks nested DTO).
//! - `task_attempts(id)` — `GET /api/dispatches/{id}/task_attempts` (per-task
//!   runtime phase for runner phase derivation). Returns the operator-facing
//!   [`ExternalDispatchTaskAttempt`], which exposes the author-emitted `signals`
//!   channel in place of the internal `context_metadata` grab-bag.

use crate::types::{
    CreateDispatchSchema, CreateDispatchSetSchema, Dispatch, ExternalDispatchTaskAttempt,
};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

/// Customer-side dispatches client. Reached via
/// [`crate::ExternalDappleClient::dispatches`].
pub struct ExternalDispatchesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ExternalDispatchesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// POST /api/dispatches — create one or more dispatches. The server
    /// namespace-validates every `journey_id` and auto-spawns in-process
    /// execution for any dispatch whose journey is already fully bound.
    pub async fn create(&self, objs: Vec<CreateDispatchSchema>) -> Result<Vec<Dispatch>, SdkError> {
        let body = CreateDispatchSetSchema { objs };
        self.client.post_many("api/dispatches", &body).await
    }

    /// GET /api/dispatches/{id} — poll dispatch status. Returns the
    /// stages/steps/tasks nested structure used by the runner panel.
    pub async fn get(&self, id: Uuid) -> Result<Dispatch, SdkError> {
        self.client.get_one(&format!("api/dispatches/{id}")).await
    }

    /// GET /api/dispatches/{id}/task_attempts — per-task runtime phase for
    /// runner phase derivation (Running / Failed / Ok). Returns the
    /// operator-facing [`ExternalDispatchTaskAttempt`]: `error_detail` /
    /// `context_data` plus the author-emitted `signals` channel
    /// (`context_metadata.signals`). The internal `context_metadata` grab-bag
    /// is never on this surface.
    pub async fn task_attempts(
        &self,
        id: Uuid,
    ) -> Result<Vec<ExternalDispatchTaskAttempt>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/dispatches/{id}/task_attempts"), &Empty {})
            .await
    }
}
