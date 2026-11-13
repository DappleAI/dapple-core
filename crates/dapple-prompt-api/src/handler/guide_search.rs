use crate::extract_qs::QsQuery as Query;
use crate::handler::index_response_handler;
use crate::schema::guide_search::GuideSearchParams;
use crate::service::embedding_client::EmbeddingClient;
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use dapple_data::model::guide_content_embeddings::GuideSearchResult;
use dapple_data::query::guide_content_embeddings::{
    search_guide_content_embeddings, search_guide_content_embeddings_by_guide_ids,
};
use serde_json::json;
use std::sync::Arc;

pub async fn guide_search_handler(
    opts: Query<GuideSearchParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    // 1. Validate query
    if params.q.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "EMPTY_QUERY",
                "message": "Search query 'q' cannot be empty"
            })),
        ));
    }

    let resolved = params.resolve();

    // 2. Get conduit URL
    let conduit_url = match &data.conduit_url {
        Some(url) => url.clone(),
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "SEARCH_UNAVAILABLE",
                    "message": "Search is not available: embedding service not configured"
                })),
            ));
        }
    };

    // 3. Embed the query text
    let client = EmbeddingClient::new(&conduit_url);
    // `truncated_count` is deliberately ignored here: this is a QUERY, not an
    // index write. A very long search string clipped to the model's cap still
    // yields a usable query vector, and there is nothing persisted to be lossy.
    // The ingest path (handler::guide_embeddings) is where it must be fatal.
    let query_embedding = match client.embed(std::slice::from_ref(&resolved.q)).await {
        Ok(Some(batch)) if !batch.embeddings.is_empty() => {
            batch.embeddings.into_iter().next().unwrap()
        }
        Ok(Some(_)) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "EMBEDDING_FAILED",
                    "message": "Embedding service returned empty result"
                })),
            ));
        }
        Ok(None) => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "SEARCH_UNAVAILABLE",
                    "message": "Embedding service not available"
                })),
            ));
        }
        Err(e) => {
            log::error!("Search embedding failed: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "EMBEDDING_FAILED",
                    "message": format!("Failed to embed search query: {}", e)
                })),
            ));
        }
    };

    // 4. Run pgvector similarity search
    let search_results = if let Some(ref guide_ids) = resolved.guide_ids {
        search_guide_content_embeddings_by_guide_ids(
            &data.db,
            &query_embedding,
            guide_ids,
            &resolved.tags,
            &resolved.locale,
            resolved.limit,
        )
        .await
    } else {
        search_guide_content_embeddings(
            &data.db,
            &query_embedding,
            &resolved.tags,
            &resolved.locale,
            resolved.limit,
        )
        .await
    };

    // 5. Convert to DTOs, apply threshold filter
    let objs: Result<Vec<GuideSearchResult>, _> = search_results.map(|results| {
        results
            .into_iter()
            .filter(|r| r.similarity >= resolved.threshold)
            .map(GuideSearchResult::from)
            .collect()
    });

    Ok(index_response_handler("guide_search", objs))
}
