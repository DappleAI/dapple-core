pub mod config;
pub mod error;
pub mod event;
pub mod handler;
pub mod service;

pub use config::Config;
pub use error::ConduitError;
pub use event::PromptEvent;

use axum::{
    routing::{get, post},
    Router,
};
use service::AppState;
use tower_http::cors::{Any, CorsLayer};

/// Build the conduit Axum router with all routes wired to the given state.
pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/health_check", get(health_check))
        .route("/prompt", post(handler::handle_prompt))
        .route("/summarize", post(handler::handle_summarize))
        .route("/madlib", post(handler::handle_madlib))
        .route("/composite", post(handler::handle_image_composite))
        .route("/compose", post(handler::handle_image_compose))
        .route("/render-document", post(handler::handle_render_document))
        .route("/embed", post(handler::handle_embed))
        .route("/search", post(handler::handle_search))
        .layer(cors)
        .with_state(state)
}

async fn health_check() -> &'static str {
    "OK"
}
