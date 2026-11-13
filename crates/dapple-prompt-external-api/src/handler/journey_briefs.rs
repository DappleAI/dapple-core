//! `GET /api/journeys/{id}/briefs` on the external surface.
//!
//! Returns the briefs attached to a journey, after validating the journey
//! lives in the caller's namespace. The actual JOIN through
//! `journey_brief_packs → brief_packs → brief_groups → briefs` is unchanged
//! from the internal API.

use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::query::journeys::{journey_in_namespace, select_briefs_by_journey_object_id};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn journey_briefs_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(journey_id): Path<Uuid>,
) -> impl IntoApiResponse {
    match journey_in_namespace(&state.db, journey_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found("journey", journey_id),
        Err(e) => return db_error("journey_in_namespace", e),
    }

    match select_briefs_by_journey_object_id(&state.db, journey_id).await {
        Ok(briefs) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": briefs.len(),
                "objs": briefs,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_briefs_by_journey_object_id", e),
    }
}

fn not_found(kind: &str, id: Uuid) -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "RESOURCE_NOT_FOUND",
            "message": format!("{} {} not found in caller's namespace", kind, id),
        })),
    )
        .into_response()
}

fn db_error(op: &str, e: dapple_data::error::DataError) -> axum::response::Response {
    log::error!("{}: {}", op, e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "DATABASE_ERROR", "message": format!("{} failed", op) })),
    )
        .into_response()
}
