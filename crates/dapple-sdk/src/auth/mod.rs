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
//!   (`external-api.dapple.x`). Gated by `external`. The two surfaces use
//!   separate signing keys and separate exchange-payload formats; tokens
//!   from one surface cannot be verified by the other.
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
