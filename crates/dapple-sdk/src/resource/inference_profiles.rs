//! Client for the `/api/inference-profiles/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    CreateInferenceProfileSchema, CreateInferenceProfileSetSchema, DeleteQueryParams,
    FilterOptions, GetInferenceProfileSetSchema, InferenceProfile,
};
use uuid::Uuid;

/// Client for the `/api/inference-profiles/` endpoints.
pub struct InferenceProfilesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> InferenceProfilesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single inference profile by object ID.
    pub async fn get(&self, id: Uuid) -> Result<InferenceProfile, SdkError> {
        self.client
            .get_one(&format!("api/inference-profiles/{id}"))
            .await
    }

    /// List all inference profiles.
    pub async fn list(
        &self,
        options: Option<FilterOptions>,
    ) -> Result<Vec<InferenceProfile>, SdkError> {
        let query = GetInferenceProfileSetSchema::All { options };
        self.client
            .get_many("api/inference-profiles/", &query)
            .await
    }

    /// List inference profiles by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<InferenceProfile>, SdkError> {
        let query = GetInferenceProfileSetSchema::ByObjectIds { ids, options };
        self.client
            .get_many("api/inference-profiles/", &query)
            .await
    }

    /// Create one or more inference profiles.
    pub async fn create(
        &self,
        objs: Vec<CreateInferenceProfileSchema>,
    ) -> Result<Vec<InferenceProfile>, SdkError> {
        let body = CreateInferenceProfileSetSchema { objs };
        self.client
            .post_many("api/inference-profiles/", &body)
            .await
    }

    /// Delete (deprecate) inference profiles by path object IDs.
    pub async fn delete(
        &self,
        _path_object_ids: Vec<Uuid>,
        level: Option<String>,
    ) -> Result<Vec<InferenceProfile>, SdkError> {
        let query = DeleteQueryParams {
            level,
            reason: None,
        };
        self.client
            .delete_many("api/inference-profiles/", &query)
            .await
    }

    /// Delete (deprecate) a single inference profile by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<InferenceProfile, SdkError> {
        self.client
            .delete_one(&format!("api/inference-profiles/{id}"))
            .await
    }
}
