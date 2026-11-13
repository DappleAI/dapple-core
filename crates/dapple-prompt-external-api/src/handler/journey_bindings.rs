//! `GET /api/journeys/{id}/bindings` on the external surface.
//!
//! Current binding state for the journey, namespace-gated via
//! `journey_in_namespace` (404 on foreign / unknown — no existence leak).
//! `Cache-Control: no-store`.

use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use dapple_data::query::journeys::journey_in_namespace;
use dapple_data::view::journey_bindings::build_bindings;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn journey_bindings_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(journey_id): Path<Uuid>,
) -> impl IntoApiResponse {
    match journey_in_namespace(&state.db, journey_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found(journey_id),
        Err(e) => return db_error("journey_in_namespace", e),
    }

    match build_bindings(&state.db, journey_id).await {
        Ok(response) => {
            let mut resp = (StatusCode::OK, Json(response)).into_response();
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            resp
        }
        Err(e) => db_error("build_bindings", e),
    }
}

fn not_found(id: Uuid) -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "RESOURCE_NOT_FOUND",
            "message": format!("journey {} not found in caller's namespace", id),
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
