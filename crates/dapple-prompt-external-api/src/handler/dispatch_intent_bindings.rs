//! `POST /api/dispatch-intent-bindings` on the external surface.
//!
//! Namespace-gated: every `dispatch_id` in the batch must belong to the
//! caller's company. We bail on the first miss with 404 (RESOURCE_NOT_FOUND)
//! so a foreign dispatch UUID looks identical to a nonexistent one — no
//! cross-namespace existence leak.
//!
//! Once bindings are inserted, we re-check each touched dispatch's brief
//! state and auto-spawn execution in-process for any dispatch that's now
//! fully bound. Mirrors the internal handler's trigger logic.

use crate::schema::dispatch_intent_bindings::CreateDispatchIntentBindingSetSchema;
use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::error::DataError;
use dapple_data::model::dispatch_intent_bindings::InsertDispatchIntentBindingArgs;
use dapple_data::model::dispatch_intent_bindings::binding_items::{
    InsertBindingAssetItemArgs, InsertBindingBoolItemArgs, InsertBindingDatasetItemArgs,
    InsertBindingIntItemArgs, InsertBindingJsonItemArgs, InsertBindingNumberItemArgs,
    InsertBindingTextItemArgs, InsertBindingValueMetaArgs,
};
use dapple_data::query::asset_dataset_extractions::resolve_bindable_dataset_extraction_id;
use dapple_data::query::dispatch_intent_bindings::{
    insert_dispatch_intent_bindings, validate_binding_cardinality,
};
use dapple_data::query::dispatches::{
    dispatch_in_namespace, select_dispatch_model_by_universal_id,
};
use dapple_data::query::journeys::dispatch_ready;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// `POST /api/dispatch-intent-bindings` — namespace-gated bindings create.
pub async fn dispatch_intent_bindings_post_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Json(body): Json<CreateDispatchIntentBindingSetSchema>,
) -> impl IntoApiResponse {
    if body.objs.is_empty() {
        return bad_request("EMPTY_BATCH", "objs must be non-empty");
    }

    // Namespace validation: every dispatch_id in the batch must belong to
    // the caller's company. The unique-set of IDs keeps the check linear in
    // the number of dispatches touched, not the binding count.
    let mut unique_dispatch_ids: Vec<Uuid> = body.objs.iter().map(|o| o.dispatch_id).collect();
    unique_dispatch_ids.sort();
    unique_dispatch_ids.dedup();

    for dispatch_id in &unique_dispatch_ids {
        match dispatch_in_namespace(&state.db, *dispatch_id, caller.company_id).await {
            Ok(true) => (),
            Ok(false) => return not_found("dispatch", *dispatch_id),
            Err(e) => return db_error("dispatch_in_namespace", e),
        }
    }

    // Build InsertDispatchIntentBindingArgs (mirrors the internal handler).
    let dispatch_ids: Vec<Uuid> = body.objs.iter().map(|o| o.dispatch_id).collect();
    let value_slot_ids: Vec<Uuid> = body.objs.iter().map(|o| o.value_slot_id).collect();

    let mut text_items = Vec::new();
    let mut int_items = Vec::new();
    let mut number_items = Vec::new();
    let mut bool_items = Vec::new();
    let mut json_items = Vec::new();
    let mut asset_items = Vec::new();
    let mut dataset_items = Vec::new();
    let mut value_meta = Vec::new();

    for obj in body.objs.iter() {
        text_items.push(InsertBindingTextItemArgs {
            values: obj.text_items.iter().map(|i| i.value.clone()).collect(),
        });
        int_items.push(InsertBindingIntItemArgs {
            values: obj.int_items.iter().map(|i| i.value).collect(),
        });
        number_items.push(InsertBindingNumberItemArgs {
            values: obj.number_items.iter().map(|i| i.value).collect(),
        });
        bool_items.push(InsertBindingBoolItemArgs {
            values: obj.bool_items.iter().map(|i| i.value).collect(),
        });
        json_items.push(InsertBindingJsonItemArgs {
            values: obj.json_items.iter().map(|i| i.value.clone()).collect(),
        });
        asset_items.push(InsertBindingAssetItemArgs {
            asset_ids: obj.asset_items.iter().map(|i| i.asset_id).collect(),
        });
        // Resolve each dataset item's uploaded asset_id to its current ok
        // extraction row and PIN it by id. Not-yet-ok → clear 400. Namespace is
        // already enforced above via `dispatch_in_namespace` (the asset reaches us
        // only through a dispatch the caller owns).
        let mut extraction_ids = Vec::with_capacity(obj.dataset_items.len());
        for item in obj.dataset_items.iter() {
            match resolve_bindable_dataset_extraction_id(&state.db, item.asset_id).await {
                Ok(id) => extraction_ids.push(id),
                Err(e) => return db_error("resolve_bindable_dataset_extraction_id", e),
            }
        }
        dataset_items.push(InsertBindingDatasetItemArgs {
            asset_dataset_extraction_ids: extraction_ids,
        });
        value_meta.push(InsertBindingValueMetaArgs {
            ui_specs: obj.value_meta.iter().map(|m| m.ui_spec.clone()).collect(),
            producers: obj.value_meta.iter().map(|m| m.producer.clone()).collect(),
        });
    }

    let args = InsertDispatchIntentBindingArgs {
        dispatch_ids,
        value_slot_ids,
        // Operator write: never "produced empty" — the binding carries exactly
        // what was submitted. An empty vec pads to all-false.
        produced_empty: Vec::new(),
        text_items,
        int_items,
        number_items,
        bool_items,
        json_items,
        asset_items,
        dataset_items,
        value_meta,
    };

    // Cardinality guard (same rule as the internal surface): single → exactly
    // one item, multi → min..max. Returns 400 on violation.
    let submissions: Vec<(Uuid, usize)> = body
        .objs
        .iter()
        .map(|o| {
            let count = o.text_items.len()
                + o.int_items.len()
                + o.number_items.len()
                + o.bool_items.len()
                + o.json_items.len()
                + o.asset_items.len()
                + o.dataset_items.len();
            (o.value_slot_id, count)
        })
        .collect();
    if let Err(e) = validate_binding_cardinality(&state.db, &submissions).await {
        return db_error("validate_binding_cardinality", e);
    }

    let bindings = match insert_dispatch_intent_bindings(&state.db, args).await {
        Ok(b) => b,
        Err(e) => return db_error("insert_dispatch_intent_bindings", e),
    };

    // After inserting, re-check each touched dispatch. Any dispatch whose
    // journey is now fully bound gets queued for in-process execution. The
    // external surface always runs in-process (no flow-url branch).
    for dispatch_id in unique_dispatch_ids {
        let model = match select_dispatch_model_by_universal_id(&state.db, dispatch_id).await {
            Ok(m) => m,
            Err(e) => {
                log::error!(
                    "external bindings: failed to load dispatch {} for execution: {}",
                    dispatch_id,
                    e
                );
                continue;
            }
        };

        let ready = dispatch_ready(&state.db, &model).await.unwrap_or(false);
        if !ready {
            continue;
        }

        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = dapple_prompt_flow::dispatch_runner::execute_dispatch_and_retrigger(
                state.db.clone(),
                model,
                4,
            )
            .await
            {
                log::error!(
                    "external dispatch execution failed for {}: {}",
                    dispatch_id,
                    e
                );
            }
        });
    }

    (
        StatusCode::CREATED,
        Json(json!({
            "status": "ok",
            "results": bindings.len(),
            "objs": bindings,
        })),
    )
        .into_response()
}

// ---------- error helpers ----------

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

fn bad_request(code: &str, msg: &str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": code, "message": msg })),
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
