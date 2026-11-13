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
    async fn exchange_token(&self) -> Result<String, SdkError> {
        use ed25519_dalek::Signer;

        // Create payload: service_id + timestamp
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_secs() as i64;
        let payload = format!("{}:{}", self.service_id, timestamp);
        let signature = self.signing_key.sign(payload.as_bytes());

        // Encode signature as hex manually (avoid hex crate dependency)
        let sig_bytes = signature.to_bytes();
        let sig_hex: String = sig_bytes.iter().map(|b| format!("{b:02x}")).collect();

        let body = serde_json::json!({
            "service_id": self.service_id,
            "timestamp": timestamp,
            "signature": sig_hex,
        });

        let url = format!("{}/api/auth/service-token", self.base_url);
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
            token: String,
        }

        let token_resp: TokenResponse = resp.json().await.map_err(SdkError::Http)?;
        Ok(token_resp.token)
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
