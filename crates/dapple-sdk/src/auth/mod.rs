//! Authentication providers for the Dapple SDK.
//!
//! Four auth modes are supported, gated by feature flags. Internal is the
//! default flavor; `external` is the carve-out for customer SDKs (it pulls
//! in a smaller surface than the internal SDK and uses different auth):
//!
//! - **API token** (`dpl_` prefix): static bearer token. Always available.
//! - **Internal service token** (`dps_` prefix, Ed25519): signs a token,
//!   exchanges for a JWT against the **internal** surface
//!   (`api.dapple.x`). Gated by `service-auth`.
//! - **External service token** (Ed25519): same primitive, but exchanges
//!   for a JWT against the **external** surface
//!   (`external-api.dapple.x`). Gated by `external`. Both surfaces take the
//!   same 128-byte structured token at `POST /api/get-service-token`
//!   ([`sign_structured_token`]), but they register separate signing keys,
//!   so a token from one surface cannot be verified by the other.
//! - **Raw Bearer JWT**: pre-obtained JWT passed directly. Always available.
//!
//! `service-auth` and `external` are independent siblings. Internal SDK
//! consumers (dapple-ui, conduit, flow) flip `service-auth`; customer
//! SDKs flip `external`; lifecycle tests in this repo are the only place
//! both flip on at once.

pub mod api_token;
#[cfg(feature = "external")]
pub mod external_service_token;
pub mod jwt;
#[cfg(feature = "service-auth")]
pub mod service_token;

use crate::SdkError;

/// Build the 128-byte structured service token and base64-encode it, in the
/// format the API's `verify_service_token` expects on both surfaces:
///
/// `signature (64) || message (64)`, where the message is
/// `version (1) | valid_from (8, BE) | valid_until (8, BE) | sid_len (1) | sid | zero padding`.
#[cfg(any(feature = "service-auth", feature = "external"))]
pub(crate) fn sign_structured_token(
    signing_key: &ed25519_dalek::SigningKey,
    service_id: &str,
    validity_secs: u64,
) -> String {
    use base64::Engine as _;
    use ed25519_dalek::Signer;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs();

    let mut message = [0u8; 64];
    message[0] = 0x01; // version
    message[1..9].copy_from_slice(&now_secs.to_be_bytes());
    message[9..17].copy_from_slice(&(now_secs + validity_secs).to_be_bytes());
    let sid_bytes = service_id.as_bytes();
    // The server caps service_id at 32 bytes.
    assert!(sid_bytes.len() <= 32, "service_id longer than 32 bytes");
    message[17] = sid_bytes.len() as u8;
    message[18..18 + sid_bytes.len()].copy_from_slice(sid_bytes);

    let signature = signing_key.sign(&message);
    let mut token = [0u8; 128];
    token[..64].copy_from_slice(&signature.to_bytes());
    token[64..].copy_from_slice(&message);
    base64::engine::general_purpose::STANDARD.encode(token)
}

/// Trait for providing authorization headers to HTTP requests.
///
/// Implementations handle different authentication strategies.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub(crate) trait AuthProvider: Send + Sync {
    /// Get the Authorization header value for a request.
    ///
    /// Returns a string like `"Bearer dpl_abc123..."` or `"Bearer <jwt>"`.
    async fn authorization_header(&self) -> Result<String, SdkError>;

    /// Notify that the server returned 401 (token may need refresh).
    ///
    /// Service token implementations use this to invalidate cached JWTs.
    async fn on_unauthorized(&self);
}
