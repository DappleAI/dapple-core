//! conduit-oximedia — an isolated media sub-service.
//!
//! Owns the OxiMedia dependency family (audio analysis + MIR + OTIO timeline
//! export) so the main conduit never links it. Callers (dapple-prompt-flow) hit
//! this service directly at its own URL; it is only reachable when that URL env
//! is configured on the caller.
//!
//! The OxiMedia-backed endpoints are gated behind the `oximedia` cargo feature
//! (off by default). Without it, this builds as a bare health-check binary.

pub mod auth;
pub mod config;
pub mod error;
pub mod handler;
pub mod service;

#[cfg(feature = "oximedia")]
pub mod persist;

pub use config::Config;
pub use error::OximediaError;

use auth::InternalKey;
use axum::{middleware, routing::get, Extension, Router};
use service::AppState;
use tower_http::cors::{Any, CorsLayer};

/// Build the conduit-oximedia Axum router.
///
/// The `/analyze-audio` + `/compose-av` routes are only mounted when built with
/// the `oximedia` feature. If `internal_key` is provided, all endpoints except
/// health check require a matching `X-Internal-Key` header.
pub fn build_router(state: AppState, internal_key: Option<String>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    #[allow(unused_mut)]
    let mut app = Router::new().route("/api/health_check", get(health_check));

    #[cfg(feature = "oximedia")]
    {
        use axum::routing::post;
        app = app
            .route("/analyze-audio", post(handler::handle_analyze_audio))
            .route("/compose-av", post(handler::handle_compose_av));
    }

    if let Some(key) = internal_key {
        app = app
            .layer(middleware::from_fn(auth::validate_internal_key))
            .layer(Extension(InternalKey(key)));
    }

    app.layer(cors).with_state(state)
}

async fn health_check() -> &'static str {
    "OK"
}
