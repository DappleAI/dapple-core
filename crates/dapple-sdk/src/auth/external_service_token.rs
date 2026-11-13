//! External-surface Ed25519 service token authentication.
//!
//! Sister to `service_token.rs` but for the **external** API surface
//! (`external-api.dapple.x`). The two share the same signing primitive
//! (Ed25519) but differ in the token payload format and exchange endpoint:
//!
//! - **Endpoint:** `POST /api/get-service-token` (vs internal's
//!   `POST /api/auth/service-token`).
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
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
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

    /// Build the 128-byte structured token payload (sig || message) and
    /// base64-encode it. Match the format `verify_service_token` expects.
    fn sign_token(&self) -> String {
        use ed25519_dalek::Signer;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_secs();
        let valid_from = now_secs;
        let valid_until = now_secs + DEFAULT_TOKEN_VALIDITY_SECS;

        let mut message = [0u8; 64];
        message[0] = 0x01; // version
        message[1..9].copy_from_slice(&valid_from.to_be_bytes());
        message[9..17].copy_from_slice(&valid_until.to_be_bytes());
        let sid_bytes = self.service_id.as_bytes();
        // sid_len is one byte, sid_bytes follows; total message len is 64
        // bytes so the sid must fit (with header) — practically sid ≤ 46
        // bytes. Real service_ids are short (e.g. "acme_prod"); this
        // bound has never come up.
        assert!(
            sid_bytes.len() <= 32,
            "service_id too long for external token format"
        );
        message[17] = sid_bytes.len() as u8;
        message[18..18 + sid_bytes.len()].copy_from_slice(sid_bytes);

        let signature = self.signing_key.sign(&message);

        let mut token = [0u8; 128];
        token[..64].copy_from_slice(&signature.to_bytes());
        token[64..].copy_from_slice(&message);

        BASE64.encode(token)
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
