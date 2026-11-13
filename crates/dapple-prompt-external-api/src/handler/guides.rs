//! `GET /api/guides` on the external surface — proves the SQL filter pattern
//! end-to-end. Lists guides whose path lives in the caller's company namespace
//! and whose scope is `'external'`.
//!
//! There is no code path here that selects guides without binding the
//! caller's `company_id` — that's the structural-isolation property the
//! external API is built around.

use crate::schema::guides::ListExternalGuidesQuery;
use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::FilterOptions;
use dapple_data::query::guides::select_guides_by_external_caller;
use serde_json::json;
use std::sync::Arc;

pub async fn list_guides_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Query(params): Query<ListExternalGuidesQuery>,
) -> impl IntoApiResponse {
    let filter_ops = FilterOptions {
        page: params.page.map(|p| p as usize),
        limit: params.limit.map(|l| l as usize),
    };

    log::debug!(
        "list_guides company_slug={} company_id={} limit={:?} page={:?}",
        caller.company_slug,
        caller.company_id,
        params.limit,
        params.page
    );

    match select_guides_by_external_caller(&state.db, caller.company_id, filter_ops).await {
        Ok(guides) => {
            let body = json!({
                "status": "ok",
                "results": guides.len(),
                "objs": guides,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => {
            log::error!(
                "list_guides db error for company={}: {}",
                caller.company_slug,
                e
            );
            let body = json!({
                "status": "error",
                "error": "DATABASE_ERROR",
                "message": "failed to list guides",
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
        }
    }
}
