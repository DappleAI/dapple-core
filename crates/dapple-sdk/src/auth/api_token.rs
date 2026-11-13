//! API token authentication provider.
//!
//! Uses a static `dpl_` prefixed token passed as a Bearer token.

use super::AuthProvider;
use crate::SdkError;

/// Authentication via a static API token (dpl_ prefix).
pub(crate) struct ApiTokenAuth {
    token: String,
}

impl ApiTokenAuth {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_string(),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AuthProvider for ApiTokenAuth {
    async fn authorization_header(&self) -> Result<String, SdkError> {
        Ok(format!("Bearer {}", self.token))
    }

    async fn on_unauthorized(&self) {
        // API tokens are static, nothing to refresh
    }
}
