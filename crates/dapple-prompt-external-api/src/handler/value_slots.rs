//! `/api/value-slots` on the external surface.

use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::FilterOptions;
use dapple_data::query::value_slots::{
    select_value_slot_by_object_id, select_value_slots_by_external_caller,
    select_value_slots_by_external_caller_and_object_ids, value_slot_in_namespace,
};
use dapple_prompt_api::extract_qs::QsQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// Raw query params. We accept either:
///   * `?query=by_object_ids&ids=<uuid>&ids=<uuid>...` — batch metadata
///     fetch (also accepts the bracket-notation array form
///     `?query=by_object_ids&ids[0]=<uuid>&ids[1]=<uuid>...`).
///   * bare `?page=&limit=` — list everything in the caller's namespace.
///
/// The two are mutually exclusive; presence of `query` switches modes.
/// The `by_object_ids` variant name matches the internal surface's
/// `GetValueSlotSetSchema::ByObjectIds`, and deserialization uses the
/// same `QsQuery` extractor as the internal surface, so the wire
/// vocabulary is identical across both surfaces.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct ExternalValueSlotsQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Vec<Uuid>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub page: Option<u32>,
}

pub async fn value_slots_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    QsQuery(params): QsQuery<ExternalValueSlotsQueryParams>,
) -> impl IntoApiResponse {
    match params.query.as_deref() {
        Some("by_object_ids") => {
            // Batch fetch: foreign IDs are silently dropped by the SQL, so
            // a third party guessing a UUID from another namespace gets no
            // existence signal back.
            match select_value_slots_by_external_caller_and_object_ids(
                &state.db,
                caller.company_id,
                params.ids,
            )
            .await
            {
                Ok(objs) => (
                    StatusCode::OK,
                    Json(json!({
                        "status": "ok",
                        "results": objs.len(),
                        "objs": objs,
                    })),
                )
                    .into_response(),
                Err(e) => db_error("select_value_slots_by_external_caller_and_object_ids", e),
            }
        }
        Some(other) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "UNKNOWN_QUERY",
                "message": format!("unknown query variant: {}", other),
            })),
        )
            .into_response(),
        None => {
            // Default: paged list scoped to caller's namespace. Matches the
            // documented `ListExternalValueSlotsQuery` schema (which is the
            // OpenAPI-documented shape for this endpoint when no `query=`
            // discriminator is present).
            let filter_ops = FilterOptions {
                page: params.page.map(|p| p as usize),
                limit: params.limit.map(|l| l as usize),
            };
            match select_value_slots_by_external_caller(&state.db, caller.company_id, filter_ops)
                .await
            {
                Ok(objs) => (
                    StatusCode::OK,
                    Json(json!({
                        "status": "ok",
                        "results": objs.len(),
                        "objs": objs,
                    })),
                )
                    .into_response(),
                Err(e) => db_error("select_value_slots_by_external_caller", e),
            }
        }
    }
}

pub async fn value_slots_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    if !match value_slot_in_namespace(&state.db, id, caller.company_id).await {
        Ok(b) => b,
        Err(e) => return db_error("value_slot_in_namespace", e),
    } {
        return not_found("value_slot", id);
    }

    match select_value_slot_by_object_id(&state.db, id).await {
        Ok(obj) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": 1,
                "obj": obj,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_value_slot_by_object_id", e),
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
