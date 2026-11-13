//! `/api/tasks` on the external surface.
//!
//! Tasks are shared/reusable across steps, but every task carries a
//! `namespace_company_id` on its path. Both GET handlers bind
//! `caller.company_id` so customers only see tasks whose path lives in
//! their namespace.

use crate::schema::tasks::ListExternalTasksQuery;
use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::FilterOptions;
use dapple_data::query::tasks::{
    select_task_by_universal_id, select_tasks_by_external_caller, task_in_namespace,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn tasks_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Query(params): Query<ListExternalTasksQuery>,
) -> impl IntoApiResponse {
    let filter_ops = FilterOptions {
        page: params.page.map(|p| p as usize),
        limit: params.limit.map(|l| l as usize),
    };

    match select_tasks_by_external_caller(&state.db, caller.company_id, filter_ops).await {
        Ok(tasks) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": tasks.len(),
                "objs": tasks,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_tasks_by_external_caller", e),
    }
}

pub async fn tasks_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    if !match task_in_namespace(&state.db, id, caller.company_id).await {
        Ok(b) => b,
        Err(e) => return db_error("task_in_namespace", e),
    } {
        return not_found("task", id);
    }

    match select_task_by_universal_id(&state.db, id).await {
        Ok(task) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": 1,
                "obj": task,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_task_by_universal_id", e),
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
