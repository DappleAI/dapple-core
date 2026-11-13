//! `GET /api/journeys/{id}/steps` on the external surface.
//!
//! Per-step exposed-slot status for a journey: one entry per playbook step,
//! with `done = true` when every value slot the step exposes (its overlays,
//! cascade-resolved) is bound somewhere in the journey's chain, plus
//! `exposed_slots` carrying each slot's bind target, kind, and bound flag.
//! Finer-grained than `/stages` — lets a client discover which step (e.g. a
//! generate→pick→refine pick step) is awaiting a binding and which slot to
//! bind. Namespace-gated by `caller.company_id`; backed by the overlay-driven
//! `select_step_binding_status_for_journey` (see dapple-data).

use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::query::journeys::{journey_in_namespace, select_step_binding_status_for_journey};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn journey_steps_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(journey_id): Path<Uuid>,
) -> impl IntoApiResponse {
    match journey_in_namespace(&state.db, journey_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found("journey", journey_id),
        Err(e) => return db_error("journey_in_namespace", e),
    }

    // Reconcile-on-poll: see `journey_stages_index_handler`. The UI polls both
    // /stages and /steps, so either endpoint re-drives a stalled journey. Spawned
    // after the namespace check so a foreign/unknown journey is never touched.
    tokio::spawn(
        dapple_prompt_flow::dispatch_runner::reconcile_journey_dispatches(
            state.db.clone(),
            journey_id,
            4,
        ),
    );

    match select_step_binding_status_for_journey(&state.db, journey_id).await {
        Ok(steps) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": steps.len(),
                "objs": steps,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_step_binding_status_for_journey", e),
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
