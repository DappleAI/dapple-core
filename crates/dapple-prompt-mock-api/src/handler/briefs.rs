use crate::handler::{
    get_response, index_response, not_found_response, patch_response, post_response,
};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::{BRIEF_PATHS, BRIEFS};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetBriefSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetBriefSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetBriefSetQueryParams> for GetBriefSetQuery {
    fn from(params: GetBriefSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetBriefSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetBriefSetQuery::All,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct GetBriefPathSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetBriefPathSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetBriefPathSetQueryParams> for GetBriefPathSetQuery {
    fn from(params: GetBriefPathSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetBriefPathSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetBriefPathSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBriefSchema {
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBriefSetSchema {
    pub objs: Vec<CreateBriefSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchBriefSchema {
    pub path_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchBriefSetSchema {
    pub objs: Vec<PatchBriefSchema>,
}

pub async fn briefs_index_handler(
    Query(params): Query<GetBriefSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetBriefSetQuery = params.into();
    let objs = match query {
        GetBriefSetQuery::ByObjectIds { ids } => store.get_by_ids(BRIEFS, &ids),
        GetBriefSetQuery::All => store.get_all(BRIEFS),
    };

    index_response(objs)
}

pub async fn briefs_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(BRIEFS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Brief")),
    }
}

pub async fn briefs_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateBriefSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let path_id = Uuid::now_v7();
            json!({
                "path": { "id": path_id.to_string() },
                "name": s.name,
                "title": s.title,
                "description": s.description.unwrap_or_default(),
                "version": { "major": 0, "minor": 0, "patch": 1 },
                "units": []
            })
        })
        .collect();

    let inserted = store.insert(BRIEFS, entities);
    post_response(inserted)
}

pub async fn briefs_patch_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<PatchBriefSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let mut updated = vec![];
    for patch in body.objs {
        let existing = store.get_all(BRIEFS).into_iter().find(|p| {
            p.get("path")
                .and_then(|path| path.get("id"))
                .and_then(|id| id.as_str())
                == Some(&patch.path_id.to_string())
        });

        if let Some(mut existing) = existing {
            if let Some(name) = patch.name {
                existing["name"] = json!(name);
            }
            if let Some(title) = patch.title {
                existing["title"] = json!(title);
            }
            if let Some(description) = patch.description {
                existing["description"] = json!(description);
            }
            if let Some(version) = existing.get_mut("version")
                && let Some(patch_num) = version.get("patch").and_then(|v| v.as_u64())
            {
                version["patch"] = json!(patch_num + 1);
            }
            existing["id"] = json!(Uuid::now_v7().to_string());

            let inserted = store.insert(BRIEFS, vec![existing]);
            updated.extend(inserted);
        }
    }

    patch_response(updated)
}

// Brief Paths handlers
pub async fn brief_paths_index_handler(
    Query(params): Query<GetBriefPathSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetBriefPathSetQuery = params.into();
    let objs = match query {
        GetBriefPathSetQuery::ByObjectIds { ids } => store.get_by_ids(BRIEF_PATHS, &ids),
        GetBriefPathSetQuery::All => store.get_all(BRIEF_PATHS),
    };

    index_response(objs)
}

pub async fn brief_paths_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(BRIEF_PATHS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Brief Path")),
    }
}
