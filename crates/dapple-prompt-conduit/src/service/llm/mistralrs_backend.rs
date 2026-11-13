//! mistral.rs inference backend
//!
//! Replaces the hand-rolled Candle LLM pipeline with mistral.rs, which handles
//! quantization, KV cache, chat templates, and streaming internally.
//!
//! Uses pre-quantized GGUF weights for instant loading (~4GB, no ISQ startup delay).

use async_trait::async_trait;
use mistralrs::{
    ChatCompletionChunkResponse, ChunkChoice, Delta, GgufModelBuilder, Model, RequestBuilder,
    Response, TextMessageRole, TokenSource,
};
use std::sync::Arc;
use tokio::sync::OnceCell;

use super::{LlmBackend, StreamChunk, TokenStream, UsageMetadata};
use crate::error::ConduitError;
use crate::service::model::ModelConfig;
use crate::service::template::RenderedPrompt;

/// Default GGUF repo and filename
const DEFAULT_GGUF_MODEL_ID: &str = "bartowski/Mistral-7B-Instruct-v0.3-GGUF";
const DEFAULT_GGUF_FILE: &str = "Mistral-7B-Instruct-v0.3-Q4_K_M.gguf";
/// Source tokenizer/chat template from the base model
const DEFAULT_TOK_MODEL_ID: &str = "mistralai/Mistral-7B-Instruct-v0.3";

/// Configuration for the mistral.rs GGUF backend
#[derive(Debug, Clone)]
pub struct MistralRsConfig {
    /// HuggingFace repo containing the GGUF file (or local directory path)
    pub model_id: String,
    /// GGUF filename within the repo
    pub gguf_file: String,
    /// Model ID to source tokenizer and chat template from
    pub tok_model_id: String,
    pub hf_token: Option<String>,
}

impl Default for MistralRsConfig {
    fn default() -> Self {
        Self {
            model_id: DEFAULT_GGUF_MODEL_ID.to_string(),
            gguf_file: DEFAULT_GGUF_FILE.to_string(),
            tok_model_id: DEFAULT_TOK_MODEL_ID.to_string(),
            hf_token: None,
        }
    }
}

impl MistralRsConfig {
    pub fn from_env() -> Self {
        Self {
            model_id: std::env::var("MISTRALRS_MODEL_ID")
                .unwrap_or_else(|_| DEFAULT_GGUF_MODEL_ID.to_string()),
            gguf_file: std::env::var("MISTRALRS_GGUF_FILE")
                .unwrap_or_else(|_| DEFAULT_GGUF_FILE.to_string()),
            tok_model_id: std::env::var("MISTRALRS_TOK_MODEL_ID")
                .unwrap_or_else(|_| DEFAULT_TOK_MODEL_ID.to_string()),
            hf_token: std::env::var("HF_TOKEN")
                .ok()
                .or_else(|| std::env::var("HUGGINGFACE_TOKEN").ok()),
        }
    }
}

/// LLM backend using mistral.rs with pre-quantized GGUF weights
pub struct MistralRsBackend {
    config: MistralRsConfig,
    model: OnceCell<Arc<Model>>,
}

impl MistralRsBackend {
    pub fn new(config: MistralRsConfig) -> Self {
        Self {
            config,
            model: OnceCell::new(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(MistralRsConfig::from_env())
    }

    async fn get_model(&self) -> Result<&Arc<Model>, ConduitError> {
        self.model.get_or_try_init(|| self.build_model()).await
    }

    async fn build_model(&self) -> Result<Arc<Model>, ConduitError> {
        log::info!(
            "Loading mistral.rs GGUF model: {}/{}",
            self.config.model_id,
            self.config.gguf_file
        );

        let mut builder = GgufModelBuilder::new(
            self.config.model_id.clone(),
            vec![self.config.gguf_file.clone()],
        )
        .with_tok_model_id(self.config.tok_model_id.clone())
        .with_force_cpu()
        .with_logging();

        if self.config.hf_token.is_some() {
            builder = builder.with_token_source(TokenSource::EnvVar("HF_TOKEN".to_string()));
        }

        let model = builder.build().await.map_err(|e| {
            ConduitError::LlmError(format!("Failed to build mistral.rs GGUF model: {}", e))
        })?;

        log::info!("mistral.rs GGUF model loaded successfully");
        Ok(Arc::new(model))
    }

    fn build_request(&self, prompt: &RenderedPrompt, model_config: &ModelConfig) -> RequestBuilder {
        let mut request = RequestBuilder::new();

        for msg in &prompt.messages {
            let role = match msg.role.as_deref() {
                Some("system") => TextMessageRole::System,
                Some("assistant") | Some("ai") => TextMessageRole::Assistant,
                Some("user") | Some("human") | None => TextMessageRole::User,
                Some(other) => TextMessageRole::Custom(other.to_string()),
            };
            request = request.add_message(role, &msg.content);
        }

        if let Some(temp) = model_config.temperature {
            request = request.set_sampler_temperature(temp as f64);
        }
        if let Some(top_p) = model_config.top_p {
            request = request.set_sampler_topp(top_p as f64);
        }
        if let Some(top_k) = model_config.top_k {
            request = request.set_sampler_topk(top_k);
        }
        if let Some(max_tokens) = model_config.max_tokens {
            request = request.set_sampler_max_len(max_tokens);
        }

        request
    }
}

#[async_trait]
impl LlmBackend for MistralRsBackend {
    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        model_config: &ModelConfig,
    ) -> Result<TokenStream, ConduitError> {
        let model = self.get_model().await?.clone();
        let request = self.build_request(prompt, model_config);

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamChunk, ConduitError>>(32);

        tokio::spawn(async move {
            let stream_result = model.stream_chat_request(request).await;

            let mut stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx
                        .send(Err(ConduitError::LlmError(format!(
                            "mistral.rs stream init failed: {}",
                            e
                        ))))
                        .await;
                    return;
                }
            };

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Response::Chunk(ChatCompletionChunkResponse { choices, .. }) => {
                        if let Some(ChunkChoice {
                            delta:
                                Delta {
                                    content: Some(content),
                                    ..
                                },
                            ..
                        }) = choices.first()
                        {
                            if !content.is_empty()
                                && tx
                                    .send(Ok(StreamChunk::Token(content.clone())))
                                    .await
                                    .is_err()
                            {
                                break; // Receiver dropped
                            }
                        }
                    }
                    Response::Done(_) => break,
                    Response::ValidationError(e) => {
                        let _ = tx
                            .send(Err(ConduitError::LlmError(format!(
                                "mistral.rs validation error: {}",
                                e
                            ))))
                            .await;
                        break;
                    }
                    Response::InternalError(e) => {
                        let _ = tx
                            .send(Err(ConduitError::LlmError(format!(
                                "mistral.rs internal error: {}",
                                e
                            ))))
                            .await;
                        break;
                    }
                    Response::ModelError(msg, _) => {
                        let _ = tx
                            .send(Err(ConduitError::LlmError(format!(
                                "mistral.rs model error: {}",
                                msg
                            ))))
                            .await;
                        break;
                    }
                    Response::CompletionDone(_) => break,
                    Response::CompletionModelError(msg, _) => {
                        let _ = tx
                            .send(Err(ConduitError::LlmError(format!(
                                "mistral.rs completion error: {}",
                                msg
                            ))))
                            .await;
                        break;
                    }
                    _ => {} // ImageGeneration, Speech, Raw — not relevant for text
                }
            }

            // Emit a trailing Final chunk at stream-end. mistral.rs doesn't report
            // provider token counts here, so usage is empty (the consumer falls back
            // to its streamed-token estimate).
            let _ = tx
                .send(Ok(StreamChunk::Final(UsageMetadata::default())))
                .await;
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn supports(&self, model: &ModelConfig) -> bool {
        model.lineage_key == "mistral"
            || model.provider_key == "self-hosted"
            || model.lineage_key == "local"
    }

    async fn warmup(&self) -> Result<(), ConduitError> {
        log::info!("Warming up mistral.rs backend — loading GGUF model...");
        self.get_model().await?;
        log::info!("mistral.rs backend warm — model ready for inference");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::template::PromptMessage;

    #[test]
    fn test_config_default() {
        let config = MistralRsConfig::default();
        assert_eq!(config.model_id, DEFAULT_GGUF_MODEL_ID);
        assert_eq!(config.gguf_file, DEFAULT_GGUF_FILE);
        assert_eq!(config.tok_model_id, DEFAULT_TOK_MODEL_ID);
    }

    #[test]
    fn test_supports() {
        let backend = MistralRsBackend::new(MistralRsConfig::default());

        let mistral_model = ModelConfig {
            lineage_key: "mistral".to_string(),
            provider_key: "mistral".to_string(),
            tier: "7b-instruct".to_string(),
            ..Default::default()
        };
        assert!(backend.supports(&mistral_model));

        let self_hosted_model = ModelConfig {
            lineage_key: "llama".to_string(),
            provider_key: "self-hosted".to_string(),
            tier: "7b".to_string(),
            ..Default::default()
        };
        assert!(backend.supports(&self_hosted_model));

        let anthropic_model = ModelConfig {
            lineage_key: "claude-3-5".to_string(),
            provider_key: "anthropic".to_string(),
            tier: "sonnet".to_string(),
            ..Default::default()
        };
        assert!(!backend.supports(&anthropic_model));
    }

    #[test]
    fn test_build_request_maps_roles() {
        let backend = MistralRsBackend::new(MistralRsConfig::default());
        let prompt = RenderedPrompt {
            prompt_name: "test".to_string(),
            messages: vec![
                PromptMessage {
                    role: Some("system".to_string()),
                    content: "You are helpful.".to_string(),
                },
                PromptMessage {
                    role: Some("user".to_string()),
                    content: "Hello!".to_string(),
                },
                PromptMessage {
                    role: Some("assistant".to_string()),
                    content: "Hi there!".to_string(),
                },
            ],
        };

        let model_config = ModelConfig {
            temperature: Some(0.5),
            top_p: Some(0.8),
            top_k: Some(50),
            max_tokens: Some(1024),
            ..Default::default()
        };

        // Just verify it doesn't panic — we can't inspect RequestBuilder internals
        let _request = backend.build_request(&prompt, &model_config);
    }
}
