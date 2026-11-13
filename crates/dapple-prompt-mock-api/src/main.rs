use dapple_prompt_mock_api::{Fixture, InMemoryStore, MockState, build_app};
use std::env;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dapple_prompt_mock_api=info,tower_http=debug".into()),
        )
        .init();

    // Load fixture from environment variable or use default
    let fixture = match env::var("FIXTURE_PATH") {
        Ok(path) => {
            info!("Loading fixture from: {}", path);
            Fixture::load_from_path(&path).expect("Failed to load fixture")
        }
        Err(_) => {
            info!("Using default fixture");
            Fixture::load_default()
        }
    };

    // Optionally validate the fixture
    if env::var("VALIDATE_FIXTURE").is_ok() {
        info!("Validating fixture...");
        if let Err(e) = fixture.validate() {
            eprintln!("Fixture validation failed: {}", e);
            std::process::exit(1);
        }
        info!("Fixture validation passed");
    }

    // Create the store and state
    let store = InMemoryStore::from_fixture(fixture);
    let state = Arc::new(MockState::new(store));

    // Bind to port 8888 (or from DAPPLE_MOCK_API_PORT env var)
    let port = env::var("DAPPLE_MOCK_API_PORT").unwrap_or_else(|_| "8888".to_string());
    let addr = format!("0.0.0.0:{}", port);

    info!("Starting mock API server on {}", addr);

    let listener = TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    let app = build_app(state);

    info!("Mock API server listening on http://{}", addr);
    info!("Health check: http://{}/api/health_check", addr);
    info!("API Documentation: http://{}/docs", addr);
    info!("OpenAPI spec: http://{}/api.json", addr);

    axum::serve(listener, app).await.unwrap();
}
