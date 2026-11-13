use conduit_onnx::{
    build_router,
    service::{
        AppState, EmbeddingConfig, EmbeddingService, MlmConfig, MlmService, SummarizationConfig,
        SummarizationService, TtsConfig, TtsService,
    },
    Config,
};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    env_logger::init();

    log::info!("Starting conduit-onnx inference service");

    let config = Config::from_env();
    log::info!("Configuration loaded: {:?}", config);

    let mut state = AppState::new();

    // Initialize embedding service if configured
    if let Some(embedding_config) = EmbeddingConfig::from_env() {
        log::info!(
            "Embedding service configured with model: {}",
            embedding_config.model_id
        );
        match EmbeddingService::with_modernbert(&embedding_config).await {
            Ok(service) => {
                log::info!(
                    "Embedding service initialized with {} backend (dim={})",
                    service.backend_name(),
                    service.dimension()
                );
                state.embedding_service = Some(Arc::new(service));
            }
            Err(e) => {
                if embedding_config.required {
                    panic!("Failed to initialize required embedding service: {}", e);
                }
                log::warn!(
                    "Failed to initialize embedding service, continuing without: {}",
                    e
                );
            }
        }
    }

    // Initialize TTS service if configured
    if let Some(tts_config) = TtsConfig::from_env() {
        log::info!(
            "TTS service configured with VITS model: {}",
            tts_config.model_id
        );
        match TtsService::with_vits(&tts_config).await {
            Ok(service) => {
                log::info!(
                    "TTS service initialized with {} backend",
                    service.backend_name()
                );
                state.tts_service = Some(Arc::new(service));
            }
            Err(e) => {
                if tts_config.required {
                    panic!("Failed to initialize required TTS service: {}", e);
                }
                log::warn!(
                    "Failed to initialize TTS service, continuing without: {}",
                    e
                );
            }
        }
    }

    // Initialize summarization service if configured
    if let Some(summarization_config) = SummarizationConfig::from_env() {
        match SummarizationService::with_t5(&summarization_config).await {
            Ok(service) => {
                log::info!(
                    "Summarization service initialized with {} backend (max_input={})",
                    service.backend_name(),
                    service.max_input_length()
                );
                state.summarization_service = Some(Arc::new(service));
            }
            Err(e) => {
                if summarization_config.required {
                    panic!("Failed to initialize required summarization service: {}", e);
                }
                log::warn!(
                    "Failed to initialize summarization service, continuing without: {}",
                    e
                );
            }
        }
    }

    // Initialize MLM service if configured
    if let Some(mlm_config) = MlmConfig::from_env() {
        match MlmService::with_modernbert(&mlm_config).await {
            Ok(service) => {
                log::info!(
                    "MLM service initialized with {} backend",
                    service.backend_name()
                );
                state.mlm_service = Some(Arc::new(service));
            }
            Err(e) => {
                if mlm_config.required {
                    panic!("Failed to initialize required MLM service: {}", e);
                }
                log::warn!(
                    "Failed to initialize MLM service, continuing without: {}",
                    e
                );
            }
        }
    }

    // Clone service handles for background warmup
    let warmup_embedding = state.embedding_service.clone();
    let warmup_tts = state.tts_service.clone();
    let warmup_mlm = state.mlm_service.clone();

    // Build router with optional internal auth
    let app = build_router(state, config.internal_key.clone());

    // Start server
    let addr = config.addr();
    log::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    // Warm up models in background (server is already listening for health probes)
    tokio::spawn(async move {
        if let Some(ref svc) = warmup_embedding {
            match svc.warmup().await {
                Ok(()) => log::info!("Embedding model warmed up"),
                Err(e) => log::warn!("Embedding warmup failed: {}", e),
            }
        }
        if let Some(ref svc) = warmup_tts {
            match svc.warmup().await {
                Ok(()) => log::info!("TTS model warmed up"),
                Err(e) => log::warn!("TTS warmup failed: {}", e),
            }
        }
        if let Some(ref svc) = warmup_mlm {
            match svc.warmup().await {
                Ok(()) => log::info!("MLM model warmed up"),
                Err(e) => log::warn!("MLM warmup failed: {}", e),
            }
        }
    });

    axum::serve(listener, app).await.expect("Server failed");
}
