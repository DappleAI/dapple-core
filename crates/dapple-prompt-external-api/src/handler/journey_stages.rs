//! `GET /api/journeys/{id}/stages` on the external surface.
//!
//! Returns per-stage completion for a journey: one entry per playbook stage,
//! with `done = true` when every value slot the stage needs (its overlay-exposed
//! slots) is bound somewhere in the journey's chain. Namespace-gated by
//! `caller.company_id`; the completion check itself is the overlay-driven
//! `select_stage_binding_status_for_journey` (see dapple-data).

use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::query::journeys::{journey_in_namespace, select_stage_binding_status_for_journey};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn journey_stages_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(journey_id): Path<Uuid>,
) -> impl IntoApiResponse {
    match journey_in_namespace(&state.db, journey_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found("journey", journey_id),
        Err(e) => return db_error("journey_in_namespace", e),
    }

    // Reconcile-on-poll: best-effort re-drive any stalled auto-dispatch cascade for
    // this journey (recovers a journey whose in-process retrigger future was lost —
    // e.g. a pod restart during a long task). Spawned so it never blocks the poll.
    // Only after the namespace check passes, so a foreign/unknown journey is never
    // touched. External V1 always runs execution in-process (no flow-url branch).
    tokio::spawn(
        dapple_prompt_flow::dispatch_runner::reconcile_journey_dispatches(
            state.db.clone(),
            journey_id,
            4,
        ),
    );

    match select_stage_binding_status_for_journey(&state.db, journey_id).await {
        Ok(stages) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": stages.len(),
                "objs": stages,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_stage_binding_status_for_journey", e),
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
