//! Handler for the /embed endpoint
//!
//! Exposes the EmbeddingService for external callers (primarily the API service).
//! Returns embeddings for a batch of input texts.

use crate::service::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

/// Request to generate embeddings for a batch of texts
#[derive(Debug, Deserialize)]
pub struct EmbedRequest {
    /// The texts to generate embeddings for
    pub texts: Vec<String>,
}

/// Successful response containing embeddings
#[derive(Debug, Serialize)]
pub struct EmbedResponse {
    /// One embedding vector per input text, in the same order
    pub embeddings: Vec<Vec<f32>>,
    /// The dimension of each embedding vector
    pub dimension: usize,
    /// How many of the submitted texts were TRUNCATED to fit the backend's
    /// sequence cap. Passed through from conduit-onnx (0 from the local
    /// backend, which has no cap).
    ///
    /// Truncation is otherwise invisible over the wire — the response is
    /// well-formed either way — so a caller that persists these vectors as a
    /// search index (dapple-prompt-api's guide ingest) must check this and
    /// refuse rather than commit an index whose content is silently clipped.
    pub truncated_count: usize,
}

/// Error response for embed endpoint
#[derive(Debug, Serialize)]
pub struct EmbedErrorResponse {
    /// Human-readable error message
    pub error: String,
}

/// Maximum number of texts to send per request to conduit-onnx.
/// Kept small to bound peak memory on the ONNX inference pod.
const MAX_EMBED_BATCH_SIZE: usize = 8;

/// Handle a batch embedding request.
///
/// Returns 503 if the embedding service is not configured,
/// 500 if embedding generation fails, or 200 with embeddings on success.
///
/// Batches larger than MAX_EMBED_BATCH_SIZE are processed in chunks to
/// prevent memory spikes from large guide ingests.
pub async fn handle_embed(
    State(state): State<AppState>,
    Json(req): Json<EmbedRequest>,
) -> Result<Json<EmbedResponse>, (StatusCode, Json<EmbedErrorResponse>)> {
    // Prefer onnx_client (sub-service) over local embedding service
    if let Some(ref client) = state.onnx_client {
        let result = client.embed(&req.texts).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(EmbedErrorResponse {
                    error: format!("Embedding generation failed: {}", e),
                }),
            )
        })?;

        let batch = result.ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(EmbedErrorResponse {
                    error: "Embedding service not available".to_string(),
                }),
            )
        })?;

        return Ok(Json(EmbedResponse {
            embeddings: batch.embeddings,
            dimension: batch.dimension,
            truncated_count: batch.truncated_count,
        }));
    }

    // Fall back to local embedding service
    let service = state.embedding_service.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(EmbedErrorResponse {
                error: "Embedding service not available".to_string(),
            }),
        )
    })?;

    let embeddings = if req.texts.len() <= MAX_EMBED_BATCH_SIZE {
        service.embed(&req.texts).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(EmbedErrorResponse {
                    error: format!("Embedding generation failed: {}", e),
                }),
            )
        })?
    } else {
        log::info!(
            "Chunking large embedding batch: {} texts into chunks of {}",
            req.texts.len(),
            MAX_EMBED_BATCH_SIZE
        );
        let mut all_embeddings = Vec::with_capacity(req.texts.len());
        for chunk in req.texts.chunks(MAX_EMBED_BATCH_SIZE) {
            let chunk_embeddings = service.embed(chunk).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(EmbedErrorResponse {
                        error: format!("Embedding generation failed: {}", e),
                    }),
                )
            })?;
            all_embeddings.extend(chunk_embeddings);
        }
        all_embeddings
    };

    Ok(Json(EmbedResponse {
        dimension: service.dimension(),
        embeddings,
        // The in-process fallback backend applies no sequence cap, so it never
        // truncates. If it ever gains one, this must stop being a constant.
        truncated_count: 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_deserialization() {
        let json = r#"{"texts": ["hello world", "foo bar"]}"#;
        let req: EmbedRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.texts.len(), 2);
        assert_eq!(req.texts[0], "hello world");
        assert_eq!(req.texts[1], "foo bar");
    }

    #[test]
    fn test_request_empty_texts() {
        let json = r#"{"texts": []}"#;
        let req: EmbedRequest = serde_json::from_str(json).unwrap();
        assert!(req.texts.is_empty());
    }

    #[test]
    fn test_response_serialization() {
        let response = EmbedResponse {
            embeddings: vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]],
            dimension: 3,
            truncated_count: 0,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("embeddings"));
        assert!(json.contains("dimension"));
        assert!(json.contains("0.1"));
        // The API's ingest refuses to index when this is non-zero, so it must
        // actually reach the wire rather than being skipped by serde.
        assert!(json.contains("truncated_count"));
    }

    #[test]
    fn test_error_response_serialization() {
        let response = EmbedErrorResponse {
            error: "Embedding service not available".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("Embedding service not available"));
    }
}
