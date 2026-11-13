//! Client for the `/api/brief-groups/` endpoints.

use crate::types::{
    BriefGroup, BriefGroupPath, CreateBriefGroupSchema, CreateBriefGroupSetSchema,
    DeleteObjectSchema, DeleteObjectSetSchema, FilterOptions, GetBriefGroupPathSetSchema,
    GetBriefGroupSetSchema, PatchBriefGroupSchema, PatchBriefGroupSetSchema,
};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

pub struct BriefGroupsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> BriefGroupsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    pub async fn get(&self, id: Uuid) -> Result<BriefGroup, SdkError> {
        self.client.get_one(&format!("api/brief-groups/{id}")).await
    }

    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<BriefGroup>, SdkError> {
        let query = GetBriefGroupSetSchema::All { options };
        self.client.get_many("api/brief-groups/", &query).await
    }

    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<BriefGroup>, SdkError> {
        let query = GetBriefGroupSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/brief-groups/", &query).await
    }

    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<BriefGroup>, SdkError> {
        let query = GetBriefGroupSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/brief-groups/", &query).await
    }

    pub async fn create(
        &self,
        objs: Vec<CreateBriefGroupSchema>,
    ) -> Result<Vec<BriefGroup>, SdkError> {
        let body = CreateBriefGroupSetSchema { objs };
        self.client.post_many("api/brief-groups/", &body).await
    }

    pub async fn patch(
        &self,
        objs: Vec<PatchBriefGroupSchema>,
    ) -> Result<Vec<BriefGroup>, SdkError> {
        let body = PatchBriefGroupSetSchema { objs };
        self.client.patch_many("api/brief-groups/", &body).await
    }

    pub async fn delete(&self, ids: Vec<Uuid>) -> Result<Vec<BriefGroup>, SdkError> {
        let body = DeleteObjectSetSchema {
            objs: ids
                .into_iter()
                .map(|id| DeleteObjectSchema { id, reason: None })
                .collect(),
        };
        self.client
            .delete_many_with_body("api/brief-groups/", &body)
            .await
    }

    pub async fn delete_one(&self, id: Uuid) -> Result<BriefGroup, SdkError> {
        self.client
            .delete_one(&format!("api/brief-groups/{id}"))
            .await
    }

    pub async fn get_path(&self, id: Uuid) -> Result<BriefGroupPath, SdkError> {
        self.client
            .get_one(&format!("api/brief-group-paths/{id}"))
            .await
    }

    pub async fn list_paths_by_ids(&self, ids: Vec<Uuid>) -> Result<Vec<BriefGroupPath>, SdkError> {
        let query = GetBriefGroupPathSetSchema::ByObjectIds { ids };
        self.client.get_many("api/brief-group-paths/", &query).await
    }
}
