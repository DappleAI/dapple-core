//! `/api/journeys/{id}/uploads` — staged operator uploads.
//!
//! Durable record of "asset X was uploaded for input I of journey J before
//! the form was submitted". The asset↔slot *binding* is still written at
//! submit time via `POST /api/dispatch-intent-bindings`; these endpoints only
//! cover the pre-submit window so clients can rehydrate file fields after a
//! reload and recover the asset id at submit when their local cache was
//! evicted. Deliberately zero contact with dispatch/binding code paths —
//! staging an upload can never fire a dispatch.

use crate::AppState;
use crate::extract_path_uuid::ValidatedPathUuid;
use crate::handler::{index_response_handler, post_response_handler};
use crate::schema::journey_asset_uploads::CreateJourneyAssetUploadSetSchema;
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::model::journey_asset_uploads::{InsertJourneyAssetUploadArgs, JourneyAssetUpload};
use dapple_data::query::journey_asset_uploads::{
    insert_journey_asset_uploads, remove_journey_asset_upload,
    select_journey_asset_uploads_by_journey,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// `POST /api/journeys/{id}/uploads` — stage uploads for a journey (batch).
pub async fn journey_asset_uploads_post_handler(
    State(state): State<Arc<AppState>>,
    _user: crate::RegisteredUser,
    ValidatedPathUuid(journey_id): ValidatedPathUuid,
    Json(payload): Json<CreateJourneyAssetUploadSetSchema>,
) -> impl IntoApiResponse {
    if payload.objs.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "EMPTY_BATCH",
                "message": "objs must contain at least one staged upload",
            })),
        )
            .into_response();
    }

    let args = InsertJourneyAssetUploadArgs {
        journey_id,
        asset_ids: payload.objs.iter().map(|o| o.asset_id).collect(),
        file_names: payload.objs.iter().map(|o| o.file_name.clone()).collect(),
        input_keys: payload.objs.iter().map(|o| o.input_key.clone()).collect(),
    };

    match insert_journey_asset_uploads(&state.db, args).await {
        // The insert validates the journey via its CTE: unknown journey_id
        // inserts nothing, which we surface as 404 rather than an empty 201.
        Ok(rows) if rows.is_empty() => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "RESOURCE_NOT_FOUND",
                "message": "journey not found",
            })),
        )
            .into_response(),
        res => post_response_handler(
            "journey_asset_uploads",
            res.map(|rows| {
                rows.into_iter()
                    .map(JourneyAssetUpload::from)
                    .collect::<Vec<_>>()
            }),
        )
        .into_response(),
    }
}

/// `GET /api/journeys/{id}/uploads` — active staged uploads, creation order.
pub async fn journey_asset_uploads_index_handler(
    State(state): State<Arc<AppState>>,
    _user: crate::RegisteredUser,
    ValidatedPathUuid(journey_id): ValidatedPathUuid,
) -> impl IntoApiResponse {
    index_response_handler(
        "journey_asset_uploads",
        select_journey_asset_uploads_by_journey(&state.db, journey_id)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(JourneyAssetUpload::from)
                    .collect::<Vec<_>>()
            }),
    )
}

/// `DELETE /api/journeys/{id}/uploads/{upload_id}` — soft-remove one staged
/// upload. 404 when there is no active upload with that id on that journey.
pub async fn journey_asset_upload_delete_handler(
    State(state): State<Arc<AppState>>,
    _user: crate::RegisteredUser,
    Path((journey_id, upload_id)): Path<(Uuid, Uuid)>,
) -> impl IntoApiResponse {
    match remove_journey_asset_upload(&state.db, journey_id, upload_id).await {
        Ok(Some(removed)) => (
            StatusCode::OK,
            Json(json!({
                "status": "success",
                "obj": {
                    "id": removed.journey_asset_upload_id,
                    "removed_at": removed.removed_at.to_string(),
                },
            })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "RESOURCE_NOT_FOUND",
                "message": "no active staged upload with that id on this journey",
            })),
        )
            .into_response(),
        Err(e) => {
            log::error!("journey_asset_upload_delete: data error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "DATABASE_ERROR",
                    "message": "failed to remove staged upload",
                })),
            )
                .into_response()
        }
    }
}
