pub mod embedding;
pub mod mlm;
pub mod onnx;
pub mod summarization;
pub mod tts;

pub use embedding::{EmbeddingBackend, EmbeddingConfig, EmbeddingService};
pub use mlm::{MlmConfig, MlmService};
pub use onnx::{ExecutionProviderType, OnnxBackend, OnnxError};
pub use summarization::{SummarizationConfig, SummarizationService};
pub use tts::{TtsConfig, TtsService};

use std::sync::Arc;

/// Shared application state for the ONNX inference service.
#[derive(Clone)]
pub struct AppState {
    pub embedding_service: Option<Arc<EmbeddingService>>,
    pub tts_service: Option<Arc<TtsService>>,
    pub summarization_service: Option<Arc<SummarizationService>>,
    pub mlm_service: Option<Arc<MlmService>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            embedding_service: None,
            tts_service: None,
            summarization_service: None,
            mlm_service: None,
        }
    }
}
