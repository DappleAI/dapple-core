//! Client for the `/api/briefs/` endpoints.

use crate::types::{
    Brief, BriefPath, BriefUnit, CreateBriefSchema, CreateBriefSetSchema, DeleteObjectSchema,
    DeleteObjectSetSchema, FilterOptions, GetBriefPathSetSchema, GetBriefSetSchema,
    PatchBriefSchema, PatchBriefSetSchema,
};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

/// Client for brief operations.
pub struct BriefsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> BriefsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single brief by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Brief, SdkError> {
        self.client.get_one(&format!("api/briefs/{id}")).await
    }

    /// List all briefs.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Brief>, SdkError> {
        let query = GetBriefSetSchema::All { options };
        self.client.get_many("api/briefs/", &query).await
    }

    /// List briefs by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Brief>, SdkError> {
        let query = GetBriefSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/briefs/", &query).await
    }

    /// List briefs by path object IDs.
    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Brief>, SdkError> {
        let query = GetBriefSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/briefs/", &query).await
    }

    /// Create one or more briefs.
    pub async fn create(&self, objs: Vec<CreateBriefSchema>) -> Result<Vec<Brief>, SdkError> {
        let body = CreateBriefSetSchema { objs };
        self.client.post_many("api/briefs/", &body).await
    }

    /// Patch (create new version of) one or more briefs.
    pub async fn patch(&self, objs: Vec<PatchBriefSchema>) -> Result<Vec<Brief>, SdkError> {
        let body = PatchBriefSetSchema { objs };
        self.client.patch_many("api/briefs/", &body).await
    }

    /// Delete (deprecate) briefs.
    pub async fn delete(&self, ids: Vec<Uuid>) -> Result<Vec<Brief>, SdkError> {
        let body = DeleteObjectSetSchema {
            objs: ids
                .into_iter()
                .map(|id| DeleteObjectSchema { id, reason: None })
                .collect(),
        };
        self.client
            .delete_many_with_body("api/briefs/", &body)
            .await
    }

    /// Delete a single brief by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<Brief, SdkError> {
        self.client.delete_one(&format!("api/briefs/{id}")).await
    }

    /// List units for a specific brief.
    pub async fn list_units(&self, brief_id: Uuid) -> Result<Vec<BriefUnit>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/briefs/{brief_id}/units/"), &Empty {})
            .await
    }

    /// Get a single brief path by object ID.
    pub async fn get_path(&self, id: Uuid) -> Result<BriefPath, SdkError> {
        self.client.get_one(&format!("api/brief-paths/{id}")).await
    }

    /// List brief paths by object IDs.
    pub async fn list_paths_by_ids(&self, ids: Vec<Uuid>) -> Result<Vec<BriefPath>, SdkError> {
        let query = GetBriefPathSetSchema::ByObjectIds { ids };
        self.client.get_many("api/brief-paths/", &query).await
    }
}
