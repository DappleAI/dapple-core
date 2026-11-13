//! Handler for the /generate SSE endpoint
//!
//! Accepts a list of messages + sampling params and streams tokens back via SSE.

use crate::backend::MistralRsBackend;
use crate::error::LlmServiceError;
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;

/// A single message in the conversation
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GenerateMessage {
    pub role: String,
    pub content: String,
}

/// Request to generate a streaming response
#[derive(Debug, Deserialize)]
pub struct GenerateRequest {
    pub messages: Vec<GenerateMessage>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub top_k: Option<usize>,
    #[serde(default)]
    pub max_tokens: Option<usize>,
}

/// SSE event types sent back to conduit
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GenerateEvent {
    Token { content: String },
    Done,
    Error { message: String },
}

type GenerateStream = BoxStream<'static, Result<Event, Infallible>>;

pub async fn handle_generate(
    State(backend): State<Arc<MistralRsBackend>>,
    Json(req): Json<GenerateRequest>,
) -> Result<Sse<GenerateStream>, LlmServiceError> {
    if req.messages.is_empty() {
        return Err(LlmServiceError::InvalidRequest(
            "Messages cannot be empty".to_string(),
        ));
    }

    log::debug!(
        "Generate request: {} messages, temp={:?}, max_tokens={:?}",
        req.messages.len(),
        req.temperature,
        req.max_tokens
    );

    let mut rx = backend.generate_streaming(&req).await?;

    let stream = async_stream::stream! {
        while let Some(result) = rx.recv().await {
            match result {
                Ok(content) => {
                    let event = GenerateEvent::Token { content };
                    if let Ok(data) = serde_json::to_string(&event) {
                        yield Ok(Event::default().data(data));
                    }
                }
                Err(e) => {
                    let event = GenerateEvent::Error { message: e.to_string() };
                    if let Ok(data) = serde_json::to_string(&event) {
                        yield Ok(Event::default().data(data));
                    }
                    break;
                }
            }
        }
        // Send done event
        let done = GenerateEvent::Done;
        if let Ok(data) = serde_json::to_string(&done) {
            yield Ok(Event::default().data(data));
        }
    };

    Ok(Sse::new(Box::pin(stream)))
}
