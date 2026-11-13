//! `/api/playbooks` on the external surface.
//!
//! `GET /api/playbooks` — list playbooks in the caller's namespace.
//! `GET /api/playbooks/{id}` — fetch a single playbook (full nested
//! stages/steps/tasks) in the caller's namespace.
//!
//! Every query binds `caller.company_id`; a foreign UUID or unknown UUID
//! both return the same 404 RESOURCE_NOT_FOUND body (no existence leak).

use crate::schema::playbooks::ListExternalPlaybooksQuery;
use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::FilterOptions;
use dapple_data::query::playbooks::{
    playbook_in_namespace, select_playbook_by_universal_id, select_playbooks_by_external_caller,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn playbooks_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Query(params): Query<ListExternalPlaybooksQuery>,
) -> impl IntoApiResponse {
    let filter_ops = FilterOptions {
        page: params.page.map(|p| p as usize),
        limit: params.limit.map(|l| l as usize),
    };

    match select_playbooks_by_external_caller(&state.db, caller.company_id, filter_ops).await {
        Ok(playbooks) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": playbooks.len(),
                "objs": playbooks,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_playbooks_by_external_caller", e),
    }
}

pub async fn playbooks_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    if !match playbook_in_namespace(&state.db, id, caller.company_id).await {
        Ok(b) => b,
        Err(e) => return db_error("playbook_in_namespace", e),
    } {
        return not_found("playbook", id);
    }

    match select_playbook_by_universal_id(&state.db, id).await {
        Ok(playbook) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": 1,
                "obj": playbook,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_playbook_by_universal_id", e),
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
        Json(json!({
            "error": "DATABASE_ERROR",
            "message": format!("{} failed", op),
        })),
    )
        .into_response()
}
