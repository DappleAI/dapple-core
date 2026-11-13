//! Raw JWT Bearer token authentication provider.
//!
//! Passes a pre-obtained JWT directly as a Bearer token.

use super::AuthProvider;
use crate::SdkError;

/// Authentication via a raw Bearer JWT.
pub(crate) struct BearerAuth {
    token: String,
}

impl BearerAuth {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_string(),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AuthProvider for BearerAuth {
    async fn authorization_header(&self) -> Result<String, SdkError> {
        Ok(format!("Bearer {}", self.token))
    }

    async fn on_unauthorized(&self) {
        // Raw JWTs are static from the SDK's perspective
    }
}
