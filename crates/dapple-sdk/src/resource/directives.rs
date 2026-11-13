//! Client for the `/api/directives/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    CreateDirectiveSchema, CreateDirectiveSetSchema, DeleteQueryParams, Directive, DirectivePath,
    FilterOptions, GetDirectivePathSetSchema, GetDirectiveSetSchema, PatchDirectiveSchema,
    PatchDirectiveSetSchema,
};
use uuid::Uuid;

/// Client for the `/api/directives/` endpoints.
pub struct DirectivesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> DirectivesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    // ========================================================================
    // Directives
    // ========================================================================

    /// Get a single directive by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Directive, SdkError> {
        self.client.get_one(&format!("api/directives/{id}")).await
    }

    /// List all directives.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Directive>, SdkError> {
        let query = GetDirectiveSetSchema::All { options };
        self.client.get_many("api/directives/", &query).await
    }

    /// List directives by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Directive>, SdkError> {
        let query = GetDirectiveSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/directives/", &query).await
    }

    /// List directives by path object IDs (all versions of each path).
    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Directive>, SdkError> {
        let query = GetDirectiveSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/directives/", &query).await
    }

    /// Create one or more directives.
    pub async fn create(
        &self,
        objs: Vec<CreateDirectiveSchema>,
    ) -> Result<Vec<Directive>, SdkError> {
        let body = CreateDirectiveSetSchema { objs };
        self.client.post_many("api/directives/", &body).await
    }

    /// Patch (create new version of) one or more directives.
    pub async fn patch(&self, objs: Vec<PatchDirectiveSchema>) -> Result<Vec<Directive>, SdkError> {
        let body = PatchDirectiveSetSchema { objs };
        self.client.patch_many("api/directives/", &body).await
    }

    /// Delete (deprecate) directives by path object IDs.
    pub async fn delete(
        &self,
        _path_object_ids: Vec<Uuid>,
        level: Option<String>,
    ) -> Result<Vec<Directive>, SdkError> {
        let query = DeleteQueryParams {
            level,
            reason: None,
        };
        self.client.delete_many("api/directives/", &query).await
    }

    /// Delete (deprecate) a single directive by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<Directive, SdkError> {
        self.client
            .delete_one(&format!("api/directives/{id}"))
            .await
    }

    // ========================================================================
    // Directive Paths
    // ========================================================================

    /// Get a single directive path by object ID.
    pub async fn get_path(&self, id: Uuid) -> Result<DirectivePath, SdkError> {
        self.client
            .get_one(&format!("api/directive-paths/{id}"))
            .await
    }

    /// List directive paths by object IDs.
    pub async fn list_paths_by_ids(&self, ids: Vec<Uuid>) -> Result<Vec<DirectivePath>, SdkError> {
        let query = GetDirectivePathSetSchema::ByObjectIds { ids };
        self.client.get_many("api/directive-paths/", &query).await
    }
}
