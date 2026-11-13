//! Client for the `/api/tasks/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    CreateTaskSchema, CreateTaskSetSchema, DeleteQueryParams, FilterOptions, GetTaskSetSchema, Task,
};
use uuid::Uuid;

/// Client for the `/api/tasks/` endpoints.
pub struct TasksClient<'a> {
    client: &'a ClientCore,
}

impl<'a> TasksClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single task by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Task, SdkError> {
        self.client.get_one(&format!("api/tasks/{id}")).await
    }

    /// List all tasks.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Task>, SdkError> {
        let query = GetTaskSetSchema::All { options };
        self.client.get_many("api/tasks/", &query).await
    }

    /// List tasks by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Task>, SdkError> {
        let query = GetTaskSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/tasks/", &query).await
    }

    /// List tasks by slug names.
    pub async fn list_by_slugs(
        &self,
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Task>, SdkError> {
        let query = GetTaskSetSchema::BySlugs { slugs, options };
        self.client.get_many("api/tasks/", &query).await
    }

    /// Create one or more tasks.
    pub async fn create(&self, objs: Vec<CreateTaskSchema>) -> Result<Vec<Task>, SdkError> {
        let body = CreateTaskSetSchema { objs };
        self.client.post_many("api/tasks/", &body).await
    }

    /// Delete (deprecate) tasks by path object IDs.
    pub async fn delete(
        &self,
        _path_object_ids: Vec<Uuid>,
        level: Option<String>,
    ) -> Result<Vec<Task>, SdkError> {
        let query = DeleteQueryParams {
            level,
            reason: None,
        };
        self.client.delete_many("api/tasks/", &query).await
    }

    /// Delete (deprecate) a single task by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<Task, SdkError> {
        self.client.delete_one(&format!("api/tasks/{id}")).await
    }
}
