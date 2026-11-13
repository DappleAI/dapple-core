//! Handler for the /search endpoint
//!
//! Exposes pgvector semantic search over guide content embeddings.
//! Returns ranked results filtered by audience tags and locale.

use crate::service::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use dapple_data::query::guide_content_embeddings;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn default_tags() -> Vec<String> {
    vec!["universal".to_string()]
}

fn default_locale() -> String {
    "en".to_string()
}

fn default_limit() -> i32 {
    20
}

fn default_threshold() -> f64 {
    0.0
}

/// Request to search guide content embeddings
#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    /// Natural language search query
    pub q: String,
    /// Audience tag filter
    #[serde(default = "default_tags")]
    pub tags: Vec<String>,
    /// Locale filter
    #[serde(default = "default_locale")]
    pub locale: String,
    /// Results limit (clamped 1..100)
    #[serde(default = "default_limit")]
    pub limit: i32,
    /// Minimum similarity score
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    /// Scope to specific guides (empty = all guides)
    #[serde(default)]
    pub guide_ids: Vec<Uuid>,
}

/// Successful response containing search results
#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub count: usize,
}

/// A single search result item
#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub block_id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub directive_type: String,
    pub chunk_text: String,
    pub metadata: serde_json::Value,
    pub similarity: f64,
}

/// Error response for search endpoint
#[derive(Debug, Serialize)]
pub struct SearchErrorResponse {
    pub error: String,
}

/// Handle a semantic search request.
///
/// Returns 400 if query is empty, 503 if embedding service is not configured,
/// 500 if embedding or search fails, or 200 with results on success.
pub async fn handle_search(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<SearchErrorResponse>)> {
    // Validate query is non-empty
    if req.q.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(SearchErrorResponse {
                error: "Query parameter 'q' must not be empty".to_string(),
            }),
        ));
    }

    // Get embedding service
    let embedding_service = state.embedding_service.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SearchErrorResponse {
                error: "Embedding service not available".to_string(),
            }),
        )
    })?;

    // Generate query embedding
    let query_embedding = embedding_service.embed_one(&req.q).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SearchErrorResponse {
                error: format!("Embedding generation failed: {}", e),
            }),
        )
    })?;

    // Clamp limit
    let limit = req.limit.clamp(1, 100);

    // Search pgvector
    let tag_keys: Vec<String> = req.tags;
    let results = if req.guide_ids.is_empty() {
        guide_content_embeddings::search_guide_content_embeddings(
            &state.pool,
            &query_embedding,
            &tag_keys,
            &req.locale,
            limit,
        )
        .await
    } else {
        guide_content_embeddings::search_guide_content_embeddings_by_guide_ids(
            &state.pool,
            &query_embedding,
            &req.guide_ids,
            &tag_keys,
            &req.locale,
            limit,
        )
        .await
    }
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SearchErrorResponse {
                error: format!("Search failed: {}", e),
            }),
        )
    })?;

    // Filter by threshold and map to response items
    let items: Vec<SearchResultItem> = results
        .into_iter()
        .filter(|r| r.similarity >= req.threshold)
        .map(|r| SearchResultItem {
            block_id: r.block_id,
            guide_id: r.guide_id,
            chapter_id: r.chapter_id,
            directive_type: r.directive_type_key,
            chunk_text: r.chunk_text,
            metadata: r.metadata,
            similarity: r.similarity,
        })
        .collect();

    let count = items.len();
    Ok(Json(SearchResponse {
        results: items,
        count,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_deserialization_minimal() {
        let json = r#"{"q": "authentication"}"#;
        let req: SearchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.q, "authentication");
        assert_eq!(req.tags, vec!["universal"]);
        assert_eq!(req.locale, "en");
        assert_eq!(req.limit, 20);
        assert_eq!(req.threshold, 0.0);
        assert!(req.guide_ids.is_empty());
    }

    #[test]
    fn test_request_deserialization_full() {
        let json = r#"{
            "q": "workflow",
            "tags": ["agent", "coder"],
            "locale": "pt-BR",
            "limit": 5,
            "threshold": 0.3,
            "guide_ids": ["550e8400-e29b-41d4-a716-446655440000"]
        }"#;
        let req: SearchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.q, "workflow");
        assert_eq!(req.tags, vec!["agent", "coder"]);
        assert_eq!(req.locale, "pt-BR");
        assert_eq!(req.limit, 5);
        assert_eq!(req.threshold, 0.3);
        assert_eq!(req.guide_ids.len(), 1);
    }

    #[test]
    fn test_response_serialization() {
        let response = SearchResponse {
            results: vec![SearchResultItem {
                block_id: Uuid::nil(),
                guide_id: Uuid::nil(),
                chapter_id: Uuid::nil(),
                directive_type: "prose".to_string(),
                chunk_text: "hello".to_string(),
                metadata: serde_json::json!({}),
                similarity: 0.85,
            }],
            count: 1,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("results"));
        assert!(json.contains("count"));
        assert!(json.contains("0.85"));
    }

    #[test]
    fn test_error_response_serialization() {
        let response = SearchErrorResponse {
            error: "Query must not be empty".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("Query must not be empty"));
    }
}
