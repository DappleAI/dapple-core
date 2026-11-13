use crate::handler::{get_response, index_response, not_found_response, post_response};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::TASK_PROMPTS;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetTaskPromptSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub task_id: Option<Uuid>,
}

pub enum GetTaskPromptSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    ByTask { task_id: Uuid },
    All,
}

impl From<GetTaskPromptSetQueryParams> for GetTaskPromptSetQuery {
    fn from(params: GetTaskPromptSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetTaskPromptSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            Some("by_task") => GetTaskPromptSetQuery::ByTask {
                task_id: params.task_id.unwrap_or_default(),
            },
            _ => GetTaskPromptSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskPromptSchema {
    pub task_id: Uuid,
    pub prompt_id: Uuid,
    #[serde(default)]
    pub ordinal: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskPromptSetSchema {
    pub objs: Vec<CreateTaskPromptSchema>,
}

pub async fn task_prompts_index_handler(
    Query(params): Query<GetTaskPromptSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetTaskPromptSetQuery = params.into();
    let objs = match query {
        GetTaskPromptSetQuery::ByObjectIds { ids } => store.get_by_ids(TASK_PROMPTS, &ids),
        GetTaskPromptSetQuery::ByTask { task_id } => {
            store.get_by_parent(TASK_PROMPTS, "task", task_id)
        }
        GetTaskPromptSetQuery::All => store.get_all(TASK_PROMPTS),
    };

    index_response(objs)
}

pub async fn task_prompts_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(TASK_PROMPTS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Task Prompt")),
    }
}

pub async fn task_prompts_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateTaskPromptSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            json!({
                "task": { "id": s.task_id.to_string() },
                "prompt": { "id": s.prompt_id.to_string() },
                "ordinal": s.ordinal.unwrap_or(i as i32)
            })
        })
        .collect();

    let inserted = store.insert(TASK_PROMPTS, entities);
    post_response(inserted)
}
