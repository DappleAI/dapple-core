//! Client for the `/api/versioned-objects/` endpoints.
//!
//! Provides version history queries that work across all versioned entity types.

use crate::types::{GetVersionedObjectSetSchema, VersionedObject};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

/// Client for versioned object operations.
pub struct VersionedObjectsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> VersionedObjectsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// List all versions for the given path IDs.
    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
    ) -> Result<Vec<VersionedObject>, SdkError> {
        let query = GetVersionedObjectSetSchema::ByPathIds {
            path_ids,
            options: None,
        };
        self.client.get_many("api/versioned-objects/", &query).await
    }
}
