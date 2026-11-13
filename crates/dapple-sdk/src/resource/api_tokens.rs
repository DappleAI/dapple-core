//! Client for the `/api/api-tokens/` endpoints.
//!
//! API tokens are user-scoped and used for programmatic authentication.
//! The plaintext token is only returned once at creation time.
//!
//! Note: These endpoints require JWT authentication (not API token auth).

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    ApiTokenInfo, CreateApiTokenResponse, CreateApiTokenSchema, ListApiTokensResponse,
    RevokeApiTokenResponse,
};
use uuid::Uuid;

/// Client for the `/api/api-tokens/` endpoints.
pub struct ApiTokensClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ApiTokensClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// List all API tokens for the authenticated user.
    ///
    /// Returns token metadata (not the secret itself).
    pub async fn list(&self) -> Result<Vec<ApiTokenInfo>, SdkError> {
        // The list endpoint returns a custom envelope with `tokens` field
        // instead of the standard `objs` envelope, so we parse the raw bytes.
        let bytes = self.client.get_bytes("api/api-tokens/").await?;
        let resp: ListApiTokensResponse = serde_json::from_slice(&bytes)?;
        Ok(resp.tokens)
    }

    /// Create a new API token.
    ///
    /// The response contains the plaintext token which can only be retrieved
    /// at creation time. Store it securely.
    pub async fn create(
        &self,
        schema: CreateApiTokenSchema,
    ) -> Result<CreateApiTokenResponse, SdkError> {
        // The create endpoint returns a custom response envelope,
        // not the standard PostResponse wrapper.
        let val: serde_json::Value = self.client.post_raw("api/api-tokens/", &schema).await?;
        serde_json::from_value(val).map_err(SdkError::from)
    }

    /// Revoke (delete) an API token by object ID.
    pub async fn revoke(&self, id: Uuid) -> Result<RevokeApiTokenResponse, SdkError> {
        self.client
            .delete_one::<RevokeApiTokenResponse>(&format!("api/api-tokens/{id}"))
            .await
    }
}
