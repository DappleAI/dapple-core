//! Client for the `/api/value-slots/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    CreateValueSlotSchema, CreateValueSlotSetSchema, DeleteQueryParams, FilterOptions,
    GetValueSlotPathSetSchema, GetValueSlotSetSchema, PatchValueSlotSchema,
    PatchValueSlotSetSchema, ValueSlot, ValueSlotPath,
};
use uuid::Uuid;

/// Client for the `/api/value-slots/` endpoints.
pub struct ValueSlotsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ValueSlotsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    // ========================================================================
    // Value Slots
    // ========================================================================

    /// Get a single value slot by object ID.
    pub async fn get(&self, id: Uuid) -> Result<ValueSlot, SdkError> {
        self.client.get_one(&format!("api/value-slots/{id}")).await
    }

    /// List all value slots.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<ValueSlot>, SdkError> {
        let query = GetValueSlotSetSchema::All { options };
        self.client.get_many("api/value-slots/", &query).await
    }

    /// List value slots by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<ValueSlot>, SdkError> {
        let query = GetValueSlotSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/value-slots/", &query).await
    }

    /// List value slots by path object IDs (all versions of each path).
    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<ValueSlot>, SdkError> {
        let query = GetValueSlotSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/value-slots/", &query).await
    }

    /// Create one or more value slots.
    pub async fn create(
        &self,
        objs: Vec<CreateValueSlotSchema>,
    ) -> Result<Vec<ValueSlot>, SdkError> {
        let body = CreateValueSlotSetSchema { objs };
        self.client.post_many("api/value-slots/", &body).await
    }

    /// Patch (create new version of) one or more value slots.
    pub async fn patch(&self, objs: Vec<PatchValueSlotSchema>) -> Result<Vec<ValueSlot>, SdkError> {
        let body = PatchValueSlotSetSchema { objs };
        self.client.patch_many("api/value-slots/", &body).await
    }

    /// Delete (deprecate) value slots by path object IDs.
    pub async fn delete(
        &self,
        _path_object_ids: Vec<Uuid>,
        level: Option<String>,
    ) -> Result<Vec<ValueSlot>, SdkError> {
        let query = DeleteQueryParams {
            level,
            reason: None,
        };
        self.client.delete_many("api/value-slots/", &query).await
    }

    /// Delete (deprecate) a single value slot by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<ValueSlot, SdkError> {
        self.client
            .delete_one(&format!("api/value-slots/{id}"))
            .await
    }

    // ========================================================================
    // Value Slot Paths
    // ========================================================================

    /// Get a single value slot path by object ID.
    pub async fn get_path(&self, id: Uuid) -> Result<ValueSlotPath, SdkError> {
        self.client
            .get_one(&format!("api/value-slot-paths/{id}"))
            .await
    }

    /// List value slot paths by object IDs.
    pub async fn list_paths_by_ids(&self, ids: Vec<Uuid>) -> Result<Vec<ValueSlotPath>, SdkError> {
        let query = GetValueSlotPathSetSchema::ByObjectIds { ids };
        self.client.get_many("api/value-slot-paths/", &query).await
    }
}
