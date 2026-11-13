//! Client for the `/api/hats/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{CreateHatSchema, CreateHatSetSchema, FilterOptions, GetHatSetSchema, Hat};
use uuid::Uuid;

/// Client for the `/api/hats/` endpoints.
pub struct HatsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> HatsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single hat by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Hat, SdkError> {
        self.client.get_one(&format!("api/hats/{id}")).await
    }

    /// List all hats.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Hat>, SdkError> {
        let query = GetHatSetSchema::All { options };
        self.client.get_many("api/hats/", &query).await
    }

    /// List hats by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Hat>, SdkError> {
        let query = GetHatSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/hats/", &query).await
    }

    /// List hats by slug names.
    pub async fn list_by_slugs(
        &self,
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Hat>, SdkError> {
        let query = GetHatSetSchema::BySlugs { slugs, options };
        self.client.get_many("api/hats/", &query).await
    }

    /// Create one or more hats.
    pub async fn create(&self, objs: Vec<CreateHatSchema>) -> Result<Vec<Hat>, SdkError> {
        let body = CreateHatSetSchema { objs };
        self.client.post_many("api/hats/", &body).await
    }
}
