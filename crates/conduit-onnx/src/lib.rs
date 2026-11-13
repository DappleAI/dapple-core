pub mod auth;
pub mod config;
pub mod error;
pub mod handler;
pub mod service;

pub use config::Config;
pub use error::OnnxServiceError;

use auth::InternalKey;
use axum::{
    middleware,
    routing::{get, post},
    Extension, Router,
};
use service::AppState;
use tower_http::cors::{Any, CorsLayer};

/// Build the conduit-onnx Axum router with all routes wired to the given state.
///
/// If `internal_key` is provided, all endpoints except health check require
/// a matching `X-Internal-Key` header.
pub fn build_router(state: AppState, internal_key: Option<String>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut app = Router::new()
        .route("/api/health_check", get(health_check))
        .route("/embed", post(handler::handle_embed))
        .route("/synthesize", post(handler::handle_synthesize))
        .route("/summarize", post(handler::handle_summarize))
        .route("/madlib", post(handler::handle_madlib));

    // Add internal auth middleware if key is configured
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
