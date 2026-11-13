use crate::handler::{get_response, index_response, not_found_response, post_response};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::STEPS;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetStepSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetStepSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetStepSetQueryParams> for GetStepSetQuery {
    fn from(params: GetStepSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetStepSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetStepSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStepTaskSchema {
    pub task_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStepSchema {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tasks: Vec<CreateStepTaskSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStepSetSchema {
    pub objs: Vec<CreateStepSchema>,
}

pub async fn steps_index_handler(
    Query(params): Query<GetStepSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetStepSetQuery = params.into();
    let objs = match query {
        GetStepSetQuery::ByObjectIds { ids } => store.get_by_ids(STEPS, &ids),
        GetStepSetQuery::All => store.get_all(STEPS),
    };

    index_response(objs)
}

pub async fn steps_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(STEPS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Step")),
    }
}

pub async fn steps_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateStepSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let tasks: Vec<Value> = s
                .tasks
                .into_iter()
                .map(|t| {
                    json!({
                        "id": Uuid::now_v7().to_string(),
                        "task": {
                            "id": t.task_id.to_string()
                        }
                    })
                })
                .collect();

            json!({
                "name": s.name,
                "description": s.description.unwrap_or_default(),
                "tasks": tasks
            })
        })
        .collect();

    let inserted = store.insert(STEPS, entities);
    post_response(inserted)
}
