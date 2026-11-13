//! `GET /api/journeys/{id}/bindings` — current binding state for a journey.
//!
//! Cheap to call, polled frequently. `Cache-Control: no-store`.
//!
//! Each row is the latest binding per `value_slot_id` across all dispatches
//! belonging to the journey, with `bound_at` for client-side staleness
//! detection.

use crate::AppState;
use crate::extract_path_uuid::ValidatedPathUuid;
use crate::handler::index_response_handler;
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use dapple_data::query::dispatch_intent_bindings::select_dispatch_intent_bindings_by_journey_id;
use dapple_data::view::journey_bindings::build_bindings;
use serde_json::json;
use std::sync::Arc;

pub async fn journey_bindings_handler(
    ValidatedPathUuid(journey_id): ValidatedPathUuid,
    State(data): State<Arc<AppState>>,
    _user: crate::RegisteredUser,
) -> impl IntoApiResponse {
    render_bindings(&data.db, journey_id).await
}

/// `GET /api/journeys/{id}/binding-history` — every binding ever attached to
/// any dispatch of this journey, ordered by `created_at ASC`.
///
/// Sibling of `journey_bindings_handler` (latest-per-slot). Consumed by
/// `dapple-ui`'s history / timeline-replay route, which partitions by
/// `dispatch_id` on each row to reconstruct the per-step answer trail.
pub async fn journey_binding_history_handler(
    ValidatedPathUuid(journey_id): ValidatedPathUuid,
    State(data): State<Arc<AppState>>,
    _user: crate::RegisteredUser,
) -> impl IntoApiResponse {
    index_response_handler(
        "dispatch_intent_bindings",
        select_dispatch_intent_bindings_by_journey_id(&data.db, journey_id).await,
    )
}

/// Shared response framing — used by both the internal handler above and
/// the external surface (after its namespace gate).
pub async fn render_bindings(
    db: &dapple_data::DbPool,
    journey_id: uuid::Uuid,
) -> axum::response::Response {
    match build_bindings(db, journey_id).await {
        Ok(response) => {
            let mut resp = (StatusCode::OK, Json(response)).into_response();
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            resp
        }
        Err(e) => {
            log::error!("journey_bindings: data error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "DATABASE_ERROR",
                    "message": "failed to load journey bindings",
                })),
            )
                .into_response()
        }
    }
}
