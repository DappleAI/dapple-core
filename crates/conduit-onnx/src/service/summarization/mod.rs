mod stub;
mod t5;

pub use stub::StubSummarizationBackend;
pub use t5::T5SummarizationBackend;

use crate::error::OnnxServiceError;
use async_trait::async_trait;
use std::sync::Arc;

/// A dialogue turn with speaker and content
#[derive(Debug, Clone)]
pub struct DialogueTurn {
    pub speaker: String,
    pub content: String,
}

/// Input for summarization
#[derive(Debug, Clone)]
pub enum SummarizationInput {
    /// Raw text to summarize with optional custom prefix
    Text {
        content: String,
        prompt_prefix: Option<String>,
    },
    /// Structured dialogue with speaker turns and optional custom prefix
    Dialogue {
        turns: Vec<DialogueTurn>,
        prompt_prefix: Option<String>,
    },
}

impl SummarizationInput {
    /// Create a text input with default prefix
    pub fn text(content: impl Into<String>) -> Self {
        SummarizationInput::Text {
            content: content.into(),
            prompt_prefix: None,
        }
    }

    /// Create a text input with custom prefix
    pub fn text_with_prefix(content: impl Into<String>, prefix: impl Into<String>) -> Self {
        SummarizationInput::Text {
            content: content.into(),
            prompt_prefix: Some(prefix.into()),
        }
    }

    /// Create a dialogue input with default prefix
    pub fn dialogue(turns: Vec<DialogueTurn>) -> Self {
        SummarizationInput::Dialogue {
            turns,
            prompt_prefix: None,
        }
    }

    /// Create a dialogue input with custom prefix
    pub fn dialogue_with_prefix(turns: Vec<DialogueTurn>, prefix: impl Into<String>) -> Self {
        SummarizationInput::Dialogue {
            turns,
            prompt_prefix: Some(prefix.into()),
        }
    }

    /// Convert input to text format for the model
    pub fn to_text(&self) -> String {
        match self {
            SummarizationInput::Text { content, .. } => content.clone(),
            SummarizationInput::Dialogue { turns, .. } => turns
                .iter()
                .map(|t| format!("{}: {}", t.speaker, t.content))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Get the custom prompt prefix, if any
    pub fn prompt_prefix(&self) -> Option<&str> {
        match self {
            SummarizationInput::Text { prompt_prefix, .. } => prompt_prefix.as_deref(),
            SummarizationInput::Dialogue { prompt_prefix, .. } => prompt_prefix.as_deref(),
        }
    }
}

/// Trait for summarization backends
#[async_trait]
pub trait SummarizationBackend: Send + Sync {
    /// Summarize a batch of inputs
    /// Returns a vector of summaries, one per input
    async fn summarize(
        &self,
        inputs: &[SummarizationInput],
    ) -> Result<Vec<String>, OnnxServiceError>;

    /// Get the backend name for logging
    fn name(&self) -> &str;

    /// Get the maximum input length in tokens
    fn max_input_length(&self) -> usize;

    /// Eagerly load heavy resources. Default is a no-op.
    async fn warmup(&self) -> Result<(), OnnxServiceError> {
        Ok(())
    }
}

/// Configuration for summarization service
#[derive(Debug, Clone)]
pub struct SummarizationConfig {
    /// HuggingFace model ID (e.g., "philschmid/flan-t5-base-samsum")
    pub model_id: String,
    /// Maximum input length in tokens
    pub max_input_length: usize,
    /// Maximum output length in tokens
    pub max_output_length: usize,
    /// Minimum output length in tokens
    pub min_output_length: usize,
    /// Number of beams for beam search (1 = greedy)
    pub num_beams: usize,
    /// Whether to fail startup if model loading fails
    pub required: bool,
    /// Optional HuggingFace token for private models
    pub hf_token: Option<String>,
}

impl Default for SummarizationConfig {
    fn default() -> Self {
        Self {
            model_id: "philschmid/flan-t5-base-samsum".to_string(),
            max_input_length: 512,
            max_output_length: 150,
            min_output_length: 20,
            num_beams: 4,
            required: false,
            hf_token: None,
        }
    }
}

impl SummarizationConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Option<Self> {
        // Only create config if explicitly enabled or model ID is set
        let model_id = std::env::var("SUMMARIZATION_MODEL_ID").ok();
        let enabled = std::env::var("SUMMARIZATION_ENABLED")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        if !enabled && model_id.is_none() {
            return None;
        }

        Some(Self {
            model_id: model_id.unwrap_or_else(|| "philschmid/flan-t5-base-samsum".to_string()),
            max_input_length: std::env::var("SUMMARIZATION_MAX_INPUT_LENGTH")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(512),
            max_output_length: std::env::var("SUMMARIZATION_MAX_OUTPUT_LENGTH")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(150),
            min_output_length: std::env::var("SUMMARIZATION_MIN_OUTPUT_LENGTH")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20),
            num_beams: std::env::var("SUMMARIZATION_NUM_BEAMS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4),
            required: std::env::var("SUMMARIZATION_REQUIRED")
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
            hf_token: std::env::var("HF_TOKEN").ok(),
        })
    }
}

/// Summarization service for generating text summaries
pub struct SummarizationService {
    backend: Arc<dyn SummarizationBackend>,
}

impl SummarizationService {
    /// Create a new summarization service with the given backend
    pub fn new(backend: Arc<dyn SummarizationBackend>) -> Self {
        Self { backend }
    }

    /// Create a summarization service with the T5 backend
    pub async fn with_t5(config: &SummarizationConfig) -> Result<Self, OnnxServiceError> {
        let backend = T5SummarizationBackend::new(config).await?;
        Ok(Self {
            backend: Arc::new(backend),
        })
    }

    /// Create a summarization service with the stub backend (for testing)
    pub fn new_stub(max_input_length: usize) -> Self {
        Self {
            backend: Arc::new(StubSummarizationBackend::new(max_input_length)),
        }
    }

    /// Summarize a batch of inputs
    pub async fn summarize(
        &self,
        inputs: &[SummarizationInput],
    ) -> Result<Vec<String>, OnnxServiceError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        log::debug!(
            "Generating summaries for {} inputs using {}",
            inputs.len(),
            self.backend.name()
        );

        self.backend.summarize(inputs).await
    }

    /// Summarize a single text string
    pub async fn summarize_text(&self, text: &str) -> Result<String, OnnxServiceError> {
        let inputs = vec![SummarizationInput::text(text)];
        let mut summaries = self.summarize(&inputs).await?;
        summaries.pop().ok_or_else(|| {
            OnnxServiceError::SummarizationError("No summary returned for text".to_string())
        })
    }

    /// Summarize a single text string with optional custom prompt prefix
    pub async fn summarize_text_with_prefix(
        &self,
        text: &str,
        prompt_prefix: Option<&str>,
    ) -> Result<String, OnnxServiceError> {
        let input = match prompt_prefix {
            Some(prefix) => SummarizationInput::text_with_prefix(text, prefix),
            None => SummarizationInput::text(text),
        };
        let inputs = vec![input];
        let mut summaries = self.summarize(&inputs).await?;
        summaries.pop().ok_or_else(|| {
            OnnxServiceError::SummarizationError("No summary returned for text".to_string())
        })
    }

    /// Summarize a dialogue
    pub async fn summarize_dialogue(
        &self,
        turns: Vec<DialogueTurn>,
    ) -> Result<String, OnnxServiceError> {
        let inputs = vec![SummarizationInput::dialogue(turns)];
        let mut summaries = self.summarize(&inputs).await?;
        summaries.pop().ok_or_else(|| {
            OnnxServiceError::SummarizationError("No summary returned for dialogue".to_string())
        })
    }

    /// Get the maximum input length in tokens
    pub fn max_input_length(&self) -> usize {
        self.backend.max_input_length()
    }

    /// Get the backend name
    pub fn backend_name(&self) -> &str {
        self.backend.name()
    }

    /// Eagerly load the model so errors surface at startup.
    pub async fn warmup(&self) -> Result<(), OnnxServiceError> {
        self.backend.warmup().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialogue_to_text() {
        let turns = vec![
            DialogueTurn {
                speaker: "Alice".to_string(),
                content: "Hello, how are you?".to_string(),
            },
            DialogueTurn {
                speaker: "Bob".to_string(),
                content: "I'm doing great, thanks!".to_string(),
            },
        ];
        let input = SummarizationInput::dialogue(turns);
        let text = input.to_text();
        assert_eq!(
            text,
            "Alice: Hello, how are you?\nBob: I'm doing great, thanks!"
        );
    }

    #[test]
    fn test_text_input() {
        let input = SummarizationInput::text("This is a test.");
        let text = input.to_text();
        assert_eq!(text, "This is a test.");
    }

    #[test]
    fn test_text_with_prefix() {
        let input = SummarizationInput::text_with_prefix("Some content", "compress:");
        assert_eq!(input.to_text(), "Some content");
        assert_eq!(input.prompt_prefix(), Some("compress:"));
    }

    #[test]
    fn test_text_without_prefix() {
        let input = SummarizationInput::text("Some content");
        assert_eq!(input.to_text(), "Some content");
        assert_eq!(input.prompt_prefix(), None);
    }

    #[test]
    fn test_dialogue_with_prefix() {
        let turns = vec![DialogueTurn {
            speaker: "Alice".to_string(),
            content: "Hello".to_string(),
        }];
        let input = SummarizationInput::dialogue_with_prefix(turns, "normalize:");
        assert_eq!(input.prompt_prefix(), Some("normalize:"));
    }

    #[tokio::test]
    async fn test_stub_service() {
        let service = SummarizationService::new_stub(512);
        let result = service.summarize_text("Hello world").await.unwrap();
        assert!(result.starts_with("[STUB SUMMARY]"));
    }

    #[tokio::test]
    async fn test_stub_service_with_prefix() {
        let service = SummarizationService::new_stub(512);
        let result = service
            .summarize_text_with_prefix("Hello world", Some("compress:"))
            .await
            .unwrap();
        assert!(result.starts_with("[STUB SUMMARY]"));
    }
}
