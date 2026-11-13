pub mod context;
pub mod embedding;
pub mod image;
pub mod llm;
pub mod llm_client;
pub mod mlm;
pub mod model;
pub mod onnx;
pub mod onnx_client;
pub mod overlay;
pub mod rag;
pub mod rath_util;
pub mod summarization;
pub mod task_context_client;
pub mod template;
pub mod tts;
pub mod video;

pub use context::ContextService;
pub use embedding::{EmbeddingBackend, EmbeddingConfig, EmbeddingService};
pub use image::{ImageBackend, ImageConfig, ImageService};
pub use llm::{
    AnthropicBackend, LlmBackend, LlmConfig, LlmService, MistralRsBackend, MistralRsConfig,
    StubBackend, TokenStream,
};
pub use llm_client::LlmInferenceClient;
pub use mlm::{MaskPrediction, MlmBackend, MlmConfig, MlmResult, MlmService, TokenPrediction};
pub use model::{ModelConfig, ModelService};
pub use onnx::{ExecutionProviderType, OnnxBackend, OnnxError};
pub use onnx_client::OnnxInferenceClient;
pub use overlay::{OverlayConfig, OverlayService, RankedValueSlot};
pub use rag::{RagContext, RagResult, RagService};
pub use summarization::{
    DialogueTurn, SummarizationBackend, SummarizationConfig, SummarizationInput,
    SummarizationService,
};
pub use task_context_client::{
    TaskContext, TaskContextAsset, TaskContextBinding, TaskContextBrief, TaskContextBriefUnit,
    TaskContextClient, TaskContextDispatchPosition, TaskContextPriorTask,
};
pub use template::{PromptMessage, RenderedPrompt, TemplateService};
pub use tts::{TtsBackend, TtsGenRequest, TtsGenResult, TtsService};
pub use video::{VideoBackend, VideoGenRequest, VideoGenResult, VideoService};

use dapple_data::DbPool;
use std::sync::Arc;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<DbPool>,
    pub context_service: Arc<ContextService>,
    pub overlay_service: Arc<OverlayService>,
    pub template_service: Arc<TemplateService>,
    pub llm_service: Arc<LlmService>,
    pub model_service: Arc<ModelService>,
    pub embedding_service: Option<Arc<EmbeddingService>>,
    pub rag_service: Option<Arc<RagService>>,
    pub summarization_service: Option<Arc<SummarizationService>>,
    pub mlm_service: Option<Arc<MlmService>>,
    pub tts_service: Option<Arc<TtsService>>,
    pub image_service: Option<Arc<ImageService>>,
    /// Video generation service (rath → Fal). `None` unless a provider key is set
    /// (`FAL_AUTH_KEY`) — the opt-in gate that keeps video off by default.
    pub video_service: Option<Arc<VideoService>>,
    /// HTTP client to conduit-onnx sub-service (embed, synthesize, summarize, madlib)
    pub onnx_client: Option<Arc<OnnxInferenceClient>>,
    /// HTTP client to conduit-llm sub-service (streaming LLM generation)
    pub llm_client: Option<Arc<LlmInferenceClient>>,
    /// HTTP client to dapple-prompt-api's `task_context` endpoint. Used to
    /// enrich LLM prompts with runtime dispatch state (bindings, assets,
    /// prior task outcomes, brief context). Configured from
    /// `DAPPLE_PROMPT_API_URL` at startup; when unset, `fetch()` returns
    /// `Ok(None)` and injection becomes a no-op (MVP Phase F).
    pub task_context_client: Arc<TaskContextClient>,
}

/// Read `DAPPLE_PROMPT_API_URL` from the environment. Shared by all AppState
/// constructors so TaskContextClient initialization stays consistent.
fn task_context_client_from_env() -> Arc<TaskContextClient> {
    let base_url = std::env::var("DAPPLE_PROMPT_API_URL")
        .ok()
        .filter(|s| !s.is_empty());
    let signing_key = std::env::var("DAPPLE_PROMPT_API_SIGNING_KEY")
        .ok()
        .filter(|s| !s.is_empty());
    if let Some(ref u) = base_url {
        let auth = if signing_key.is_some() {
            "with service JWT auth"
        } else {
            "without auth (signing key not set)"
        };
        log::info!("TaskContextClient enabled: api_base={} {}", u, auth);
    } else {
        log::debug!(
            "TaskContextClient disabled: DAPPLE_PROMPT_API_URL is not set; task_context injection will be a no-op"
        );
    }
    Arc::new(TaskContextClient::new(base_url, signing_key))
}

impl AppState {
    pub fn new(pool: DbPool, llm_config: &LlmConfig) -> Self {
        let pool = Arc::new(pool);
        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service: Arc::new(OverlayService::new(pool.clone())),
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new(llm_config)),
            model_service: Arc::new(ModelService::new(pool.clone(), llm_config.clone())),
            embedding_service: None,
            rag_service: None,
            summarization_service: None,
            mlm_service: None,
            tts_service: None,
            image_service: None,
            video_service: None,
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Create an AppState with embedding service
    pub async fn with_embedding(
        pool: DbPool,
        llm_config: &LlmConfig,
        embedding_config: Option<&EmbeddingConfig>,
    ) -> Self {
        let pool = Arc::new(pool);

        // Try to initialize embedding service
        let embedding_service = match embedding_config {
            Some(config) => match EmbeddingService::with_modernbert(config).await {
                Ok(service) => {
                    log::info!(
                        "Embedding service initialized with {} backend (dim={})",
                        service.backend_name(),
                        service.dimension()
                    );
                    Some(Arc::new(service))
                }
                Err(e) => {
                    if config.required {
                        panic!("Failed to initialize required embedding service: {}", e);
                    }
                    log::warn!(
                        "Failed to initialize embedding service, continuing without: {}",
                        e
                    );
                    None
                }
            },
            None => None,
        };

        // Create overlay service with embeddings if available
        let overlay_config = OverlayConfig::from_env();
        let overlay_service = match &embedding_service {
            Some(emb) if overlay_config.semantic_ranking => {
                log::info!("Overlay service configured with semantic ranking");
                Arc::new(OverlayService::with_embeddings(
                    pool.clone(),
                    overlay_config,
                    emb.clone(),
                ))
            }
            _ => Arc::new(OverlayService::with_config(pool.clone(), overlay_config)),
        };

        // Create RAG service if embedding is available
        let rag_service = embedding_service
            .as_ref()
            .map(|emb| Arc::new(RagService::new(pool.clone(), emb.clone())));

        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service,
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new(llm_config)),
            model_service: Arc::new(ModelService::new(pool.clone(), llm_config.clone())),
            embedding_service,
            rag_service,
            summarization_service: None,
            mlm_service: None,
            tts_service: None,
            image_service: None,
            video_service: None,
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Create an AppState with embedding, summarization, and MLM services
    pub async fn with_services(
        pool: DbPool,
        llm_config: &LlmConfig,
        embedding_config: Option<&EmbeddingConfig>,
        summarization_config: Option<&SummarizationConfig>,
        mlm_config: Option<&MlmConfig>,
    ) -> Self {
        let pool = Arc::new(pool);

        // Try to initialize embedding service
        let embedding_service = match embedding_config {
            Some(config) => match EmbeddingService::with_modernbert(config).await {
                Ok(service) => {
                    log::info!(
                        "Embedding service initialized with {} backend (dim={})",
                        service.backend_name(),
                        service.dimension()
                    );
                    Some(Arc::new(service))
                }
                Err(e) => {
                    if config.required {
                        panic!("Failed to initialize required embedding service: {}", e);
                    }
                    log::warn!(
                        "Failed to initialize embedding service, continuing without: {}",
                        e
                    );
                    None
                }
            },
            None => None,
        };

        // Try to initialize summarization service
        let summarization_service = match summarization_config {
            Some(config) => match SummarizationService::with_t5(config).await {
                Ok(service) => {
                    log::info!(
                        "Summarization service initialized with {} backend (max_input={})",
                        service.backend_name(),
                        service.max_input_length()
                    );
                    Some(Arc::new(service))
                }
                Err(e) => {
                    if config.required {
                        panic!("Failed to initialize required summarization service: {}", e);
                    }
                    log::warn!(
                        "Failed to initialize summarization service, continuing without: {}",
                        e
                    );
                    None
                }
            },
            None => None,
        };

        // Try to initialize MLM service
        let mlm_service = match mlm_config {
            Some(config) => match MlmService::with_modernbert(config).await {
                Ok(service) => {
                    log::info!(
                        "MLM service initialized with {} backend",
                        service.backend_name()
                    );
                    Some(Arc::new(service))
                }
                Err(e) => {
                    if config.required {
                        panic!("Failed to initialize required MLM service: {}", e);
                    }
                    log::warn!(
                        "Failed to initialize MLM service, continuing without: {}",
                        e
                    );
                    None
                }
            },
            None => None,
        };

        // Create overlay service with embeddings if available
        let overlay_config = OverlayConfig::from_env();
        let overlay_service = match &embedding_service {
            Some(emb) if overlay_config.semantic_ranking => {
                log::info!("Overlay service configured with semantic ranking");
                Arc::new(OverlayService::with_embeddings(
                    pool.clone(),
                    overlay_config,
                    emb.clone(),
                ))
            }
            _ => Arc::new(OverlayService::with_config(pool.clone(), overlay_config)),
        };

        // Create RAG service if embedding is available
        let rag_service = embedding_service
            .as_ref()
            .map(|emb| Arc::new(RagService::new(pool.clone(), emb.clone())));

        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service,
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new(llm_config)),
            model_service: Arc::new(ModelService::new(pool.clone(), llm_config.clone())),
            embedding_service,
            rag_service,
            summarization_service,
            mlm_service,
            tts_service: None,
            image_service: None,
            video_service: None,
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Create an AppState with stub LLM service (for testing)
    pub fn new_stub(pool: DbPool) -> Self {
        let pool = Arc::new(pool);
        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service: Arc::new(OverlayService::new(pool.clone())),
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new_stub()),
            model_service: Arc::new(ModelService::new(pool.clone(), LlmConfig::default())),
            embedding_service: None,
            rag_service: None,
            summarization_service: None,
            mlm_service: None,
            tts_service: None,
            image_service: None,
            video_service: None,
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Create an AppState with stub services including embedding (for testing)
    pub fn new_stub_with_embedding(pool: DbPool, dimension: usize) -> Self {
        let pool = Arc::new(pool);
        let embedding_service = Arc::new(EmbeddingService::new_stub(dimension));
        let rag_service = Arc::new(RagService::new(pool.clone(), embedding_service.clone()));

        let overlay_config = OverlayConfig {
            semantic_ranking: true,
            semantic_top_k: 10,
            similarity_threshold: 0.3,
        };

        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service: Arc::new(OverlayService::with_embeddings(
                pool.clone(),
                overlay_config,
                embedding_service.clone(),
            )),
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new_stub()),
            model_service: Arc::new(ModelService::new(pool.clone(), LlmConfig::default())),
            embedding_service: Some(embedding_service),
            rag_service: Some(rag_service),
            summarization_service: None,
            mlm_service: None,
            tts_service: None,
            image_service: None,
            video_service: None,
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Create an AppState with stub summarization service (for testing)
    pub fn new_stub_with_summarization(pool: DbPool, max_input_length: usize) -> Self {
        let pool = Arc::new(pool);
        let summarization_service = Arc::new(SummarizationService::new_stub(max_input_length));

        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service: Arc::new(OverlayService::new(pool.clone())),
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new_stub()),
            model_service: Arc::new(ModelService::new(pool.clone(), LlmConfig::default())),
            embedding_service: None,
            rag_service: None,
            summarization_service: Some(summarization_service),
            mlm_service: None,
            tts_service: None,
            image_service: None,
            video_service: None,
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Create an AppState with stub MLM service (for testing)
    pub fn new_stub_with_mlm(pool: DbPool) -> Self {
        let pool = Arc::new(pool);
        let mlm_service = Arc::new(MlmService::new_stub());

        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service: Arc::new(OverlayService::new(pool.clone())),
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new_stub()),
            model_service: Arc::new(ModelService::new(pool.clone(), LlmConfig::default())),
            embedding_service: None,
            rag_service: None,
            summarization_service: None,
            mlm_service: Some(mlm_service),
            tts_service: None,
            image_service: None,
            video_service: None,
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Create an AppState with stub TTS service (for testing)
    pub fn new_stub_with_tts(pool: DbPool) -> Self {
        let pool = Arc::new(pool);
        let tts_service = Arc::new(TtsService::new_stub());

        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service: Arc::new(OverlayService::new(pool.clone())),
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new_stub()),
            model_service: Arc::new(ModelService::new(pool.clone(), LlmConfig::default())),
            embedding_service: None,
            rag_service: None,
            summarization_service: None,
            mlm_service: None,
            tts_service: Some(tts_service),
            image_service: None,
            video_service: None,
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Create an AppState with a stub image service (for testing)
    pub fn new_stub_with_image(pool: DbPool) -> Self {
        let pool = Arc::new(pool);
        let image_service = Arc::new(ImageService::new_stub());

        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service: Arc::new(OverlayService::new(pool.clone())),
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new_stub()),
            model_service: Arc::new(ModelService::new(pool.clone(), LlmConfig::default())),
            embedding_service: None,
            rag_service: None,
            summarization_service: None,
            mlm_service: None,
            tts_service: None,
            image_service: Some(image_service),
            video_service: None,
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Create an AppState with a stub video service (for testing)
    pub fn new_stub_with_video(pool: DbPool) -> Self {
        let pool = Arc::new(pool);
        let video_service = Arc::new(VideoService::new_stub());

        Self {
            pool: pool.clone(),
            context_service: Arc::new(ContextService::new(pool.clone())),
            overlay_service: Arc::new(OverlayService::new(pool.clone())),
            template_service: Arc::new(TemplateService::new()),
            llm_service: Arc::new(LlmService::new_stub()),
            model_service: Arc::new(ModelService::new(pool.clone(), LlmConfig::default())),
            embedding_service: None,
            rag_service: None,
            summarization_service: None,
            mlm_service: None,
            tts_service: None,
            image_service: None,
            video_service: Some(video_service),
            onnx_client: None,
            llm_client: None,
            task_context_client: task_context_client_from_env(),
        }
    }

    /// Check if embedding service is available
    pub fn has_embedding_service(&self) -> bool {
        self.embedding_service.is_some()
    }

    /// Check if RAG service is available
    pub fn has_rag_service(&self) -> bool {
        self.rag_service.is_some()
    }

    /// Check if summarization service is available
    pub fn has_summarization_service(&self) -> bool {
        self.summarization_service.is_some()
    }

    /// Check if MLM service is available
    pub fn has_mlm_service(&self) -> bool {
        self.mlm_service.is_some()
    }

    /// Check if TTS service is available
    pub fn has_tts_service(&self) -> bool {
        self.tts_service.is_some()
    }

    /// Check if image service is available
    pub fn has_image_service(&self) -> bool {
        self.image_service.is_some()
    }

    /// Check if video service is available
    pub fn has_video_service(&self) -> bool {
        self.video_service.is_some()
    }
}
