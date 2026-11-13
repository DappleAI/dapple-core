//! Client for the `/api/prompts/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    CreatePromptSchema, CreatePromptSetSchema, DeleteQueryParams, FilterOptions,
    GetPromptPathSetSchema, GetPromptSetSchema, Prompt, PromptPath,
};
use uuid::Uuid;

/// Client for the `/api/prompts/` endpoints.
pub struct PromptsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> PromptsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    // ========================================================================
    // Prompts
    // ========================================================================

    /// Get a single prompt by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Prompt, SdkError> {
        self.client.get_one(&format!("api/prompts/{id}")).await
    }

    /// List all prompts.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Prompt>, SdkError> {
        let query = GetPromptSetSchema::All { options };
        self.client.get_many("api/prompts/", &query).await
    }

    /// List prompts by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Prompt>, SdkError> {
        let query = GetPromptSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/prompts/", &query).await
    }

    /// List prompts by path object IDs (all versions of each path).
    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Prompt>, SdkError> {
        let query = GetPromptSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/prompts/", &query).await
    }

    /// Create one or more prompts.
    pub async fn create(&self, objs: Vec<CreatePromptSchema>) -> Result<Vec<Prompt>, SdkError> {
        let body = CreatePromptSetSchema { objs };
        self.client.post_many("api/prompts/", &body).await
    }

    /// Delete (deprecate) prompts by path object IDs.
    pub async fn delete(
        &self,
        _path_object_ids: Vec<Uuid>,
        level: Option<String>,
    ) -> Result<Vec<Prompt>, SdkError> {
        let query = DeleteQueryParams {
            level,
            reason: None,
        };
        self.client.delete_many("api/prompts/", &query).await
    }

    /// Delete (deprecate) a single prompt by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<Prompt, SdkError> {
        self.client.delete_one(&format!("api/prompts/{id}")).await
    }

    // ========================================================================
    // Prompt Paths
    // ========================================================================

    /// Get a single prompt path by object ID.
    pub async fn get_path(&self, id: Uuid) -> Result<PromptPath, SdkError> {
        self.client.get_one(&format!("api/prompt-paths/{id}")).await
    }

    /// List prompt paths by object IDs.
    pub async fn list_paths_by_ids(&self, ids: Vec<Uuid>) -> Result<Vec<PromptPath>, SdkError> {
        let query = GetPromptPathSetSchema::ByObjectIds { ids };
        self.client.get_many("api/prompt-paths/", &query).await
    }
}
