use crate::handler::{get_response, index_response, not_found_response, post_response};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::STAGES;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetStageSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetStageSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetStageSetQueryParams> for GetStageSetQuery {
    fn from(params: GetStageSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetStageSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetStageSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStageSchema {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub step_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStageSetSchema {
    pub objs: Vec<CreateStageSchema>,
}

pub async fn stages_index_handler(
    Query(params): Query<GetStageSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetStageSetQuery = params.into();
    let objs = match query {
        GetStageSetQuery::ByObjectIds { ids } => store.get_by_ids(STAGES, &ids),
        GetStageSetQuery::All => store.get_all(STAGES),
    };

    index_response(objs)
}

pub async fn stages_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(STAGES, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Stage")),
    }
}

pub async fn stages_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateStageSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let steps: Vec<Value> = s
                .step_ids
                .into_iter()
                .map(|id| {
                    json!({
                        "id": id.to_string()
                    })
                })
                .collect();

            json!({
                "name": s.name,
                "description": s.description.unwrap_or_default(),
                "steps": steps
            })
        })
        .collect();

    let inserted = store.insert(STAGES, entities);
    post_response(inserted)
}
