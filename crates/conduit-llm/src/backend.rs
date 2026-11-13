//! MistralRS GGUF inference backend
//!
//! Loads pre-quantized GGUF weights via mistral.rs and exposes a simple
//! generate interface that streams tokens over a channel.

use crate::config::MistralRsConfig;
use crate::error::LlmServiceError;
use crate::handler::generate::GenerateRequest;
use mistralrs::{
    ChatCompletionChunkResponse, ChunkChoice, Delta, GgufModelBuilder, Model, RequestBuilder,
    Response, TextMessageRole, TokenSource,
};
use std::sync::Arc;
use tokio::sync::OnceCell;

/// MistralRS GGUF backend — lazy-loads the model on first request or warmup.
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

    async fn get_model(&self) -> Result<&Arc<Model>, LlmServiceError> {
        self.model.get_or_try_init(|| self.build_model()).await
    }

    async fn build_model(&self) -> Result<Arc<Model>, LlmServiceError> {
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
            LlmServiceError::LlmError(format!("Failed to build mistral.rs GGUF model: {}", e))
        })?;

        log::info!("mistral.rs GGUF model loaded successfully");
        Ok(Arc::new(model))
    }

    /// Eagerly load the model (called during startup warmup).
    pub async fn warmup(&self) -> Result<(), LlmServiceError> {
        log::info!("Warming up mistral.rs backend - loading GGUF model...");
        self.get_model().await?;
        log::info!("mistral.rs backend warm - model ready for inference");
        Ok(())
    }

    /// Stream tokens for a generate request.
    ///
    /// Returns a receiver that yields token strings or errors.
    pub async fn generate_streaming(
        &self,
        req: &GenerateRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String, LlmServiceError>>, LlmServiceError> {
        let model = self.get_model().await?.clone();
        let request = self.build_request(req);

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, LlmServiceError>>(32);

        tokio::spawn(async move {
            let stream_result = model.stream_chat_request(request).await;

            let mut stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx
                        .send(Err(LlmServiceError::LlmError(format!(
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
                            if !content.is_empty() && tx.send(Ok(content.clone())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Response::Done(_) | Response::CompletionDone(_) => break,
                    Response::ValidationError(e) => {
                        let _ = tx
                            .send(Err(LlmServiceError::LlmError(format!(
                                "mistral.rs validation error: {}",
                                e
                            ))))
                            .await;
                        break;
                    }
                    Response::InternalError(e) => {
                        let _ = tx
                            .send(Err(LlmServiceError::LlmError(format!(
                                "mistral.rs internal error: {}",
                                e
                            ))))
                            .await;
                        break;
                    }
                    Response::ModelError(msg, _) | Response::CompletionModelError(msg, _) => {
                        let _ = tx
                            .send(Err(LlmServiceError::LlmError(format!(
                                "mistral.rs model error: {}",
                                msg
                            ))))
                            .await;
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(rx)
    }

    fn build_request(&self, req: &GenerateRequest) -> RequestBuilder {
        let mut request = RequestBuilder::new();

        for msg in &req.messages {
            let role = map_role(&msg.role);
            request = request.add_message(role, &msg.content);
        }

        if let Some(temp) = req.temperature {
            request = request.set_sampler_temperature(temp as f64);
        }
        if let Some(top_p) = req.top_p {
            request = request.set_sampler_topp(top_p as f64);
        }
        if let Some(top_k) = req.top_k {
            request = request.set_sampler_topk(top_k);
        }
        if let Some(max_tokens) = req.max_tokens {
            request = request.set_sampler_max_len(max_tokens);
        }

        request
    }
}

fn map_role(role: &str) -> TextMessageRole {
    match role {
        "system" => TextMessageRole::System,
        "assistant" | "ai" => TextMessageRole::Assistant,
        "user" | "human" => TextMessageRole::User,
        other => TextMessageRole::Custom(other.to_string()),
    }
}
