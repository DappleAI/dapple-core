pub mod auth;
pub mod backend;
pub mod config;
pub mod error;
pub mod handler;

pub use backend::MistralRsBackend;
pub use config::{Config, MistralRsConfig};
pub use error::LlmServiceError;

use auth::InternalKey;
use axum::{
    middleware,
    routing::{get, post},
    Extension, Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

/// Build the conduit-llm router.
///
/// If `internal_key` is provided, all endpoints except health check require
/// a matching `X-Internal-Key` header.
pub fn build_router(backend: Arc<MistralRsBackend>, internal_key: Option<String>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut app = Router::new()
        .route("/api/health_check", get(health_check))
        .route("/generate", post(handler::handle_generate));

    if let Some(key) = internal_key {
        app = app
            .layer(middleware::from_fn(auth::validate_internal_key))
            .layer(Extension(InternalKey(key)));
    }

    app.layer(cors).with_state(backend)
}

async fn health_check() -> &'static str {
    "OK"
}
