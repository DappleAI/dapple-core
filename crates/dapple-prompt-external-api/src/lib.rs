//! External API surface for `dapple-prompt`.
//!
//! Mounted at `external-api.dapple.x` (Pattern B deployment, port 8081).
//! Customer companies (third parties such as Acme) authenticate to
//! this surface using Ed25519 service tokens registered in
//! `rev.external_service_tokens` and bound to a row in `rev.companies`.
//!
//! The internal `dapple-prompt-api` and this crate share **no token
//! table, no signing key, no extractor** — JWTs minted here cannot be
//! verified by the internal surface and vice versa (cryptographic
//! isolation; see `docs/plans/external-api/00-architecture.md` §3
//! Decision 4).
//!
//! V1 surface (under construction):
//!
//! | Endpoint | Issue |
//! |---|---|
//! | `POST /api/get-service-token` | Issue 2 (token exchange — this crate) |
//! | `GET /api/guides` (scope-filtered) | Issue 4 |
//! | journey/briefs/dispatch endpoints | Issue 5 |

pub mod extractor;
pub mod handler;
pub mod route;
pub mod schema;

pub use extractor::{ExternalAuthRejection, ExternalCaller};

use dapple_data::DbPool;
use std::sync::Arc;

/// Application state shared across external-API handlers.
pub struct ExternalAppState {
    pub db: DbPool,
}

impl ExternalAppState {
    pub fn new(db: DbPool) -> Self {
        Self { db }
    }
}

/// Environment-variable name for the external surface's JWT signing key.
///
/// **Distinct from `DAPPLE_PROMPT_API_SIGNING_KEY`** — the external surface
/// loads a different secret than the internal surface, providing
/// cryptographic separation: a JWT signed for one surface fails signature
/// verification on the other before any claim is even parsed.
pub const EXTERNAL_JWT_SIGNING_KEY_ENV: &str = "DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY";

/// `iss` claim value embedded in JWTs issued by this surface.
pub const EXTERNAL_JWT_ISS: &str = "external-api.dapple.x";

/// `token_use` claim value for external service-token JWTs.
pub const EXTERNAL_TOKEN_USE: &str = "external_service";

/// Build the external-surface router. Pattern B: a separate
/// `dapple-prompt-server` binary mounts this when launched with
/// `--surface=external`.
pub fn build_router(state: Arc<ExternalAppState>) -> aide::axum::ApiRouter {
    use aide::axum::ApiRouter;
    ApiRouter::new()
        .nest("/api/get-service-token", route::service_token::router())
        .nest("/api/guides", route::guides::router())
        .nest("/api/journeys", route::journeys::router())
        .nest("/api/playbooks", route::playbooks::router())
        .nest("/api/stages", route::stages::router())
        .nest("/api/steps", route::steps::router())
        .nest("/api/tasks", route::tasks::router())
        .nest("/api/categories", route::categories::router())
        .nest("/api/prompts", route::prompts::router())
        .nest("/api/directives", route::directives::router())
        .nest("/api/value-slots", route::value_slots::router())
        .nest("/api/fluent-messages", route::fluent_messages::router())
        .nest("/api/dispatches", route::dispatches::router())
        .nest(
            "/api/dispatch-intent-bindings",
            route::dispatch_intent_bindings::router(),
        )
        .with_state(state)
}
