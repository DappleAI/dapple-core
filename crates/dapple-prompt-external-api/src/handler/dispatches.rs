//! Dispatch endpoints on the external surface.
//!
//! - `POST /api/dispatches` — create a dispatch on a journey owned by the caller
//! - `GET /api/dispatches/{id}` — poll status (returns stages/steps/tasks)
//! - `GET /api/dispatches/{id}/task_attempts` — per-dispatch task-attempt list
//!
//! Mirrors the internal `dapple-prompt-api::handler::dispatches` flow but
//! every resource access is gated by `journey_in_namespace` /
//! `dispatch_in_namespace` first. The customer cannot create a dispatch on
//! another company's journey or poll another company's dispatch.

use crate::schema::dispatches::{CreateDispatchSetSchema, CreateDispatchStepSchema};
use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::error::DataError;
use dapple_data::error::InsertArgColumnMismatch;
use dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
use dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs;
use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
use dapple_data::model::dispatches::{Dispatch, InsertDispatchArgs};
use dapple_data::query::dispatch_task_attempts::select_task_attempts_by_dispatch_id;
use dapple_data::query::dispatches::{
    dispatch_in_namespace, insert_dispatches, resolve_playbook_stages_for_steps,
    select_dispatch_by_universal_id,
};
use dapple_data::query::journeys::{dispatch_ready, journey_in_namespace};
use indexmap::IndexMap;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// `POST /api/dispatches` — namespace-gated dispatch create.
pub async fn dispatches_post_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Json(body): Json<CreateDispatchSetSchema>,
) -> impl IntoApiResponse {
    if body.objs.is_empty() {
        return bad_request("EMPTY_BATCH", "objs must be non-empty");
    }

    // Namespace validation: every journey_id in the batch must belong to the
    // caller's company. We bail on the first miss with 404.
    for obj in body.objs.iter() {
        match journey_in_namespace(&state.db, obj.journey_id, caller.company_id).await {
            Ok(true) => (),
            Ok(false) => return not_found("journey", obj.journey_id),
            Err(e) => return db_error("journey_in_namespace", e),
        }
    }

    // Step → stage resolution + InsertDispatchArgs build (mirrors internal
    // logic; deliberately duplicated for now to keep the surfaces decoupled —
    // future refactor: extract `prepare_dispatch_args` into `dapple-data`).
    let result: Result<_, DataError> = async {
        let step_oids: Vec<Uuid> = body
            .objs
            .iter()
            .flat_map(|d| d.steps.iter().map(|s| s.stage_step_id))
            .collect();

        let resolutions = resolve_playbook_stages_for_steps(&state.db, &step_oids).await?;
        let step_to_stage: HashMap<Uuid, Uuid> = resolutions
            .iter()
            .map(|r| (r.stage_step_id, r.playbook_stage_id))
            .collect();

        let mut journey_ids = Vec::new();
        let mut objs = Vec::new();

        for d in body.objs.iter() {
            journey_ids.push(d.journey_id);

            let mut stage_groups: IndexMap<Uuid, Vec<&CreateDispatchStepSchema>> = IndexMap::new();
            for step in &d.steps {
                let stage_oid = step_to_stage.get(&step.stage_step_id).ok_or_else(|| {
                    DataError::from(InsertArgColumnMismatch::InvalidColumnSize {
                        column: format!(
                            "playbook_step_id {} not found in playbook_stages",
                            step.stage_step_id
                        ),
                        expected: 1,
                        found: 0,
                    })
                })?;
                stage_groups.entry(*stage_oid).or_default().push(step);
            }

            for (_stage_oid, steps) in &stage_groups {
                for step in steps {
                    if step.tasks.is_empty() {
                        return Err(DataError::InvalidInput(format!(
                            "Step {} has no tasks linked.",
                            step.stage_step_id
                        )));
                    }
                }
            }

            let stages: Vec<InsertDispatchStagesArgs> = stage_groups
                .into_iter()
                .map(|(stage_oid, steps)| {
                    let step_args: Vec<InsertDispatchStepsArgs> = steps
                        .iter()
                        .map(|s| InsertDispatchStepsArgs {
                            stage_step_ids: vec![s.stage_step_id],
                            params: vec![s.params.clone()],
                            tasks: vec![InsertDispatchTaskArgs {
                                step_task_ids: s.tasks.iter().map(|t| t.step_task_id).collect(),
                            }],
                        })
                        .collect();

                    InsertDispatchStagesArgs {
                        playbook_stage_ids: vec![stage_oid],
                        params: vec![json!({})],
                        steps: step_args,
                    }
                })
                .collect();

            objs.push(InsertDispatchArgs {
                intents: vec![d.intent.clone()],
                stages,
            });
        }

        insert_dispatches(&state.db, journey_ids, objs).await
    }
    .await;

    let models = match result {
        Ok(m) => m,
        Err(e) => return db_error("insert_dispatches", e),
    };

    // Auto-spawn in-process execution for any dispatch whose journey is fully
    // bound. External V1 always runs in-process (no flow-url branch yet —
    // can be added when external customers want a separate flow service).
    for model in models.iter() {
        let dispatch_id = model.id;

        let ready = dispatch_ready(&state.db, model).await.unwrap_or(false);
        if !ready {
            log::info!(
                "Dispatch {} (journey {}) not ready — unbound step inputs, skipping auto-execution",
                dispatch_id,
                model.journey_id,
            );
            continue;
        }

        let state = state.clone();
        let model = model.clone();
        tokio::spawn(async move {
            let dispatch_id = model.id;
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

    let dtos: Vec<Dispatch> = models.into_iter().map(Dispatch::from).collect();
    (
        StatusCode::CREATED,
        Json(json!({
            "status": "ok",
            "results": dtos.len(),
            "objs": dtos,
        })),
    )
        .into_response()
}

/// `GET /api/dispatches/{id}` — poll status, namespace-gated.
pub async fn dispatches_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    match dispatch_in_namespace(&state.db, id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found("dispatch", id),
        Err(e) => return db_error("dispatch_in_namespace", e),
    }

    match select_dispatch_by_universal_id(&state.db, id).await {
        Ok(d) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": 1,
                "obj": d,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_dispatch_by_universal_id", e),
    }
}

/// `GET /api/dispatches/{id}/task_attempts` — per-dispatch task-attempt list,
/// namespace-gated. Pure read; no execution side-effects.
pub async fn dispatches_task_attempts_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    match dispatch_in_namespace(&state.db, id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found("dispatch", id),
        Err(e) => return db_error("dispatch_in_namespace", e),
    }

    match select_task_attempts_by_dispatch_id(&state.db, id).await {
        Ok(attempts) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": attempts.len(),
                "objs": attempts,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_task_attempts_by_dispatch_id", e),
    }
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
