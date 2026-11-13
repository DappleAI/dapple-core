use crate::handler::{
    get_response, index_response, not_found_response, patch_response, post_response,
};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::{BRIEF_GROUP_PATHS, BRIEF_GROUPS};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetBriefGroupSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetBriefGroupSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetBriefGroupSetQueryParams> for GetBriefGroupSetQuery {
    fn from(params: GetBriefGroupSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetBriefGroupSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetBriefGroupSetQuery::All,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct GetBriefGroupPathSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetBriefGroupPathSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetBriefGroupPathSetQueryParams> for GetBriefGroupPathSetQuery {
    fn from(params: GetBriefGroupPathSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetBriefGroupPathSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetBriefGroupPathSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBriefGroupSchema {
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub brief_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBriefGroupSetSchema {
    pub objs: Vec<CreateBriefGroupSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchBriefGroupSchema {
    pub path_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub brief_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchBriefGroupSetSchema {
    pub objs: Vec<PatchBriefGroupSchema>,
}

pub async fn brief_groups_index_handler(
    Query(params): Query<GetBriefGroupSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetBriefGroupSetQuery = params.into();
    let objs = match query {
        GetBriefGroupSetQuery::ByObjectIds { ids } => store.get_by_ids(BRIEF_GROUPS, &ids),
        GetBriefGroupSetQuery::All => store.get_all(BRIEF_GROUPS),
    };

    index_response(objs)
}

pub async fn brief_groups_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(BRIEF_GROUPS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Brief Group")),
    }
}

pub async fn brief_groups_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateBriefGroupSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let path_id = Uuid::now_v7();
            let briefs: Vec<Value> = s
                .brief_ids
                .into_iter()
                .enumerate()
                .map(|(idx, id)| json!({ "id": id.to_string(), "ordinal": idx }))
                .collect();

            json!({
                "path": { "id": path_id.to_string() },
                "name": s.name,
                "title": s.title,
                "description": s.description.unwrap_or_default(),
                "version": { "major": 0, "minor": 0, "patch": 1 },
                "briefs": briefs
            })
        })
        .collect();

    let inserted = store.insert(BRIEF_GROUPS, entities);
    post_response(inserted)
}

pub async fn brief_groups_patch_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<PatchBriefGroupSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let mut updated = vec![];
    for patch in body.objs {
        let existing = store.get_all(BRIEF_GROUPS).into_iter().find(|p| {
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
            if let Some(brief_ids) = patch.brief_ids {
                let briefs: Vec<Value> = brief_ids
                    .into_iter()
                    .enumerate()
                    .map(|(idx, id)| json!({ "id": id.to_string(), "ordinal": idx }))
                    .collect();
                existing["briefs"] = json!(briefs);
            }
            if let Some(version) = existing.get_mut("version")
                && let Some(patch_num) = version.get("patch").and_then(|v| v.as_u64())
            {
                version["patch"] = json!(patch_num + 1);
            }
            existing["id"] = json!(Uuid::now_v7().to_string());

            let inserted = store.insert(BRIEF_GROUPS, vec![existing]);
            updated.extend(inserted);
        }
    }

    patch_response(updated)
}

// Brief Group Paths handlers
pub async fn brief_group_paths_index_handler(
    Query(params): Query<GetBriefGroupPathSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetBriefGroupPathSetQuery = params.into();
    let objs = match query {
        GetBriefGroupPathSetQuery::ByObjectIds { ids } => store.get_by_ids(BRIEF_GROUP_PATHS, &ids),
        GetBriefGroupPathSetQuery::All => store.get_all(BRIEF_GROUP_PATHS),
    };

    index_response(objs)
}

pub async fn brief_group_paths_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(BRIEF_GROUP_PATHS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Brief Group Path")),
    }
}
