use dapple_data::DbPool;
use dapple_lago::client::LagoClient;
use dapple_lago::config::LagoConfig;

mod event_builder;
mod sync;
mod worker;

#[tokio::main]
async fn main() {
    env_logger::init();

    log::info!("Starting dapple-prompt-lago-worker");

    // DbPool::connect() reads DAPPLE_PROMPT_DATABASE_URL from the environment
    let pool = DbPool::connect()
        .await
        .expect("Failed to connect to database");

    // Load Lago configuration
    let lago_config = LagoConfig::from_env()
        .expect("LAGO_API_URL and LAGO_API_KEY must be set for the lago worker");
    let lago_client = LagoClient::new(lago_config);

    log::info!("Connected to database and Lago API");

    // Run the worker loop (never returns)
    worker::run_worker(&pool, &lago_client).await;
}
