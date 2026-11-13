//! `/api/stages` on the external surface.
//!
//! Stages are shared/reusable across playbooks, but every stage carries a
//! `namespace_company_id` on its path. Both GET handlers bind
//! `caller.company_id` so customers only see stages whose path lives in
//! their namespace.

use crate::schema::stages::ListExternalStagesQuery;
use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::FilterOptions;
use dapple_data::query::stages::{
    select_stages_by_external_caller, select_stages_by_object_ids, stage_in_namespace,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn stages_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Query(params): Query<ListExternalStagesQuery>,
) -> impl IntoApiResponse {
    let filter_ops = FilterOptions {
        page: params.page.map(|p| p as usize),
        limit: params.limit.map(|l| l as usize),
    };

    match select_stages_by_external_caller(&state.db, caller.company_id, filter_ops).await {
        Ok(stages) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": stages.len(),
                "objs": stages,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_stages_by_external_caller", e),
    }
}

pub async fn stages_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    if !match stage_in_namespace(&state.db, id, caller.company_id).await {
        Ok(b) => b,
        Err(e) => return db_error("stage_in_namespace", e),
    } {
        return not_found("stage", id);
    }

    match select_stages_by_object_ids(&state.db, vec![id], FilterOptions::default()).await {
        Ok(stages) => match stages.into_iter().next() {
            Some(stage) => (
                StatusCode::OK,
                Json(json!({
                    "status": "ok",
                    "results": 1,
                    "obj": stage,
                })),
            )
                .into_response(),
            None => not_found("stage", id),
        },
        Err(e) => db_error("select_stages_by_object_ids", e),
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
