//! `GET /api/journeys/{id}/brief-form` on the external surface.
//!
//! Structural form definition for a journey, namespace-gated via
//! `journey_in_namespace` (foreign or unknown journey both return 404
//! `RESOURCE_NOT_FOUND` — no cross-namespace existence leak).
//!
//! After the gate, delegates to `dapple_data::view::journey_brief_form::build_brief_form`
//! — the same builder the internal surface uses. Sets `ETag` and
//! `Cache-Control: private, max-age=0, must-revalidate`; honors `If-None-Match`
//! → 304 on hash match.

use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use dapple_data::query::journeys::journey_in_namespace;
use dapple_data::view::journey_brief_form::build_brief_form;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn journey_brief_form_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(journey_id): Path<Uuid>,
    headers: HeaderMap,
) -> impl IntoApiResponse {
    // Namespace gate. 404 (not 403) so foreign-journey probes can't
    // distinguish "exists but not yours" from "doesn't exist."
    match journey_in_namespace(&state.db, journey_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found(journey_id),
        Err(e) => return db_error("journey_in_namespace", e),
    }

    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    match build_brief_form(&state.db, journey_id).await {
        Ok((response, etag)) => {
            let etag_value = format!("\"{}\"", etag);
            if if_none_match
                .as_deref()
                .map(|v| v.trim() == etag_value)
                .unwrap_or(false)
            {
                let mut resp = StatusCode::NOT_MODIFIED.into_response();
                set_cache_headers(&mut resp, &etag_value);
                return resp;
            }
            let mut resp = (StatusCode::OK, Json(response)).into_response();
            set_cache_headers(&mut resp, &etag_value);
            resp
        }
        Err(e) => db_error("build_brief_form", e),
    }
}

const CACHE_CONTROL_VALUE: &str = "private, max-age=0, must-revalidate";

fn set_cache_headers(resp: &mut axum::response::Response, etag_value: &str) {
    if let Ok(hv) = HeaderValue::from_str(etag_value) {
        resp.headers_mut().insert(header::ETAG, hv);
    }
    if let Ok(hv) = HeaderValue::from_str(CACHE_CONTROL_VALUE) {
        resp.headers_mut().insert(header::CACHE_CONTROL, hv);
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
