//! `/api/directives` on the external surface.

use crate::schema::directives::ListExternalDirectivesQuery;
use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::FilterOptions;
use dapple_data::query::directives::{
    directive_in_namespace, select_directive_by_object_id, select_directives_by_external_caller,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn directives_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Query(params): Query<ListExternalDirectivesQuery>,
) -> impl IntoApiResponse {
    let filter_ops = FilterOptions {
        page: params.page.map(|p| p as usize),
        limit: params.limit.map(|l| l as usize),
    };

    match select_directives_by_external_caller(&state.db, caller.company_id, filter_ops).await {
        Ok(objs) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": objs.len(),
                "objs": objs,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_directives_by_external_caller", e),
    }
}

pub async fn directives_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    if !match directive_in_namespace(&state.db, id, caller.company_id).await {
        Ok(b) => b,
        Err(e) => return db_error("directive_in_namespace", e),
    } {
        return not_found("directive", id);
    }

    match select_directive_by_object_id(&state.db, id).await {
        Ok(obj) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": 1,
                "obj": obj,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_directive_by_object_id", e),
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
