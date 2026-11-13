//! Client for the `/api/steps/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    CreateStepSchema, CreateStepSetSchema, DeleteQueryParams, FilterOptions, GetStepSetSchema, Step,
};
use uuid::Uuid;

/// Client for the `/api/steps/` endpoints.
pub struct StepsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> StepsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single step by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Step, SdkError> {
        self.client.get_one(&format!("api/steps/{id}")).await
    }

    /// List all steps.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Step>, SdkError> {
        let query = GetStepSetSchema::All { options };
        self.client.get_many("api/steps/", &query).await
    }

    /// List steps by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Step>, SdkError> {
        let query = GetStepSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/steps/", &query).await
    }

    /// List steps by slug names.
    pub async fn list_by_slugs(
        &self,
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Step>, SdkError> {
        let query = GetStepSetSchema::BySlugs { slugs, options };
        self.client.get_many("api/steps/", &query).await
    }

    /// Create one or more steps.
    pub async fn create(&self, objs: Vec<CreateStepSchema>) -> Result<Vec<Step>, SdkError> {
        let body = CreateStepSetSchema { objs };
        self.client.post_many("api/steps/", &body).await
    }

    /// Delete (deprecate) steps by path object IDs.
    pub async fn delete(
        &self,
        _path_object_ids: Vec<Uuid>,
        level: Option<String>,
    ) -> Result<Vec<Step>, SdkError> {
        let query = DeleteQueryParams {
            level,
            reason: None,
        };
        self.client.delete_many("api/steps/", &query).await
    }

    /// Delete (deprecate) a single step by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<Step, SdkError> {
        self.client.delete_one(&format!("api/steps/{id}")).await
    }
}
