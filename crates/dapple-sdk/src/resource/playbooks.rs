//! Client for the `/api/playbooks/` endpoints.

use crate::types::{
    CreatePlaybookSchema, CreatePlaybookSetSchema, DeleteObjectSetSchema, FilterOptions,
    GetPlaybookSetSchema, Playbook,
};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

/// Client for playbook operations.
pub struct PlaybooksClient<'a> {
    client: &'a ClientCore,
}

impl<'a> PlaybooksClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single playbook by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Playbook, SdkError> {
        self.client.get_one(&format!("api/playbooks/{id}")).await
    }

    /// List all playbooks.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Playbook>, SdkError> {
        let query = GetPlaybookSetSchema::All { options };
        self.client.get_many("api/playbooks/", &query).await
    }

    /// List playbooks by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Playbook>, SdkError> {
        let query = GetPlaybookSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/playbooks/", &query).await
    }

    /// List playbooks by slug names.
    pub async fn list_by_slugs(
        &self,
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Playbook>, SdkError> {
        let query = GetPlaybookSetSchema::BySlugs { slugs, options };
        self.client.get_many("api/playbooks/", &query).await
    }

    /// Create one or more playbooks.
    pub async fn create(&self, objs: Vec<CreatePlaybookSchema>) -> Result<Vec<Playbook>, SdkError> {
        let body = CreatePlaybookSetSchema { objs };
        self.client.post_many("api/playbooks/", &body).await
    }

    /// Delete (deprecate) playbooks.
    pub async fn delete(&self, objs: Vec<Uuid>) -> Result<Vec<Playbook>, SdkError> {
        let body = DeleteObjectSetSchema {
            objs: objs
                .into_iter()
                .map(|id| crate::types::DeleteObjectSchema { id, reason: None })
                .collect(),
        };
        self.client
            .delete_many_with_body("api/playbooks/", &body)
            .await
    }

    /// Delete (deprecate) a single playbook by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<Playbook, SdkError> {
        self.client.delete_one(&format!("api/playbooks/{id}")).await
    }
}
