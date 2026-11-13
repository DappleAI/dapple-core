//! Client for the `/api/fluent-messages/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    CreateFluentMessageSchema, CreateFluentMessageSetSchema, DeleteQueryParams, FilterOptions,
    FluentMessage, FluentMessagePath, GetFluentMessagePathSetSchema, GetFluentMessageSetSchema,
    PatchFluentMessageSchema, PatchFluentMessageSetSchema,
};
use uuid::Uuid;

/// Client for the `/api/fluent-messages/` endpoints.
pub struct FluentMessagesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> FluentMessagesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    // ========================================================================
    // Fluent Messages
    // ========================================================================

    /// Get a single fluent message by object ID.
    pub async fn get(&self, id: Uuid) -> Result<FluentMessage, SdkError> {
        self.client
            .get_one(&format!("api/fluent-messages/{id}"))
            .await
    }

    /// List all fluent messages.
    pub async fn list(
        &self,
        options: Option<FilterOptions>,
    ) -> Result<Vec<FluentMessage>, SdkError> {
        let query = GetFluentMessageSetSchema::All { options };
        self.client.get_many("api/fluent-messages/", &query).await
    }

    /// List fluent messages by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<FluentMessage>, SdkError> {
        let query = GetFluentMessageSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/fluent-messages/", &query).await
    }

    /// List fluent messages by path object IDs (all versions of each path).
    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<FluentMessage>, SdkError> {
        let query = GetFluentMessageSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/fluent-messages/", &query).await
    }

    /// Create one or more fluent messages.
    pub async fn create(
        &self,
        objs: Vec<CreateFluentMessageSchema>,
    ) -> Result<Vec<FluentMessage>, SdkError> {
        let body = CreateFluentMessageSetSchema { objs };
        self.client.post_many("api/fluent-messages/", &body).await
    }

    /// Patch (create new version of) one or more fluent messages.
    pub async fn patch(
        &self,
        objs: Vec<PatchFluentMessageSchema>,
    ) -> Result<Vec<FluentMessage>, SdkError> {
        let body = PatchFluentMessageSetSchema { objs };
        self.client.patch_many("api/fluent-messages/", &body).await
    }

    /// Delete (deprecate) fluent messages by path object IDs.
    pub async fn delete(
        &self,
        _path_object_ids: Vec<Uuid>,
        level: Option<String>,
    ) -> Result<Vec<FluentMessage>, SdkError> {
        let query = DeleteQueryParams {
            level,
            reason: None,
        };
        self.client
            .delete_many("api/fluent-messages/", &query)
            .await
    }

    /// Delete (deprecate) a single fluent message by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<FluentMessage, SdkError> {
        self.client
            .delete_one(&format!("api/fluent-messages/{id}"))
            .await
    }

    // ========================================================================
    // Fluent Message Paths
    // ========================================================================

    /// Get a single fluent message path by object ID.
    pub async fn get_path(&self, id: Uuid) -> Result<FluentMessagePath, SdkError> {
        self.client
            .get_one(&format!("api/fluent-message-paths/{id}"))
            .await
    }

    /// List fluent message paths by object IDs.
    pub async fn list_paths_by_ids(
        &self,
        ids: Vec<Uuid>,
    ) -> Result<Vec<FluentMessagePath>, SdkError> {
        let query = GetFluentMessagePathSetSchema::ByObjectIds { ids };
        self.client
            .get_many("api/fluent-message-paths/", &query)
            .await
    }
}
