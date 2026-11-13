//! `GET /api/dispatches/{id}/task_attempts` on the external surface.
//!
//! Per-task runtime phase (Running / Failed / Ok) for the runner panel.
//! Namespace-gated via `dispatch_in_namespace`; a foreign-namespace dispatch
//! UUID and an unknown dispatch UUID both return the same 404 body — no
//! cross-namespace existence leak.
//!
//! Projected DTO: each internal `DispatchTaskAttempt` is mapped to
//! [`ExternalDispatchTaskAttempt`], which exposes the author-emitted `signals`
//! channel (`context_metadata.signals`) in place of the raw `context_metadata`
//! grab-bag. The grab-bag also carries dapple-side identifiers on internal
//! `flow_directive` audit events, so it must never reach a third-party
//! operator; the projection keeps it off the wire structurally. `error_detail`
//! / `context_data` remain operator-facing per the External-scope authoring
//! contract in `AGENTS.md`.

use crate::schema::dispatch_task_attempts::ExternalDispatchTaskAttempt;
use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::error::DataError;
use dapple_data::query::dispatch_task_attempts::select_task_attempts_by_dispatch_id;
use dapple_data::query::dispatches::dispatch_in_namespace;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// `GET /api/dispatches/{id}/task_attempts` — per-task phase list, namespace-gated.
pub async fn dispatch_task_attempts_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(dispatch_id): Path<Uuid>,
) -> impl IntoApiResponse {
    // Namespace gate. Identical 404 body for foreign-namespace and unknown-id
    // — no cross-namespace existence leak.
    match dispatch_in_namespace(&state.db, dispatch_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found("dispatch", dispatch_id),
        Err(e) => return db_error("dispatch_in_namespace", e),
    }

    match select_task_attempts_by_dispatch_id(&state.db, dispatch_id).await {
        Ok(attempts) => {
            // Project each attempt to the operator-facing view: expose only the
            // author-emitted `signals` channel, never the internal
            // `context_metadata` grab-bag.
            let objs: Vec<ExternalDispatchTaskAttempt> =
                attempts.into_iter().map(Into::into).collect();
            (
                StatusCode::OK,
                Json(json!({
                    "status": "ok",
                    "results": objs.len(),
                    "objs": objs,
                })),
            )
                .into_response()
        }
        Err(e) => db_error("select_task_attempts_by_dispatch_id", e),
    }
}

// ---------- error helpers (mirrors handler/dispatches.rs) ----------

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

fn db_error(op: &str, e: DataError) -> axum::response::Response {
    log::error!("{}: {}", op, e);
    let status = match &e {
        DataError::InvalidInput(_) | DataError::InsertArgColumnMismatch(_) => {
            StatusCode::BAD_REQUEST
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(json!({ "error": "REQUEST_FAILED", "message": format!("{}: {}", op, e) })),
    )
        .into_response()
}
