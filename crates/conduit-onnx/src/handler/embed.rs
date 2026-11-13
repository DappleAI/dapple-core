//! Handler for the /embed endpoint

use crate::service::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

/// Request to generate embeddings for a batch of texts
#[derive(Debug, Deserialize)]
pub struct EmbedRequest {
    pub texts: Vec<String>,
}

/// Successful response containing embeddings
#[derive(Debug, Serialize)]
pub struct EmbedResponse {
    pub embeddings: Vec<Vec<f32>>,
    pub dimension: usize,
    /// How many of the submitted texts exceeded the backend's sequence cap and
    /// were TRUNCATED before embedding. Non-zero means those vectors do not
    /// represent the whole text.
    ///
    /// Reported because truncation is otherwise invisible over the wire: the
    /// response is well-formed either way and the only evidence is a WARN in
    /// this pod's log. A caller that persists these vectors (guide ingest) must
    /// refuse rather than commit an index whose tail is missing; a caller that
    /// clips deliberately (a clamped RAG query) ignores it.
    pub truncated_count: usize,
}

/// Error response for embed endpoint
#[derive(Debug, Serialize)]
pub struct EmbedErrorResponse {
    pub error: String,
}

/// Maximum number of texts to embed in a single ONNX inference call.
/// Kept low to avoid OOM kills — ModernBERT uses ~400MB per batch of 4.
const MAX_EMBED_BATCH_SIZE: usize = 4;

pub async fn handle_embed(
    State(state): State<AppState>,
    Json(req): Json<EmbedRequest>,
) -> Result<Json<EmbedResponse>, (StatusCode, Json<EmbedErrorResponse>)> {
    let service = state.embedding_service.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(EmbedErrorResponse {
                error: "Embedding service not available".to_string(),
            }),
        )
    })?;

    let (embeddings, truncated_count) = if req.texts.len() <= MAX_EMBED_BATCH_SIZE {
        let batch = service.embed_batch(&req.texts).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(EmbedErrorResponse {
                    error: format!("Embedding generation failed: {}", e),
                }),
            )
        })?;
        (batch.embeddings, batch.truncated_count)
    } else {
        log::info!(
            "Chunking large embedding batch: {} texts into chunks of {}",
            req.texts.len(),
            MAX_EMBED_BATCH_SIZE
        );
        let mut all_embeddings = Vec::with_capacity(req.texts.len());
        // Summed across chunks: the count describes the whole REQUEST, so a
        // caller need not know this endpoint chunks internally.
        let mut truncated = 0usize;
        for chunk in req.texts.chunks(MAX_EMBED_BATCH_SIZE) {
            let batch = service.embed_batch(chunk).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(EmbedErrorResponse {
                        error: format!("Embedding generation failed: {}", e),
                    }),
                )
            })?;
            all_embeddings.extend(batch.embeddings);
            truncated += batch.truncated_count;
        }
        (all_embeddings, truncated)
    };

    Ok(Json(EmbedResponse {
        dimension: service.dimension(),
        embeddings,
        truncated_count,
    }))
}
