use crate::{AppState, RegisteredUser};
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Response, StatusCode, header};
use dapple_data::query::drafts::{
    select_draft_by_object_id, select_draft_chunks_by_draft_object_id,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// Content streaming handlers
// ============================================================================

/// Stream draft content as raw binary
/// Can be called while draft is still being written - returns chunks available so far
pub async fn draft_content_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    // Get draft metadata
    let draft = match select_draft_by_object_id(&data.db, id).await {
        Ok(d) => d,
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "NOT_FOUND", "message": "draft not found"})),
            ));
        }
    };

    // Get available chunks
    let chunks = match select_draft_chunks_by_draft_object_id(&data.db, id).await {
        Ok(c) => c,
        Err(_) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({"error": "INTERNAL_ERROR", "message": "failed to stream draft content"}),
                ),
            ));
        }
    };

    let chunk_count = chunks.len();

    // Concatenate chunk data
    let mut content = Vec::new();
    for chunk in chunks {
        content.extend(chunk.data);
    }

    // Build response with Transfer-Encoding: chunked (size unknown until finalized)
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &draft.mime_type)
        .header(header::TRANSFER_ENCODING, "chunked")
        .header("X-Draft-Status", "in-progress")
        .header("X-Chunks-Available", chunk_count.to_string())
        .body(Body::from(content))
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "INTERNAL_ERROR", "message": "failed to stream draft content"})),
            )
        })
}
