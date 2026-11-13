//! Client for the `/api/categories/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    Category, CreateCategorySchema, CreateCategorySetSchema, FilterOptions, GetCategorySetSchema,
};
use uuid::Uuid;

/// Client for the `/api/categories/` endpoints.
pub struct CategoriesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> CategoriesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single category by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Category, SdkError> {
        self.client.get_one(&format!("api/categories/{id}")).await
    }

    /// List all categories.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Category>, SdkError> {
        let query = GetCategorySetSchema::All { options };
        self.client.get_many("api/categories/", &query).await
    }

    /// List categories by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Category>, SdkError> {
        let query = GetCategorySetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/categories/", &query).await
    }

    /// List categories by slug names.
    pub async fn list_by_slugs(
        &self,
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Category>, SdkError> {
        let query = GetCategorySetSchema::BySlugs { slugs, options };
        self.client.get_many("api/categories/", &query).await
    }

    /// Create one or more categories.
    pub async fn create(&self, objs: Vec<CreateCategorySchema>) -> Result<Vec<Category>, SdkError> {
        let body = CreateCategorySetSchema { objs };
        self.client.post_many("api/categories/", &body).await
    }
}
