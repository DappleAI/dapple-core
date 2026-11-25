//! External-surface Ed25519 service token authentication.
//!
//! Sister to `service_token.rs` but for the **external** API surface
//! (`external-api.dapple.x`). The two share the signing primitive, the
//! token format ([`super::sign_structured_token`]) and the exchange endpoint;
//! they differ in which signing keys the server accepts:
//!
//! - **Endpoint:** `POST /api/get-service-token` on each surface.
//! - **Token payload:** a 128-byte structured blob — 64 bytes of Ed25519
//!   signature followed by a 64-byte message:
//!   `[version (1)][valid_from (8 BE)][valid_until (8 BE)][sid_len (1)][sid bytes][zero padding]`.
//!   Base64-encoded for transport.
//! - **Wire body:** `{ "service_id": ..., "token": "<base64>" }`.
//! - **Response:** `{ "status": "ok", "jwt": "...", "service_id": "...", "company_slug": "..." }`.
//!
//! Mirrors what
//! `crates/dapple-prompt-external-api/tests/external_lifecycle.rs::sign_test_token`
//! constructs on the test side and what
//! `dapple_prompt_api::verify_service_token` validates on the server side.
//!
//! Enable with the `service-auth` feature flag.

use super::AuthProvider;
use crate::SdkError;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default validity window (seconds) for a signed external service token.
/// The server's clock-skew tolerance is generous; one minute is plenty.
const DEFAULT_TOKEN_VALIDITY_SECS: u64 = 60;

/// Authentication via Ed25519 service token exchange against the
/// `external-api.dapple.x` surface.
pub struct ExternalServiceTokenAuth {
    service_id: String,
    signing_key: ed25519_dalek::SigningKey,
    base_url: String,
    http: reqwest::Client,
    cached_jwt: Arc<RwLock<Option<String>>>,
}

impl ExternalServiceTokenAuth {
    /// Create a new external-surface service token auth provider.
    ///
    /// - `service_id`: The service identifier from `rev.external_service_tokens.service_id`.
    /// - `signing_key_bytes`: 32-byte Ed25519 private key (matching the
    ///   public key stored in `rev.external_service_tokens.public_key_pem`).
    /// - `base_url`: External-surface base URL (e.g.,
    ///   `https://external-api.dapple.x`).
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

    /// Build the base64 structured token `verify_service_token` expects.
    fn sign_token(&self) -> String {
        super::sign_structured_token(
            &self.signing_key,
            &self.service_id,
            DEFAULT_TOKEN_VALIDITY_SECS,
        )
    }

    /// Sign + exchange for a JWT against the external surface.
    async fn exchange_token(&self) -> Result<String, SdkError> {
        let signed = self.sign_token();
        let body = serde_json::json!({
            "service_id": self.service_id,
            "token": signed,
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
                message: format!("External service token exchange failed: {text}"),
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
impl AuthProvider for ExternalServiceTokenAuth {
    async fn authorization_header(&self) -> Result<String, SdkError> {
        {
            let cached = self.cached_jwt.read().await;
            if let Some(jwt) = cached.as_ref() {
                return Ok(format!("Bearer {jwt}"));
            }
        }

        let jwt = self.exchange_token().await?;
        {
            let mut cached = self.cached_jwt.write().await;
            *cached = Some(jwt.clone());
        }
        Ok(format!("Bearer {jwt}"))
    }

    async fn on_unauthorized(&self) {
        let mut cached = self.cached_jwt.write().await;
        *cached = None;
    }
}
