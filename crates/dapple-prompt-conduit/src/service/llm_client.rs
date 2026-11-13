//! HTTP client for conduit-llm sub-service.
//!
//! Streams LLM generation from the internal conduit-llm service via SSE.

use crate::error::ConduitError;
use crate::service::llm::{StreamChunk, TokenStream, UsageMetadata};
use crate::service::model::ModelConfig;
use crate::service::template::RenderedPrompt;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Client for communicating with the conduit-llm internal service.
pub struct LlmInferenceClient {
    client: Client,
    base_url: String,
    internal_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct GenerateMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct GenerateRequest {
    messages: Vec<GenerateMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GenerateEvent {
    Token { content: String },
    Done,
    Error { message: String },
}

impl LlmInferenceClient {
    pub fn new(base_url: &str, internal_key: Option<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            internal_key,
        }
    }

    /// Stream tokens from conduit-llm for the given rendered prompt.
    ///
    /// Returns a TokenStream (same type the LlmBackend trait produces). Usage is
    /// delivered at stream-END via a trailing [`StreamChunk::Final`]; this backend
    /// doesn't get provider counts, so that Final carries empty usage.
    pub async fn generate_streaming(
        &self,
        prompt: &RenderedPrompt,
        model_config: &ModelConfig,
    ) -> Result<TokenStream, ConduitError> {
        let url = format!("{}/generate", self.base_url);

        let messages: Vec<GenerateMessage> = prompt
            .messages
            .iter()
            .map(|m| GenerateMessage {
                role: m.role.clone().unwrap_or_else(|| "user".to_string()),
                content: m.content.clone(),
            })
            .collect();

        let req = GenerateRequest {
            messages,
            temperature: model_config.temperature,
            top_p: model_config.top_p,
            top_k: model_config.top_k,
            max_tokens: model_config.max_tokens,
        };

        let mut request = self
            .client
            .post(&url)
            .header("Accept", "text/event-stream")
            .json(&req);

        if let Some(ref key) = self.internal_key {
            request = request.header("x-internal-key", key);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ConduitError::LlmError(format!("conduit-llm request failed: {}", e)))?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ConduitError::LlmError(format!(
                "conduit-llm /generate failed: {}",
                body
            )));
        }

        // Parse SSE stream into a TokenStream
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamChunk, ConduitError>>(32);

        let byte_stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut stream = byte_stream;

            while let Some(chunk_result) = stream.next().await {
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx
                            .send(Err(ConduitError::LlmError(format!(
                                "Stream read error: {}",
                                e
                            ))))
                            .await;
                        break;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process complete SSE event blocks
                while let Some(end) = buffer.find("\n\n") {
                    let event_block = buffer[..end].to_string();
                    buffer = buffer[end + 2..].to_string();

                    // Extract data: line
                    for line in event_block.lines() {
                        if let Some(data) = line.strip_prefix("data:") {
                            let data = data.trim();
                            if data.is_empty() {
                                continue;
                            }

                            match serde_json::from_str::<GenerateEvent>(data) {
                                Ok(GenerateEvent::Token { content }) => {
                                    if tx.send(Ok(StreamChunk::Token(content))).await.is_err() {
                                        return; // receiver dropped
                                    }
                                }
                                Ok(GenerateEvent::Done) => {
                                    // Emit the trailing Final at clean stream-end.
                                    let _ = tx
                                        .send(Ok(StreamChunk::Final(UsageMetadata::default())))
                                        .await;
                                    return;
                                }
                                Ok(GenerateEvent::Error { message }) => {
                                    let _ = tx
                                        .send(Err(ConduitError::LlmError(format!(
                                            "conduit-llm error: {}",
                                            message
                                        ))))
                                        .await;
                                    return;
                                }
                                Err(e) => {
                                    log::warn!("Failed to parse SSE event: {} - {}", data, e);
                                }
                            }
                        }
                    }
                }
            }

            // If the byte stream ended without an explicit Done event, still emit a
            // trailing Final so the consumer has a uniform contract.
            let _ = tx
                .send(Ok(StreamChunk::Final(UsageMetadata::default())))
                .await;
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    /// Check if this client should handle the given model config.
    ///
    /// Same logic as the original MistralRsBackend::supports().
    pub fn supports(&self, model: &ModelConfig) -> bool {
        model.lineage_key == "mistral"
            || model.provider_key == "self-hosted"
            || model.lineage_key == "local"
    }
}
