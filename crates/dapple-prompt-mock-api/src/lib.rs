pub mod auth;
pub mod fixture;
pub mod handler;
pub mod route;
pub mod service;
pub mod state;
pub mod store;

pub use auth::MockAuthContext;
pub use fixture::Fixture;
pub use state::{MockAuthConfig, MockKeypair, MockState};
pub use store::InMemoryStore;

use aide::openapi::OpenApi;
use axum::http::{
    Method,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
};
use axum::{Extension, Router};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::trace::TraceLayer;

/// Build the mock API application
pub fn build_app(state: Arc<MockState>) -> Router {
    // Get the OpenAPI spec from the real API
    let api: OpenApi = dapple_prompt_api::build_openapi_spec();
    let router = route::build_router(state);

    // Use mirror_request() to echo back the Origin header from the request.
    // This is required when allow_credentials is true (wildcard * is not allowed).
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::mirror_request())
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_credentials(true)
        .allow_headers([AUTHORIZATION, ACCEPT, CONTENT_TYPE]);

    router
        .layer(Extension(api))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .layer(NormalizePathLayer::append_trailing_slash())
}

/// Spawn a mock API server on an ephemeral port
/// Returns the base URL and the server handle
pub async fn spawn_mock_server(fixture: Fixture) -> (String, tokio::task::JoinHandle<()>) {
    let store = InMemoryStore::from_fixture(fixture);
    let state = Arc::new(MockState::new(store));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to ephemeral port");

    let addr = listener.local_addr().unwrap().to_string();
    let base_url = format!("http://{}", addr);

    let app = build_app(state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (base_url, handle)
}
