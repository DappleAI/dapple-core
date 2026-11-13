use crate::handler::{get_response, index_response, not_found_response, post_response};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::CATEGORIES;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetCategorySetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetCategorySetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetCategorySetQueryParams> for GetCategorySetQuery {
    fn from(params: GetCategorySetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetCategorySetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetCategorySetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCategorySchema {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCategorySetSchema {
    pub objs: Vec<CreateCategorySchema>,
}

pub async fn categories_index_handler(
    Query(params): Query<GetCategorySetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetCategorySetQuery = params.into();
    let objs = match query {
        GetCategorySetQuery::ByObjectIds { ids } => store.get_by_ids(CATEGORIES, &ids),
        GetCategorySetQuery::All => store.get_all(CATEGORIES),
    };

    index_response(objs)
}

pub async fn categories_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(CATEGORIES, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Category")),
    }
}

pub async fn categories_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateCategorySetSchema>,
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

    let inserted = store.insert(CATEGORIES, entities);
    post_response(inserted)
}
