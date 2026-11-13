use conduit_oximedia::{build_router, service::AppState, Config};
use dapple_data::DbPool;

#[tokio::main]
async fn main() {
    env_logger::init();
    dapple_data::load_env();

    log::info!("Starting conduit-oximedia service");

    let config = Config::from_env();
    log::info!("Configuration loaded: {:?}", config);

    // DbPool::connect() reads DAPPLE_PROMPT_DATABASE_URL from the environment.
    let pool = DbPool::connect()
        .await
        .expect("Failed to create database pool");
    log::info!("Database pool created");

    let state = AppState::new(pool);
    let app = build_router(state, config.internal_key.clone());

    let addr = config.addr();
    log::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    axum::serve(listener, app).await.expect("Server failed");
}
