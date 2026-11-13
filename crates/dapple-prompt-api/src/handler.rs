//pub(crate) mod step;

pub(crate) mod admin;
pub(crate) mod api_tokens;
pub(crate) mod auth_token;
pub(crate) mod companies;
pub(crate) mod dispatch_phases;
pub(crate) mod dispatch_prompt_responses;
pub(crate) mod dispatch_task_attempts;
pub mod dispatches;
pub(crate) mod external_service_tokens;
pub(crate) mod service_token;

pub(crate) mod playbooks;

pub(crate) mod stages;

pub(crate) mod categories;
pub(crate) mod tasks;

pub(crate) mod user_details;

pub(crate) mod journeys;

pub(crate) mod hats;
pub(crate) mod journey_asset_uploads;
pub(crate) mod journey_bindings;
pub(crate) mod journey_brief_form;
pub(crate) mod journey_chains;
pub(crate) mod steps;

pub(crate) mod fluent_messages;
pub(crate) mod guide_embeddings;
pub mod guide_ingest;
pub(crate) mod guide_render;
pub(crate) mod guide_search;
pub(crate) mod guides;

pub(crate) mod assets;
pub(crate) mod brief_groups;
pub(crate) mod brief_packs;
pub(crate) mod brief_units;
pub(crate) mod briefs;
pub(crate) mod canonical;
pub(crate) mod deprecations;
pub(crate) mod directives;
pub(crate) mod dispatch_intent_bindings;
pub(crate) mod drafts;
pub(crate) mod inference_profiles;

pub(crate) mod prompts;
pub(crate) mod user_favorite_guides;
pub(crate) mod user_favorite_journey_chains;
pub(crate) mod value_slots;
pub(crate) mod versioned_objects;

use aide::axum::IntoApiResponse;
use aide::openapi::OpenApi;
use axum::http::StatusCode;
use axum::{Extension, Json};
use dapple_data::error::DataError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Serialize, JsonSchema)]
struct HealthResponse {
    status: String,
    message: String,
}

pub async fn health_checker_handler() -> impl IntoApiResponse {
    const MESSAGE: &str = "REST-ful interface to orchestrate dapple-prompt workflows";

    let json_response = HealthResponse {
        status: "success".into(),
        message: MESSAGE.into(),
    };

    Json(json_response)
}

pub async fn serve_api_json(Extension(api): Extension<Arc<OpenApi>>) -> impl IntoApiResponse {
    Json(api)
}

/// Emit a log line for a `DataError` returned by a handler. 5xx errors log at
/// `error`; client errors log at `warn` so that routine 4xx (e.g. 422
/// MissingIdLookup) are still visible during diagnosis without dominating
/// the log feed.
fn log_data_error(action: &str, obj_type: &str, status: StatusCode, e: &DataError) {
    if status.is_server_error() {
        log::error!("{} {}: {} ({})", action, obj_type, e, status);
    } else {
        log::warn!("{} {}: {} ({})", action, obj_type, e, status);
    }
}

/// Map a `DataError` variant to the appropriate HTTP status code.
///
/// **Note on `InvalidInput` returning 400 here vs 422 in guide ingest.**
/// CRUD endpoints reach this helper when the request itself is malformed
/// (e.g., `{ "objs": [] }` — an empty batch is a structurally non-meaningful
/// request) — that's 400 territory. The guide-ingest pipeline raises the
/// same `DataError::InvalidInput` for *content-level* problems (e.g., a
/// brief-input referencing a value-slot the parsed tarball doesn't declare)
/// where the request is well-formed but its contents fail semantic
/// validation — that's 422 territory, and `guide_ingest::data_error_to_status`
/// maps it accordingly. Two failure classes sharing one Rust variant; the
/// split is intentional. If you're adding a new endpoint that raises
/// `InvalidInput` for content-level reasons, prefer the ingest-side helper
/// (or write a dedicated one) rather than coercing the global mapping.
pub fn data_error_to_status_code(e: &DataError) -> StatusCode {
    match e {
        DataError::InsertArgColumnMismatch(_) | DataError::InvalidInput(_) => {
            StatusCode::BAD_REQUEST
        }
        DataError::MissingIdLookup(_)
        | DataError::AmbiguousIdLookup(_)
        | DataError::ResultSetProblems(_)
        | DataError::BlockedReference(_) => StatusCode::UNPROCESSABLE_ENTITY,
        DataError::SqlX(sqlx_err) => {
            let msg = sqlx_err.to_string();
            if msg.contains("duplicate key value violates unique constraint") {
                StatusCode::CONFLICT
            } else if matches!(sqlx_err, sqlx::Error::RowNotFound) {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
        DataError::ModelColumnOrderMismatch(_) | DataError::Custom(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

/// Machine-readable error code for a `DataError`, mirroring the HTTP-status
/// branches of [`data_error_to_status_code`]. Pairs with the unified error
/// envelope `{ "error": "<CODE>", "message": ... }` so consumers branch on a
/// stable code rather than parse the human-readable `message`. Codes match the
/// external surface's vocabulary (`DATABASE_ERROR`, `NOT_FOUND`, `DUPLICATE`, …).
pub fn data_error_to_code(e: &DataError) -> &'static str {
    match e {
        DataError::InsertArgColumnMismatch(_) | DataError::InvalidInput(_) => "INVALID_INPUT",
        DataError::MissingIdLookup(_) => "MISSING_REFERENCE",
        DataError::AmbiguousIdLookup(_) => "AMBIGUOUS_REFERENCE",
        DataError::ResultSetProblems(_) => "RESULT_SET_PROBLEM",
        DataError::BlockedReference(_) => "BLOCKED_REFERENCE",
        DataError::SqlX(sqlx_err) => {
            let msg = sqlx_err.to_string();
            if msg.contains("duplicate key value violates unique constraint") {
                "DUPLICATE"
            } else if matches!(sqlx_err, sqlx::Error::RowNotFound) {
                "NOT_FOUND"
            } else {
                "DATABASE_ERROR"
            }
        }
        DataError::ModelColumnOrderMismatch(_) | DataError::Custom(_) => "DATABASE_ERROR",
    }
}

/// Coarse machine-readable code derived from an HTTP status, for error sites
/// that don't carry a `DataError` (hand-rolled validation failures, pipeline
/// errors). Prefer a specific code at the call site where one is meaningful;
/// this is the fallback for helpers keyed only on the status.
pub fn status_to_error_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "BAD_REQUEST",
        StatusCode::UNAUTHORIZED => "UNAUTHORIZED",
        StatusCode::FORBIDDEN => "FORBIDDEN",
        StatusCode::NOT_FOUND => "NOT_FOUND",
        StatusCode::CONFLICT => "CONFLICT",
        StatusCode::UNPROCESSABLE_ENTITY => "UNPROCESSABLE_ENTITY",
        StatusCode::SERVICE_UNAVAILABLE => "SERVICE_UNAVAILABLE",
        s if s.is_server_error() => "INTERNAL_ERROR",
        _ => "ERROR",
    }
}

#[derive(Clone, Eq, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct GetResponse<O> {
    pub status: String,
    pub results: usize,
    pub obj: O,
}

#[derive(Serialize, JsonSchema)]
pub struct GetErrorResponse {
    pub error: String,
    pub message: String,
}

pub fn get_response_handler<O>(
    obj_type: &str,
    res: Result<O, DataError>,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)>
where
    O: Serialize,
{
    match res {
        Ok(obj) => {
            let response = json!(GetResponse {
                status: "success".into(),
                results: 1,
                obj
            });

            Ok(Json(response))
        }
        Err(e) => {
            let status = data_error_to_status_code(&e);
            log_data_error("get", obj_type, status, &e);
            let error_response = json!(GetErrorResponse {
                error: data_error_to_code(&e).to_string(),
                message: e.to_string(),
            });
            Err((status, Json(error_response)))
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct IndexResponse<O> {
    pub status: String,
    pub results: usize,
    pub objs: Vec<O>,
}

#[derive(Serialize, JsonSchema)]
pub struct IndexErrorResponse {
    pub error: String,
    pub message: String,
}

pub fn index_response_handler<O>(
    obj_type: &str,
    res: Result<Vec<O>, DataError>,
) -> impl IntoApiResponse
where
    O: Serialize,
{
    match res {
        Ok(objs) => {
            let json_response = json!(IndexResponse {
                status: "success".into(),
                results: objs.len(),
                objs
            });
            Ok(Json(json_response))
        }
        Err(e) => {
            let status = data_error_to_status_code(&e);
            log_data_error("index", obj_type, status, &e);
            let error_response = json!(IndexErrorResponse {
                error: data_error_to_code(&e).to_string(),
                message: format!("Error fetching project items: {}", e),
            });
            Err((status, Json(error_response)))
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PostResponse<O> {
    pub status: String,
    pub results: usize,
    pub objs: Vec<O>,
}

#[derive(Serialize, JsonSchema)]
pub struct PostErrorResponse {
    pub error: String,
    pub message: String,
}

pub fn post_response_handler<O>(
    obj_type: &str,
    res: Result<Vec<O>, DataError>,
) -> impl IntoApiResponse
where
    O: Serialize,
{
    match res {
        Ok(objs) => {
            let response = json!(PostResponse {
                status: "success".into(),
                results: objs.len(),
                objs
            });

            Ok((StatusCode::CREATED, Json(response)))
        }
        Err(e) => {
            let status = data_error_to_status_code(&e);
            log_data_error("post", obj_type, status, &e);
            let error_response = json!(PostErrorResponse {
                error: data_error_to_code(&e).to_string(),
                message: e.to_string(),
            });
            Err((status, Json(error_response)))
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteResponse<O> {
    pub status: String,
    pub results: usize,
    pub objs: Vec<O>,
}

#[derive(Serialize, JsonSchema)]
pub struct DeleteErrorResponse {
    pub error: String,
    pub message: String,
}

pub fn delete_response_handler<O>(
    obj_type: &str,
    res: Result<Vec<O>, DataError>,
) -> impl IntoApiResponse
where
    O: Serialize,
{
    match res {
        Ok(objs) => {
            let response = json!(DeleteResponse {
                status: "success".into(),
                results: objs.len(),
                objs
            });

            Ok((StatusCode::OK, Json(response)))
        }
        Err(e) => {
            let status = data_error_to_status_code(&e);
            log_data_error("delete", obj_type, status, &e);
            let error_response = json!(DeleteErrorResponse {
                error: data_error_to_code(&e).to_string(),
                message: e.to_string(),
            });
            Err((status, Json(error_response)))
        }
    }
}
