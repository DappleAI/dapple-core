//! Client for the `/api/stages/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    CreateStageSchema, CreateStageSetSchema, DeleteQueryParams, FilterOptions, GetStageSetSchema,
    Stage,
};
use uuid::Uuid;

/// Client for the `/api/stages/` endpoints.
pub struct StagesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> StagesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single stage by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Stage, SdkError> {
        self.client.get_one(&format!("api/stages/{id}")).await
    }

    /// List all stages.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Stage>, SdkError> {
        let query = GetStageSetSchema::All { options };
        self.client.get_many("api/stages/", &query).await
    }

    /// List stages by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Stage>, SdkError> {
        let query = GetStageSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/stages/", &query).await
    }

    /// List stages by slug names.
    pub async fn list_by_slugs(
        &self,
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Stage>, SdkError> {
        let query = GetStageSetSchema::BySlugs { slugs, options };
        self.client.get_many("api/stages/", &query).await
    }

    /// Create one or more stages.
    pub async fn create(&self, objs: Vec<CreateStageSchema>) -> Result<Vec<Stage>, SdkError> {
        let body = CreateStageSetSchema { objs };
        self.client.post_many("api/stages/", &body).await
    }

    /// Delete (deprecate) stages by path object IDs.
    pub async fn delete(
        &self,
        _path_object_ids: Vec<Uuid>,
        level: Option<String>,
    ) -> Result<Vec<Stage>, SdkError> {
        let query = DeleteQueryParams {
            level,
            reason: None,
        };
        self.client.delete_many("api/stages/", &query).await
    }

    /// Delete (deprecate) a single stage by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<Stage, SdkError> {
        self.client.delete_one(&format!("api/stages/{id}")).await
    }
}
