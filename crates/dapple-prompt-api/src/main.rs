use dapple_data::DbPool;
use dapple_data::integrations;
use dapple_prompt_api::build_app;

#[tokio::main]
async fn main() {
    dapple_data::load_env();

    env_logger::init();

    let pool = match DbPool::connect().await {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("Failed to connect to database: {}", err);
            std::process::exit(1);
        }
    };

    // Fail-fast: publisher pool must initialize successfully at startup.
    if let Err(e) = integrations::init_publisher_pool(pool.get_pool().clone()) {
        eprintln!("failed to initialize publisher pool: {}", e);
        std::process::exit(1);
    }

    // Log conduit client status
    if let Ok(url) = std::env::var("DAPPLE_PROMPT_CONDUIT_URL") {
        println!("Conduit service URL: {}", url);
    } else {
        println!("DAPPLE_PROMPT_CONDUIT_URL not set, TTS synthesis will be unavailable");
    }

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8001").await.unwrap();

    let app = build_app(&listener, pool).await;

    println!("🚀 Server started successfully");

    axum::serve(listener, app).await.unwrap()
}
