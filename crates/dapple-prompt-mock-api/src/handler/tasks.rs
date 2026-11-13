use crate::handler::{get_response, index_response, not_found_response, post_response};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::TASKS;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetTaskSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetTaskSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetTaskSetQueryParams> for GetTaskSetQuery {
    fn from(params: GetTaskSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetTaskSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetTaskSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskSchema {
    pub name: String,
    pub description: String,
    pub function: String,
    #[serde(default)]
    pub function_input: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskSetSchema {
    pub objs: Vec<CreateTaskSchema>,
}

pub async fn tasks_index_handler(
    Query(params): Query<GetTaskSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetTaskSetQuery = params.into();
    let objs = match query {
        GetTaskSetQuery::ByObjectIds { ids } => store.get_by_ids(TASKS, &ids),
        GetTaskSetQuery::All => store.get_all(TASKS),
    };

    index_response(objs)
}

pub async fn tasks_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(TASKS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Task")),
    }
}

pub async fn tasks_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateTaskSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name,
                "description": s.description,
                "function": s.function,
                "function_input": s.function_input,
                "version": {
                    "major": 0,
                    "minor": 0,
                    "patch": 1
                }
            })
        })
        .collect();

    let inserted = store.insert(TASKS, entities);
    post_response(inserted)
}
