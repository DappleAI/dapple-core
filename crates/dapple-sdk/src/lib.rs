//! Rust SDK for the Dapple Prompt API.
//!
//! This crate provides a typed HTTP client for the Dapple Prompt API.
//! Public DTO types are imported from the `dapple-types` crate (the shared
//! source of truth between server and SDK), and resource clients provide
//! ergonomic RPC-style methods.
//!
//! # Quick Start
//!
//! ```no_run
//! use dapple_sdk::DappleClient;
//!
//! # async fn example() -> Result<(), dapple_sdk::SdkError> {
//! let client = DappleClient::with_api_token("https://api.example.com", "dpl_my_token");
//!
//! // List all playbooks
//! let playbooks = client.playbooks().list(None).await?;
//!
//! // Get a specific journey
//! let journey = client.journeys().get(playbooks[0].id).await?;
//! # Ok(())
//! # }
//! ```

mod auth;
mod client;
mod envelope;
pub mod query;
pub mod resource;
pub mod types;

#[cfg(feature = "hash")]
pub use dapple_hash;

// Re-export core types at crate root
pub use types::*;

/// Re-export of the shared upsert outcome — same enum returned by both
/// `GuidesClient::upsert_from_dir` (internal) and
/// `ExternalGuidesClient::upsert_from_dir` (external).
#[cfg(feature = "tarball")]
pub use resource::upsert::UpsertOutcome;

/// Error type for SDK operations.
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    /// HTTP transport error (connection, timeout, etc.).
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// API returned an error response.
    #[error("API error (HTTP {status}): {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Error message from the API response body.
        message: String,
    },

    /// Failed to deserialize the API response body.
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Failed to serialize request body or query parameters.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Client configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// I/O error (file operations, tarball creation, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parsing error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Shared HTTP + auth state used by both `DappleClient` and (under the
/// `external` feature) `ExternalDappleClient`. Holds the reqwest client,
/// base URL, and auth provider. All HTTP helper methods (`get_one`,
/// `get_many`, `post_*`, etc.) live in `impl ClientCore` in `client.rs`,
/// so resource clients on either surface call the same machinery.
pub(crate) struct ClientCore {
    pub(crate) http: reqwest::Client,
    pub(crate) base_url: String,
    pub(crate) auth: Box<dyn auth::AuthProvider>,
}

/// The main Dapple API client — for **internal** consumers (dapple-team
/// services like dapple-ui, conduit, flow).
///
/// External (customer) consumers use [`ExternalDappleClient`] under the
/// `external` feature flag instead. The two are deliberately separate
/// types: each has its own resource surface, its own constructors, and
/// its own default URL. Mixing them at the type level is impossible.
pub struct DappleClient {
    core: ClientCore,
}

/// Environment variable consulted by [`ExternalDappleClient::external`]
/// for the external-surface base URL. There is no compiled-in fallback —
/// callers either set this in the process env or use
/// [`ExternalDappleClient::external_at`] for a fully explicit URL.
///
/// Intended as a near-term knob — once production defaults stabilize this
/// override should be promoted to explicit configuration rather than a
/// process-wide env var.
#[cfg(feature = "external")]
pub const EXTERNAL_API_HOST_ENV: &str = "DAPPLE_EXTERNAL_API_HOST";

/// Customer-facing Dapple API client — for **external** consumers
/// (customer SDKs like acme-sdk).
///
/// Cryptographically isolated from [`DappleClient`]: tokens minted by the
/// external surface cannot be verified by the internal surface and vice
/// versa. The two types share no resource methods at compile time, so
/// "I accidentally called an internal-only endpoint from a customer SDK"
/// is structurally impossible.
///
/// The default constructor [`Self::external`] reads the base URL from
/// the [`EXTERNAL_API_HOST_ENV`] env var (`DAPPLE_EXTERNAL_API_HOST`) and
/// panics if it is unset or empty. There is no compiled-in fallback — for
/// staging, test, or any explicit-URL case, use [`Self::external_at`].
#[cfg(feature = "external")]
pub struct ExternalDappleClient {
    core: ClientCore,
}

impl DappleClient {
    /// Create a client with API token authentication (`dpl_` prefix).
    pub fn with_api_token(base_url: &str, token: &str) -> Self {
        Self {
            core: ClientCore {
                http: reqwest::Client::new(),
                base_url: base_url.trim_end_matches('/').to_string(),
                auth: Box::new(auth::api_token::ApiTokenAuth::new(token)),
            },
        }
    }

    /// Create a client with a raw Bearer token (pre-obtained JWT).
    pub fn with_bearer_token(base_url: &str, token: &str) -> Self {
        Self {
            core: ClientCore {
                http: reqwest::Client::new(),
                base_url: base_url.trim_end_matches('/').to_string(),
                auth: Box::new(auth::jwt::BearerAuth::new(token)),
            },
        }
    }

    /// Create a client with Ed25519 service token authentication against
    /// the **internal** surface (`api.dapple.x`).
    ///
    /// The signing key bytes are the 32-byte Ed25519 private key.
    /// The client will sign tokens, exchange them for JWTs, and cache
    /// the JWTs for reuse. On 401, the cached JWT is invalidated.
    #[cfg(feature = "service-auth")]
    pub fn with_service_token(
        base_url: &str,
        service_id: &str,
        signing_key_bytes: &[u8; 32],
    ) -> Self {
        Self {
            core: ClientCore {
                http: reqwest::Client::new(),
                base_url: base_url.trim_end_matches('/').to_string(),
                auth: Box::new(auth::service_token::ServiceTokenAuth::new(
                    service_id,
                    signing_key_bytes,
                    base_url,
                )),
            },
        }
    }

    // ========================================================================
    // Resource client accessors
    // ========================================================================

    /// Access the Playbooks API.
    pub fn playbooks(&self) -> resource::playbooks::PlaybooksClient<'_> {
        resource::playbooks::PlaybooksClient::new(&self.core)
    }

    /// Access the Journeys API.
    pub fn journeys(&self) -> resource::journeys::JourneysClient<'_> {
        resource::journeys::JourneysClient::new(&self.core)
    }

    /// Access the Guides API (including ingest, render, cards, favorites).
    pub fn guides(&self) -> resource::guides::GuidesClient<'_> {
        resource::guides::GuidesClient::new(&self.core)
    }

    /// Access the Dispatches API.
    pub fn dispatches(&self) -> resource::dispatches::DispatchesClient<'_> {
        resource::dispatches::DispatchesClient::new(&self.core)
    }

    /// Access the Briefs API.
    pub fn briefs(&self) -> resource::briefs::BriefsClient<'_> {
        resource::briefs::BriefsClient::new(&self.core)
    }

    /// Access the Brief Groups API.
    pub fn brief_groups(&self) -> resource::brief_groups::BriefGroupsClient<'_> {
        resource::brief_groups::BriefGroupsClient::new(&self.core)
    }

    /// Access the Brief Packs API.
    pub fn brief_packs(&self) -> resource::brief_packs::BriefPacksClient<'_> {
        resource::brief_packs::BriefPacksClient::new(&self.core)
    }

    /// Access the Brief Units API.
    pub fn brief_units(&self) -> resource::brief_units::BriefUnitsClient<'_> {
        resource::brief_units::BriefUnitsClient::new(&self.core)
    }

    /// Access the Steps API.
    pub fn steps(&self) -> resource::steps::StepsClient<'_> {
        resource::steps::StepsClient::new(&self.core)
    }

    /// Access the Stages API.
    pub fn stages(&self) -> resource::stages::StagesClient<'_> {
        resource::stages::StagesClient::new(&self.core)
    }

    /// Access the Tasks API.
    pub fn tasks(&self) -> resource::tasks::TasksClient<'_> {
        resource::tasks::TasksClient::new(&self.core)
    }

    /// Access the Categories API.
    pub fn categories(&self) -> resource::categories::CategoriesClient<'_> {
        resource::categories::CategoriesClient::new(&self.core)
    }

    /// Access the Hats API.
    pub fn hats(&self) -> resource::hats::HatsClient<'_> {
        resource::hats::HatsClient::new(&self.core)
    }

    /// Access the Assets API.
    pub fn assets(&self) -> resource::assets::AssetsClient<'_> {
        resource::assets::AssetsClient::new(&self.core)
    }

    /// Access the Drafts API.
    pub fn drafts(&self) -> resource::drafts::DraftsClient<'_> {
        resource::drafts::DraftsClient::new(&self.core)
    }

    /// Access the Fluent Messages API.
    pub fn fluent_messages(&self) -> resource::fluent_messages::FluentMessagesClient<'_> {
        resource::fluent_messages::FluentMessagesClient::new(&self.core)
    }

    /// Access the Prompts API.
    pub fn prompts(&self) -> resource::prompts::PromptsClient<'_> {
        resource::prompts::PromptsClient::new(&self.core)
    }

    /// Access the Directives API.
    pub fn directives(&self) -> resource::directives::DirectivesClient<'_> {
        resource::directives::DirectivesClient::new(&self.core)
    }

    /// Access the Value Slots API.
    pub fn value_slots(&self) -> resource::value_slots::ValueSlotsClient<'_> {
        resource::value_slots::ValueSlotsClient::new(&self.core)
    }

    /// Access the Inference Profiles API.
    pub fn inference_profiles(&self) -> resource::inference_profiles::InferenceProfilesClient<'_> {
        resource::inference_profiles::InferenceProfilesClient::new(&self.core)
    }

    /// Access the Deprecations API.
    pub fn deprecations(&self) -> resource::deprecations::DeprecationsClient<'_> {
        resource::deprecations::DeprecationsClient::new(&self.core)
    }

    /// Access the Dispatch Intent Bindings API.
    pub fn dispatch_intent_bindings(
        &self,
    ) -> resource::dispatch_intent_bindings::DispatchIntentBindingsClient<'_> {
        resource::dispatch_intent_bindings::DispatchIntentBindingsClient::new(&self.core)
    }

    /// Access the Dispatch Phases API.
    pub fn dispatch_phases(&self) -> resource::dispatch_phases::DispatchPhasesClient<'_> {
        resource::dispatch_phases::DispatchPhasesClient::new(&self.core)
    }

    /// Access the Dispatch Task Attempts API.
    pub fn dispatch_task_attempts(
        &self,
    ) -> resource::dispatch_task_attempts::DispatchTaskAttemptsClient<'_> {
        resource::dispatch_task_attempts::DispatchTaskAttemptsClient::new(&self.core)
    }

    /// Access the User Details API.
    pub fn user_details(&self) -> resource::user_details::UserDetailsClient<'_> {
        resource::user_details::UserDetailsClient::new(&self.core)
    }

    /// Access the API Tokens API.
    pub fn api_tokens(&self) -> resource::api_tokens::ApiTokensClient<'_> {
        resource::api_tokens::ApiTokensClient::new(&self.core)
    }

    /// Access the Versioned Objects API (version history).
    pub fn versioned_objects(&self) -> resource::versioned_objects::VersionedObjectsClient<'_> {
        resource::versioned_objects::VersionedObjectsClient::new(&self.core)
    }
}

#[cfg(feature = "external")]
impl ExternalDappleClient {
    /// Create an external-surface client whose base URL comes from the
    /// [`EXTERNAL_API_HOST_ENV`] env var (`DAPPLE_EXTERNAL_API_HOST`).
    ///
    /// **Panics** if the env var is unset or empty — there is no
    /// compiled-in default. For staging/test or any explicit-URL case,
    /// use [`Self::external_at`] instead.
    pub fn external(service_id: &str, signing_key_bytes: &[u8; 32]) -> Self {
        let base_url = std::env::var(EXTERNAL_API_HOST_ENV)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                panic!(
                    "{} is not set; either set it in the environment or call \
                     ExternalDappleClient::external_at(...) with an explicit URL",
                    EXTERNAL_API_HOST_ENV
                )
            });
        Self::external_at(&base_url, service_id, signing_key_bytes)
    }

    /// Create an external-surface client pointing at a custom base URL.
    /// Same auth shape as [`Self::external`] but lets the caller specify
    /// the host (for staging, local dev, or test).
    pub fn external_at(base_url: &str, service_id: &str, signing_key_bytes: &[u8; 32]) -> Self {
        let trimmed = base_url.trim_end_matches('/').to_string();
        Self {
            core: ClientCore {
                http: reqwest::Client::new(),
                base_url: trimmed.clone(),
                auth: Box::new(auth::external_service_token::ExternalServiceTokenAuth::new(
                    service_id,
                    signing_key_bytes,
                    &trimmed,
                )),
            },
        }
    }

    // ========================================================================
    // External-surface resource accessors
    // ========================================================================
    //
    // Only the resource clients applicable to the external surface live
    // here. Internal-only endpoints (canonical sync, fluent-message admin,
    // playbook authoring, etc.) are absent by construction. Adding more
    // is straightforward once a customer SDK actually needs them.
    //
    // Customer-facing accessors:
    //   - `guides()`       — list, ingest
    //   - `journeys()`     — create, get, list, update, hide, briefs
    //   - `playbooks()`    — list, get (read-only — authoring is via guide ingest)
    //   - `dispatches()`   — create, get, briefs, task_attempts
    //   - `dispatch_intent_bindings()` — create

    /// Access the customer's guides — list (`GET /api/guides`) and ingest
    /// (`POST /api/guides/ingest`).
    pub fn guides(&self) -> resource::external_guides::ExternalGuidesClient<'_> {
        resource::external_guides::ExternalGuidesClient::new(&self.core)
    }

    /// Access the customer's journeys — create, get, list, update
    /// (rename via `rev.principal_journey_preferences`), hide (soft-hide),
    /// and per-journey briefs. All operations are namespace-gated by the
    /// caller's `company_id`.
    pub fn journeys(&self) -> resource::external_journeys::ExternalJourneysClient<'_> {
        resource::external_journeys::ExternalJourneysClient::new(&self.core)
    }

    /// Access the customer's playbooks — read-only (list + get). Authoring
    /// happens via the guide-ingest pipeline (`POST /api/guides/ingest`),
    /// not directly on this client. Namespace-gated by the caller's
    /// `company_id`.
    pub fn playbooks(&self) -> resource::external_playbooks::ExternalPlaybooksClient<'_> {
        resource::external_playbooks::ExternalPlaybooksClient::new(&self.core)
    }

    /// Access the customer's dispatches — operator-runner methods (create,
    /// get, briefs, task_attempts). List-by-* variants and write-side
    /// task-attempt methods are absent by construction (the workflow
    /// engine creates attempts and events server-side).
    pub fn dispatches(&self) -> resource::external_dispatches::ExternalDispatchesClient<'_> {
        resource::external_dispatches::ExternalDispatchesClient::new(&self.core)
    }

    /// Access dispatch intent bindings — single-method write client
    /// (`POST /api/dispatch-intent-bindings`). The server auto-spawns
    /// execution when all slots on a dispatch are bound.
    pub fn dispatch_intent_bindings(
        &self,
    ) -> resource::external_dispatch_intent_bindings::ExternalDispatchIntentBindingsClient<'_> {
        resource::external_dispatch_intent_bindings::ExternalDispatchIntentBindingsClient::new(
            &self.core,
        )
    }

    /// External-surface value-slots client. Read-only: `get(id)`,
    /// `list_by_ids(ids)`, `list(options)`. Consumers compose this with
    /// `journeys().brief_form(id)` to enrich a `BriefUnitView::InputView`'s
    /// `value_slot_id` with slot name/title/value_kind metadata when
    /// renderers want richer labels or type-aware client-side validation.
    pub fn value_slots(&self) -> resource::external_value_slots::ExternalValueSlotsClient<'_> {
        resource::external_value_slots::ExternalValueSlotsClient::new(&self.core)
    }
}
