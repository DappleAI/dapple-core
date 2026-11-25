//! Service token (Ed25519) authentication provider.
//!
//! Signs a service token with Ed25519 and exchanges it for a JWT.
//! The JWT is cached and refreshed on 401 responses.
//!
//! Enable with the `service-auth` feature flag.

use super::AuthProvider;
use crate::SdkError;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Validity window (seconds) for a signed token; the server caps it at 300.
const DEFAULT_TOKEN_VALIDITY_SECS: u64 = 60;

/// Authentication via Ed25519 service token exchange (`dps_` prefix).
///
/// Signs a short-lived token using the provided Ed25519 private key,
/// exchanges it with the API for a JWT, and caches the JWT for reuse.
/// On 401, the cached JWT is invalidated and re-exchanged.
pub struct ServiceTokenAuth {
    service_id: String,
    signing_key: ed25519_dalek::SigningKey,
    base_url: String,
    http: reqwest::Client,
    cached_jwt: Arc<RwLock<Option<String>>>,
}

impl ServiceTokenAuth {
    /// Create a new service token auth provider.
    ///
    /// - `service_id`: The service identifier (e.g., "dapple_ui")
    /// - `signing_key_bytes`: 32-byte Ed25519 private key
    /// - `base_url`: API base URL for token exchange
    pub fn new(service_id: &str, signing_key_bytes: &[u8; 32], base_url: &str) -> Self {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(signing_key_bytes);
        Self {
            service_id: service_id.to_string(),
            signing_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
            cached_jwt: Arc::new(RwLock::new(None)),
        }
    }

    /// Exchange the signed service token for a JWT.
    ///
    /// `POST /api/get-service-token` with `{ "service_id", "token" }`, where
    /// `token` is the base64 structured token (no `dps_` prefix); the server
    /// answers `{ "status", "jwt", "service_id", "expires_at" }`.
    async fn exchange_token(&self) -> Result<String, SdkError> {
        let body = serde_json::json!({
            "service_id": self.service_id,
            "token": super::sign_structured_token(
                &self.signing_key,
                &self.service_id,
                DEFAULT_TOKEN_VALIDITY_SECS,
            ),
        });

        let url = format!("{}/api/get-service-token", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(SdkError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(SdkError::Api {
                status,
                message: format!("Service token exchange failed: {text}"),
            });
        }

        #[derive(serde::Deserialize)]
        struct TokenResponse {
            jwt: String,
        }

        let token_resp: TokenResponse = resp.json().await.map_err(SdkError::Http)?;
        Ok(token_resp.jwt)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AuthProvider for ServiceTokenAuth {
    async fn authorization_header(&self) -> Result<String, SdkError> {
        // Check for cached JWT
        {
            let cached = self.cached_jwt.read().await;
            if let Some(jwt) = cached.as_ref() {
                return Ok(format!("Bearer {jwt}"));
            }
        }

        // Exchange for new JWT
        let jwt = self.exchange_token().await?;
        {
            let mut cached = self.cached_jwt.write().await;
            *cached = Some(jwt.clone());
        }
        Ok(format!("Bearer {jwt}"))
    }

    async fn on_unauthorized(&self) {
        // Invalidate cached JWT so next request re-exchanges
        let mut cached = self.cached_jwt.write().await;
        *cached = None;
    }
}
