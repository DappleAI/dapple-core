//! `GET /api/journeys/{id}/brief-form` — structural form definition for a
//! journey. Cacheable via strong ETag derived from brief content_hashes.
//!
//! Pipeline (delegates to `dapple_data::view::journey_brief_form`):
//!   1. `select_briefs_by_journey_object_id` → brief_ids + content_hashes.
//!   2. Compute ETag (raw SHA-1 hex of sorted content_hashes joined by NUL).
//!   3. `select_brief_units_for_briefs` → units.
//!   4. `select_brief_unit_input_choices_by_unit_ids` → choices.
//!   5. Fold into `JourneyBriefFormResponse`.
//!   6. Wrap with `ETag` + `Cache-Control: private, max-age=0, must-revalidate`.
//!
//! See `crates/dapple-types/src/model/journey_brief_form.rs` for the DTO
//! shape and the architectural rationale (slot metadata stays separately
//! batch-fetchable; bindings live on a sibling endpoint).

use crate::AppState;
use crate::extract_path_uuid::ValidatedPathUuid;
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use dapple_data::view::journey_brief_form::build_brief_form;
use serde_json::json;
use std::sync::Arc;

pub async fn journey_brief_form_handler(
    ValidatedPathUuid(journey_id): ValidatedPathUuid,
    headers: HeaderMap,
    State(data): State<Arc<AppState>>,
    _user: crate::RegisteredUser,
) -> impl IntoApiResponse {
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    render_brief_form(&data.db, journey_id, if_none_match.as_deref()).await
}

/// Shared response framing: load + ETag-check + Cache-Control. Pulled out
/// of the handler proper so the external surface can reuse it after its
/// namespace gate.
pub async fn render_brief_form(
    db: &dapple_data::DbPool,
    journey_id: uuid::Uuid,
    if_none_match: Option<&str>,
) -> axum::response::Response {
    match build_brief_form(db, journey_id).await {
        Ok((response, etag)) => {
            let etag_value = quoted_etag(&etag);
            // 304 path: keep ETag + Cache-Control on the response so clients
            // don't lose their cache key.
            if if_none_match
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
        Err(e) => {
            log::error!("journey_brief_form: data error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "DATABASE_ERROR",
                    "message": "failed to load journey brief-form",
                })),
            )
                .into_response()
        }
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

/// Wrap the raw hex hash in double quotes per RFC 7232 strong-validator
/// syntax. Clients echo this exact value back via `If-None-Match`.
fn quoted_etag(raw: &str) -> String {
    format!("\"{}\"", raw)
}
