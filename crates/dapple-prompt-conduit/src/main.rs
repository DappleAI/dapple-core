use dapple_data::DbPool;
use dapple_prompt_conduit::{
    build_router,
    service::{
        AppState, EmbeddingConfig, EmbeddingService, ImageService, LlmConfig, TtsService,
        VideoService,
    },
    Config,
};
use std::sync::Arc;

/// Pre-download model weights so the server starts with a warm cache.
///
/// Usage: `dapple-prompt-conduit prefetch`
///
/// When MISTRALRS_MODEL_ID is set, uses mistral.rs warmup (downloads GGUF + loads model).
/// With `candle` feature: falls back to legacy hf_hub download for Candle backend.
/// Requires HF_TOKEN for gated models. No database or network port needed.
async fn prefetch() {
    // Prefer mistral.rs prefetch when MISTRALRS_MODEL_ID is set
    if std::env::var("MISTRALRS_MODEL_ID").is_ok() {
        use dapple_prompt_conduit::service::llm::MistralRsBackend;
        use dapple_prompt_conduit::service::llm::MistralRsConfig;
        use dapple_prompt_conduit::service::LlmBackend;

        let config = MistralRsConfig::from_env();
        log::info!(
            "Prefetching via mistral.rs GGUF: {}/{}",
            config.model_id,
            config.gguf_file
        );

        let backend = MistralRsBackend::new(config);
        match backend.warmup().await {
            Ok(()) => log::info!("mistral.rs prefetch complete"),
            Err(e) => log::error!("mistral.rs prefetch failed: {}", e),
        }
        return;
    }

    #[cfg(feature = "candle")]
    {
        use dapple_prompt_conduit::service::llm::MistralConfig;
        use hf_hub::{api::tokio::ApiBuilder, Repo, RepoType};

        let config = MistralConfig::from_env();
        log::info!("Prefetching model (legacy hf_hub): {}", config.model_id);

        let api = ApiBuilder::new()
            .with_token(config.hf_token.clone())
            .build()
            .expect("Failed to init HuggingFace API");

        let repo = match &config.revision {
            Some(rev) => api.repo(Repo::with_revision(
                config.model_id.clone(),
                RepoType::Model,
                rev.clone(),
            )),
            None => api.repo(Repo::new(config.model_id.clone(), RepoType::Model)),
        };

        for filename in &["config.json", "tokenizer.json"] {
            log::info!("  downloading {}", filename);
            match repo.get(filename).await {
                Ok(path) => log::info!("  cached at {}", path.display()),
                Err(e) => log::warn!("  {} (non-fatal): {}", filename, e),
            }
        }

        log::info!("  downloading model weights");
        match repo.get("model.safetensors").await {
            Ok(path) => log::info!("  cached at {}", path.display()),
            Err(_) => match repo.get("model.safetensors.index.json").await {
                Ok(index_path) => {
                    log::info!("  cached index at {}", index_path.display());
                    if let Ok(content) = std::fs::read_to_string(&index_path) {
                        if let Ok(index) = serde_json::from_str::<serde_json::Value>(&content) {
                            let mut shards: Vec<String> = index
                                .get("weight_map")
                                .and_then(|m| m.as_object())
                                .map(|m| {
                                    m.values()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default();
                            shards.sort();
                            shards.dedup();
                            for shard in &shards {
                                log::info!("  downloading {}", shard);
                                match repo.get(shard).await {
                                    Ok(path) => log::info!("  cached at {}", path.display()),
                                    Err(e) => {
                                        log::error!("  failed to download {}: {}", shard, e)
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => log::error!("  failed to download model weights: {}", e),
            },
        }

        log::info!("Prefetch complete");
        return;
    }

    #[cfg(not(feature = "candle"))]
    log::warn!("MISTRALRS_MODEL_ID not set and candle feature not enabled, nothing to prefetch");
}

#[tokio::main]
async fn main() {
    // Initialize logging
    env_logger::init();
    dapple_data::load_env();

    // Handle subcommands before starting the server
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "prefetch" {
        prefetch().await;
        return;
    }

    log::info!("Starting dapple-prompt-conduit service");

    // Load configuration
    let config = Config::from_env();
    log::info!("Configuration loaded: {:?}", config);

    // Load LLM configuration
    let llm_config = LlmConfig::from_env();
    if !llm_config.has_any_backend() {
        log::warn!(
            "No LLM API keys configured. Only stub backend will be available. \
             Set ANTHROPIC_API_KEY or OPENAI_API_KEY environment variables."
        );
    }

    // Create database pool
    // Note: DbPool::connect() uses DAPPLE_PROMPT_DATABASE_URL from environment
    let pool = DbPool::connect()
        .await
        .expect("Failed to create database pool");
    log::info!("Database pool created");

    // Create application state
    let mut state = AppState::new(pool, &llm_config);

    // Initialize TTS service if a provider key is present (OPENAI_API_KEY for the
    // OpenAI-via-rath backend). Off by default — this env gate is the opt-in
    // "flag" for text-to-speech. Backends are selected per-prompt by engine_body
    // `provider`, exactly like the image/video paths.
    if let Some(tts_service) = TtsService::from_env() {
        log::info!(
            "TTS service configured: backends={:?}",
            tts_service.backend_names()
        );
        state.tts_service = Some(Arc::new(tts_service));
    }

    // Initialize image service if any provider key is present (FAL_AUTH_KEY for
    // the rath/Fal backend and/or OPENAI_API_KEY for the secondary gpt-image
    // backend). Backends are selected per-prompt by the engine_body `provider`,
    // exactly like the LLM path selects by provider_key.
    if let Some(image_service) = ImageService::from_env() {
        log::info!(
            "Image service configured: backends={:?}, default_model={}",
            image_service.backend_names(),
            image_service.default_model()
        );
        state.image_service = Some(Arc::new(image_service));
    }

    // Initialize video service if a provider key is present (FAL_AUTH_KEY). Off
    // by default — this env gate is the opt-in "flag" for video generation.
    if let Some(video_service) = VideoService::from_env() {
        log::info!(
            "Video service configured: backends={:?}",
            video_service.backend_names()
        );
        state.video_service = Some(Arc::new(video_service));
    }

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

    // Initialize sub-service clients if URLs are configured
    let internal_key = std::env::var("CONDUIT_INTERNAL_KEY").ok();

    if let Ok(onnx_url) = std::env::var("CONDUIT_ONNX_URL") {
        log::info!("ONNX inference client configured: {}", onnx_url);
        state.onnx_client = Some(Arc::new(
            dapple_prompt_conduit::service::OnnxInferenceClient::new(
                &onnx_url,
                internal_key.clone(),
            ),
        ));
    }

    if let Ok(llm_url) = std::env::var("CONDUIT_LLM_URL") {
        log::info!("LLM inference client configured: {}", llm_url);
        state.llm_client = Some(Arc::new(
            dapple_prompt_conduit::service::LlmInferenceClient::new(&llm_url, internal_key.clone()),
        ));
    }

    // Initialize RAG service — prefer onnx_client, fall back to local embedding
    if let Some(ref onnx_client) = state.onnx_client {
        state.rag_service = Some(Arc::new(
            dapple_prompt_conduit::service::RagService::with_onnx_client(
                state.pool.clone(),
                onnx_client.clone(),
            ),
        ));
        log::info!("RAG service initialized (via conduit-onnx)");
    } else if let Some(ref embedding_svc) = state.embedding_service {
        state.rag_service = Some(Arc::new(dapple_prompt_conduit::service::RagService::new(
            state.pool.clone(),
            embedding_svc.clone(),
        )));
        log::info!("RAG service initialized (local embedding)");
    }

    // Clone service handles for background warmup before state is consumed by the router
    let warmup_llm = state.llm_service.clone();
    let warmup_embedding = state.embedding_service.clone();
    let warmup_mlm = state.mlm_service.clone();

    // Build router
    let app = build_router(state);

    // Start server
    let addr = config.addr();
    log::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    // Warm up all backends in the background.
    // The server is already listening so health probes pass immediately.
    // Eager loading surfaces OOM/missing-model errors at startup instead of on first request.
    tokio::spawn(async move {
        warmup_llm.warmup().await;

        if let Some(ref svc) = warmup_embedding {
            match svc.warmup().await {
                Ok(()) => log::info!("Embedding model warmed up"),
                Err(e) => log::warn!("Embedding warmup failed: {}", e),
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
