//! Client for the `/api/assets/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    Asset, AssetPath, CreateAssetSchema, CreateAssetSetSchema, FilterOptions,
    GetAssetPathSetSchema, GetAssetSetSchema, PatchAssetSchema, PatchAssetSetSchema,
};
use uuid::Uuid;

/// Client for the `/api/assets/` endpoints.
///
/// Use the `content` method to stream raw binary data.
pub struct AssetsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> AssetsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    // ========================================================================
    // Assets
    // ========================================================================

    /// Get a single asset's metadata by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Asset, SdkError> {
        self.client.get_one(&format!("api/assets/{id}")).await
    }

    /// List all assets.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Asset>, SdkError> {
        let query = GetAssetSetSchema::All { options };
        self.client.get_many("api/assets/", &query).await
    }

    /// List assets by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Asset>, SdkError> {
        let query = GetAssetSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/assets/", &query).await
    }

    /// List assets by path object IDs (all versions of each path).
    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Asset>, SdkError> {
        let query = GetAssetSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/assets/", &query).await
    }

    /// Create one or more assets.
    pub async fn create(&self, objs: Vec<CreateAssetSchema>) -> Result<Vec<Asset>, SdkError> {
        let body = CreateAssetSetSchema { objs };
        self.client.post_many("api/assets/", &body).await
    }

    /// Patch (version) one or more assets.
    pub async fn patch(&self, objs: Vec<PatchAssetSchema>) -> Result<Vec<Asset>, SdkError> {
        let body = PatchAssetSetSchema { objs };
        self.client.patch_many("api/assets/", &body).await
    }

    /// Get the raw binary content of an asset.
    ///
    /// Returns the full content as bytes. For large assets, consider using
    /// HTTP range requests at the transport level.
    pub async fn content(&self, id: Uuid) -> Result<bytes::Bytes, SdkError> {
        self.client
            .get_bytes(&format!("api/assets/{id}/content"))
            .await
    }

    // ========================================================================
    // Asset Paths
    // ========================================================================

    /// Get a single asset path by object ID.
    pub async fn get_path(&self, id: Uuid) -> Result<AssetPath, SdkError> {
        self.client.get_one(&format!("api/asset-paths/{id}")).await
    }

    /// List asset paths by object IDs.
    pub async fn list_paths_by_ids(&self, ids: Vec<Uuid>) -> Result<Vec<AssetPath>, SdkError> {
        let query = GetAssetPathSetSchema::ByObjectIds { ids };
        self.client.get_many("api/asset-paths/", &query).await
    }
}
