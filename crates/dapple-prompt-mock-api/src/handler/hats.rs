use crate::handler::{get_response, index_response, not_found_response, post_response};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::HATS;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetHatSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetHatSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetHatSetQueryParams> for GetHatSetQuery {
    fn from(params: GetHatSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetHatSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetHatSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateHatSchema {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateHatSetSchema {
    pub objs: Vec<CreateHatSchema>,
}

pub async fn hats_index_handler(
    Query(params): Query<GetHatSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetHatSetQuery = params.into();
    let objs = match query {
        GetHatSetQuery::ByObjectIds { ids } => store.get_by_ids(HATS, &ids),
        GetHatSetQuery::All => store.get_all(HATS),
    };

    index_response(objs)
}

pub async fn hats_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(HATS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Hat")),
    }
}

pub async fn hats_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateHatSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name,
                "description": s.description.unwrap_or_default(),
            })
        })
        .collect();

    let inserted = store.insert(HATS, entities);
    post_response(inserted)
}
