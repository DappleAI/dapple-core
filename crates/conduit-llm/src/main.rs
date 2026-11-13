use conduit_llm::{build_router, Config, MistralRsBackend, MistralRsConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    env_logger::init();

    log::info!("Starting conduit-llm inference service");

    let config = Config::from_env();
    log::info!("Configuration loaded: {:?}", config);

    let mistral_config = MistralRsConfig::from_env().unwrap_or_else(|| {
        log::warn!("MISTRALRS_MODEL_ID not set, using defaults");
        MistralRsConfig::default()
    });

    log::info!(
        "MistralRS config: model={}/{}, tok={}",
        mistral_config.model_id,
        mistral_config.gguf_file,
        mistral_config.tok_model_id
    );

    let backend = Arc::new(MistralRsBackend::new(mistral_config));

    // Clone for background warmup
    let warmup_backend = backend.clone();

    let app = build_router(backend, config.internal_key.clone());

    let addr = config.addr();
    log::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    // Warm up in background (server is already listening for health probes)
    tokio::spawn(async move {
        match warmup_backend.warmup().await {
            Ok(()) => log::info!("LLM model warmed up"),
            Err(e) => log::error!("LLM warmup failed: {}", e),
        }
    });

    axum::serve(listener, app).await.expect("Server failed");
}
