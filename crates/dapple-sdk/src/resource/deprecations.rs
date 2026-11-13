//! Client for the `/api/deprecations/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    CreateDeprecationSchema, CreateDeprecationSetSchema, DeprecationEvent, FilterOptions,
    GetDeprecationSetSchema,
};
use uuid::Uuid;

/// Client for the `/api/deprecations/` endpoints.
pub struct DeprecationsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> DeprecationsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// List deprecation events by versioned object IDs.
    pub async fn list_by_versioned_object_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<DeprecationEvent>, SdkError> {
        let query = GetDeprecationSetSchema::ByVersionedObjectIds { ids, options };
        self.client.get_many("api/deprecations/", &query).await
    }

    /// Get current deprecation status for versioned objects.
    pub async fn status_by_object_ids(
        &self,
        ids: Vec<Uuid>,
    ) -> Result<Vec<DeprecationEvent>, SdkError> {
        let query = GetDeprecationSetSchema::StatusByObjectIds { ids };
        self.client.get_many("api/deprecations/", &query).await
    }

    /// Create one or more deprecation events.
    pub async fn create(
        &self,
        objs: Vec<CreateDeprecationSchema>,
    ) -> Result<Vec<DeprecationEvent>, SdkError> {
        let body = CreateDeprecationSetSchema { objs };
        self.client.post_many("api/deprecations/", &body).await
    }
}
