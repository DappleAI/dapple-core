//! conduit-media-gateway — an isolated, opt-in Twitch-VOD → learning-highlight
//! service.
//!
//! Owns the heavy/native media stack (whisper.cpp via `whisper-rs`, the OxiMedia
//! OTIO/EDL export path) plus the `ffmpeg`/`yt-dlp` runtime binaries, so neither
//! the main conduit nor conduit-oximedia links any of it. Callers
//! (dapple-prompt-flow) hit this service directly at its own URL
//! (`DAPPLE_MEDIA_GATEWAY_URL`); it is only reachable when that env is configured.
//!
//! The pipeline endpoints are gated behind the `media-gateway` cargo feature (off
//! by default). Without it, this builds as a bare health-check binary.

pub mod auth;
pub mod config;
pub mod error;
pub mod handler;
pub mod persist;
pub mod service;

pub use config::Config;
pub use error::MediaGatewayError;

use auth::InternalKey;
use axum::{middleware, routing::get, Extension, Router};
use service::AppState;
use tower_http::cors::{Any, CorsLayer};

/// Build the conduit-media-gateway Axum router.
///
/// The pipeline routes (`/twitch/pull`, `/vod/*`, `/jobs/{id}`) are only mounted
/// when built with the `media-gateway` feature. If `internal_key` is provided, all
/// endpoints except the health check require a matching `X-Internal-Key` header.
pub fn build_router(state: AppState, internal_key: Option<String>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    #[allow(unused_mut)]
    let mut app = Router::new().route("/api/health_check", get(health_check));

    #[cfg(feature = "media-gateway")]
    {
        use axum::routing::post;
        app = app
            .route("/twitch/pull", post(handler::handle_twitch_pull))
            .route("/vod/transcribe", post(handler::handle_vod_transcribe))
            .route("/vod/detect", post(handler::handle_vod_detect))
            .route("/vod/context", post(handler::handle_vod_context))
            .route("/vod/cut", post(handler::handle_vod_cut))
            .route("/vod/assemble", post(handler::handle_vod_assemble))
            .route("/render-timeline", post(handler::handle_render_timeline))
            .route("/snapshot", post(handler::handle_snapshot))
            .route("/jobs/{id}", get(handler::handle_get_job));
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
