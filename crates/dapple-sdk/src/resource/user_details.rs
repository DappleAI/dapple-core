//! Client for the `/api/user-details/` endpoints.
//!
//! User details are managed through the standard query API.
//! Create and patch operations are handled internally by the API
//! and not exposed in the OpenAPI spec.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{FilterOptions, GetUserDetailsSetSchema, UserDetail};
use uuid::Uuid;

/// Client for the `/api/user-details/` endpoints.
pub struct UserDetailsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> UserDetailsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// List all user details.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<UserDetail>, SdkError> {
        let query = GetUserDetailsSetSchema::All { options };
        self.client.get_many("api/user-details/", &query).await
    }

    /// List user details by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<UserDetail>, SdkError> {
        let query = GetUserDetailsSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/user-details/", &query).await
    }
}
