//! Client for the `/api/dispatches/` endpoints.

use crate::types::{
    AllQuery, ByJourneyQuery, ByObjectIdsQuery, CreateDispatchSchema, CreateDispatchSetSchema,
    CreateDispatchTaskAttempt, CreateDispatchTaskAttemptEvent, Dispatch, DispatchTaskAttempt,
    FilterOptions, GetDispatchSetSchema,
};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

/// Client for dispatch operations.
pub struct DispatchesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> DispatchesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single dispatch by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Dispatch, SdkError> {
        self.client.get_one(&format!("api/dispatches/{id}")).await
    }

    /// List all dispatches.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Dispatch>, SdkError> {
        let query = AllQuery::new(options);
        self.client.get_many("api/dispatches/", &query).await
    }

    /// List dispatches by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Dispatch>, SdkError> {
        let query = ByObjectIdsQuery {
            query: "by_object_ids",
            ids,
            options,
        };
        self.client.get_many("api/dispatches/", &query).await
    }

    /// List dispatches by journey ID.
    pub async fn list_by_journey(
        &self,
        journey_id: Uuid,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Dispatch>, SdkError> {
        let query = ByJourneyQuery {
            query: "by_journey",
            journey_id,
            options,
        };
        self.client.get_many("api/dispatches/", &query).await
    }

    /// List dispatches by user.
    pub async fn list_by_user(
        &self,
        user_id: Uuid,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Dispatch>, SdkError> {
        let query = GetDispatchSetSchema::ByUser { user_id, options };
        self.client.get_many("api/dispatches/", &query).await
    }

    /// List dispatches by playbook.
    pub async fn list_by_playbook(
        &self,
        playbook_id: Uuid,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Dispatch>, SdkError> {
        let query = GetDispatchSetSchema::ByPlaybook {
            playbook_id,
            options,
        };
        self.client.get_many("api/dispatches/", &query).await
    }

    /// Create one or more dispatches.
    pub async fn create(&self, objs: Vec<CreateDispatchSchema>) -> Result<Vec<Dispatch>, SdkError> {
        let body = CreateDispatchSetSchema { objs };
        self.client.post_many("api/dispatches/", &body).await
    }

    // ========================================================================
    // Task Attempts
    // ========================================================================

    /// List task attempts for a dispatch.
    pub async fn task_attempts(
        &self,
        dispatch_id: Uuid,
    ) -> Result<Vec<DispatchTaskAttempt>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(
                &format!("api/dispatches/{dispatch_id}/task_attempt/"),
                &Empty {},
            )
            .await
    }

    /// Get a specific task attempt.
    pub async fn task_attempt(
        &self,
        dispatch_id: Uuid,
        attempt_id: Uuid,
    ) -> Result<DispatchTaskAttempt, SdkError> {
        self.client
            .get_one(&format!(
                "api/dispatches/{dispatch_id}/task_attempt/{attempt_id}"
            ))
            .await
    }

    /// Create a task attempt for a dispatch.
    pub async fn create_task_attempt(
        &self,
        dispatch_id: Uuid,
        body: CreateDispatchTaskAttempt,
    ) -> Result<DispatchTaskAttempt, SdkError> {
        self.client
            .post_one(
                &format!("api/dispatches/{dispatch_id}/task_attempt/"),
                &body,
            )
            .await
    }

    /// Get events for a task attempt.
    pub async fn task_attempt_events(
        &self,
        dispatch_id: Uuid,
        attempt_id: Uuid,
    ) -> Result<Vec<serde_json::Value>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(
                &format!("api/dispatches/{dispatch_id}/task_attempt/{attempt_id}/events"),
                &Empty {},
            )
            .await
    }

    /// Create an event for a task attempt.
    pub async fn create_task_attempt_event(
        &self,
        dispatch_id: Uuid,
        attempt_id: Uuid,
        body: CreateDispatchTaskAttemptEvent,
    ) -> Result<serde_json::Value, SdkError> {
        self.client
            .post_one(
                &format!("api/dispatches/{dispatch_id}/task_attempt/{attempt_id}/events"),
                &body,
            )
            .await
    }
}
