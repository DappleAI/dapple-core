use crate::handler::{get_response, index_response, not_found_response, post_response};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::{DISPATCH_TASK_ATTEMPTS, DISPATCHES};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetDispatchSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub journey_id: Option<Uuid>,
}

pub enum GetDispatchSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    ByJourney { journey_id: Uuid },
    All,
}

impl From<GetDispatchSetQueryParams> for GetDispatchSetQuery {
    fn from(params: GetDispatchSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetDispatchSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            Some("by_journey") => GetDispatchSetQuery::ByJourney {
                journey_id: params.journey_id.unwrap_or_default(),
            },
            _ => GetDispatchSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDispatchSchema {
    pub journey_id: Uuid,
    #[serde(default)]
    pub intent: serde_json::Value,
    #[serde(default)]
    pub steps: Vec<CreateDispatchStepSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDispatchStepSchema {
    pub stage_step_id: Uuid,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default)]
    pub tasks: Vec<CreateDispatchTaskSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDispatchTaskSchema {
    pub step_task_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDispatchSetSchema {
    pub objs: Vec<CreateDispatchSchema>,
}

pub async fn dispatches_index_handler(
    Query(params): Query<GetDispatchSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetDispatchSetQuery = params.into();
    let objs = match query {
        GetDispatchSetQuery::ByObjectIds { ids } => store.get_by_ids(DISPATCHES, &ids),
        GetDispatchSetQuery::ByJourney { journey_id } => {
            store.get_by_parent(DISPATCHES, "journey_id", journey_id)
        }
        GetDispatchSetQuery::All => store.get_all(DISPATCHES),
    };

    index_response(objs)
}

pub async fn dispatches_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(DISPATCHES, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Dispatch")),
    }
}

fn build_dispatch_tasks(tasks: Vec<CreateDispatchTaskSchema>) -> Vec<Value> {
    tasks
        .into_iter()
        .map(|t| {
            json!({
                "id": Uuid::now_v7().to_string(),
                "step_task_id": t.step_task_id.to_string()
            })
        })
        .collect()
}

fn build_dispatch_steps(steps: Vec<CreateDispatchStepSchema>) -> Vec<Value> {
    steps
        .into_iter()
        .map(|s| {
            json!({
                "id": Uuid::now_v7().to_string(),
                "stage_step_id": s.stage_step_id.to_string(),
                "params": s.params,
                "tasks": build_dispatch_tasks(s.tasks)
            })
        })
        .collect()
}

pub async fn dispatches_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateDispatchSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            // Mock: wrap all steps into a single auto-derived stage
            let steps = build_dispatch_steps(s.steps);
            let stages: Vec<Value> = vec![json!({
                "id": Uuid::now_v7().to_string(),
                "params": {},
                "steps": steps
            })];

            json!({
                "journey_id": s.journey_id.to_string(),
                "intent": s.intent,
                "stages": stages,
                "phase": "queued"
            })
        })
        .collect();

    let inserted = store.insert(DISPATCHES, entities);
    post_response(inserted)
}

/// Dispatch phases lookup endpoint
pub async fn dispatch_phases_handler() -> Json<Value> {
    Json(json!({
        "status": "success",
        "results": 5,
        "objs": [
            { "name": "queued", "description": "Dispatch is waiting to be processed" },
            { "name": "running", "description": "Dispatch is currently being processed" },
            { "name": "failed", "description": "Dispatch processing failed" },
            { "name": "ok", "description": "Dispatch completed successfully" },
            { "name": "canceled", "description": "Dispatch was canceled" }
        ]
    }))
}

// Task attempts handlers (nested under dispatches)
pub async fn task_attempts_index_handler(
    Path(dispatch_id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let objs = store.get_by_parent(DISPATCH_TASK_ATTEMPTS, "dispatch", dispatch_id);
    index_response(objs)
}

pub async fn task_attempts_get_handler(
    Path((dispatch_id, attempt_id)): Path<(Uuid, Uuid)>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    // Find the task attempt by both dispatch and attempt id
    let attempt = store
        .get_by_parent(DISPATCH_TASK_ATTEMPTS, "dispatch", dispatch_id)
        .into_iter()
        .find(|a| {
            a.get("id")
                .and_then(|id| id.as_str())
                .map(|s| s == attempt_id.to_string())
                .unwrap_or(false)
        });

    match attempt {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Task Attempt")),
    }
}
