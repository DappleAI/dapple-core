//! Client for the `/api/brief-packs/` endpoints.

use crate::types::{
    BriefPack, BriefPackPath, CreateBriefPackSchema, CreateBriefPackSetSchema, DeleteObjectSchema,
    DeleteObjectSetSchema, FilterOptions, GetBriefPackPathSetSchema, GetBriefPackSetSchema,
    PatchBriefPackSchema, PatchBriefPackSetSchema,
};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

pub struct BriefPacksClient<'a> {
    client: &'a ClientCore,
}

impl<'a> BriefPacksClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    pub async fn get(&self, id: Uuid) -> Result<BriefPack, SdkError> {
        self.client.get_one(&format!("api/brief-packs/{id}")).await
    }

    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<BriefPack>, SdkError> {
        let query = GetBriefPackSetSchema::All { options };
        self.client.get_many("api/brief-packs/", &query).await
    }

    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<BriefPack>, SdkError> {
        let query = GetBriefPackSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/brief-packs/", &query).await
    }

    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<BriefPack>, SdkError> {
        let query = GetBriefPackSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/brief-packs/", &query).await
    }

    pub async fn create(
        &self,
        objs: Vec<CreateBriefPackSchema>,
    ) -> Result<Vec<BriefPack>, SdkError> {
        let body = CreateBriefPackSetSchema { objs };
        self.client.post_many("api/brief-packs/", &body).await
    }

    pub async fn patch(&self, objs: Vec<PatchBriefPackSchema>) -> Result<Vec<BriefPack>, SdkError> {
        let body = PatchBriefPackSetSchema { objs };
        self.client.patch_many("api/brief-packs/", &body).await
    }

    pub async fn delete(&self, ids: Vec<Uuid>) -> Result<Vec<BriefPack>, SdkError> {
        let body = DeleteObjectSetSchema {
            objs: ids
                .into_iter()
                .map(|id| DeleteObjectSchema { id, reason: None })
                .collect(),
        };
        self.client
            .delete_many_with_body("api/brief-packs/", &body)
            .await
    }

    pub async fn delete_one(&self, id: Uuid) -> Result<BriefPack, SdkError> {
        self.client
            .delete_one(&format!("api/brief-packs/{id}"))
            .await
    }

    pub async fn get_path(&self, id: Uuid) -> Result<BriefPackPath, SdkError> {
        self.client
            .get_one(&format!("api/brief-pack-paths/{id}"))
            .await
    }

    pub async fn list_paths_by_ids(&self, ids: Vec<Uuid>) -> Result<Vec<BriefPackPath>, SdkError> {
        let query = GetBriefPackPathSetSchema::ByObjectIds { ids };
        self.client.get_many("api/brief-pack-paths/", &query).await
    }
}
