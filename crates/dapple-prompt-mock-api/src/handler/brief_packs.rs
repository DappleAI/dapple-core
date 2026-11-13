use crate::handler::{
    get_response, index_response, not_found_response, patch_response, post_response,
};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::{BRIEF_PACK_PATHS, BRIEF_PACKS};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetBriefPackSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetBriefPackSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetBriefPackSetQueryParams> for GetBriefPackSetQuery {
    fn from(params: GetBriefPackSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetBriefPackSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetBriefPackSetQuery::All,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct GetBriefPackPathSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetBriefPackPathSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetBriefPackPathSetQueryParams> for GetBriefPackPathSetQuery {
    fn from(params: GetBriefPackPathSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetBriefPackPathSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetBriefPackPathSetQuery::All,
        }
    }
}

// Input schema for group references with ui_overrides
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BriefGroupReferenceInputSchema {
    pub id: Uuid,
    #[serde(default)]
    pub ui_overrides: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBriefPackSchema {
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub pack_spec: serde_json::Value,
    #[serde(default)]
    pub groups: Vec<BriefGroupReferenceInputSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBriefPackSetSchema {
    pub objs: Vec<CreateBriefPackSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchBriefPackSchema {
    pub path_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub pack_spec: Option<serde_json::Value>,
    #[serde(default)]
    pub groups: Option<Vec<BriefGroupReferenceInputSchema>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchBriefPackSetSchema {
    pub objs: Vec<PatchBriefPackSchema>,
}

pub async fn brief_packs_index_handler(
    Query(params): Query<GetBriefPackSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetBriefPackSetQuery = params.into();
    let objs = match query {
        GetBriefPackSetQuery::ByObjectIds { ids } => store.get_by_ids(BRIEF_PACKS, &ids),
        GetBriefPackSetQuery::All => store.get_all(BRIEF_PACKS),
    };

    index_response(objs)
}

pub async fn brief_packs_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(BRIEF_PACKS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Brief Pack")),
    }
}

pub async fn brief_packs_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateBriefPackSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let path_id = Uuid::now_v7();
            let groups: Vec<Value> = s
                .groups
                .into_iter()
                .enumerate()
                .map(|(idx, g)| json!({
                    "id": g.id.to_string(),
                    "ordinal": idx,
                    "ui_overrides": if g.ui_overrides.is_null() { json!({}) } else { g.ui_overrides }
                }))
                .collect();

            json!({
                "path": { "id": path_id.to_string() },
                "name": s.name,
                "title": s.title,
                "description": s.description.unwrap_or_default(),
                "pack_spec": if s.pack_spec.is_null() { json!({}) } else { s.pack_spec },
                "version": { "major": 0, "minor": 0, "patch": 1 },
                "groups": groups
            })
        })
        .collect();

    let inserted = store.insert(BRIEF_PACKS, entities);
    post_response(inserted)
}

pub async fn brief_packs_patch_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<PatchBriefPackSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let mut updated = vec![];
    for patch in body.objs {
        let existing = store.get_all(BRIEF_PACKS).into_iter().find(|p| {
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
            if let Some(pack_spec) = patch.pack_spec {
                existing["pack_spec"] = pack_spec;
            }
            if let Some(groups_input) = patch.groups {
                let groups: Vec<Value> = groups_input
                    .into_iter()
                    .enumerate()
                    .map(|(idx, g)| json!({
                        "id": g.id.to_string(),
                        "ordinal": idx,
                        "ui_overrides": if g.ui_overrides.is_null() { json!({}) } else { g.ui_overrides }
                    }))
                    .collect();
                existing["groups"] = json!(groups);
            }
            if let Some(version) = existing.get_mut("version")
                && let Some(patch_num) = version.get("patch").and_then(|v| v.as_u64())
            {
                version["patch"] = json!(patch_num + 1);
            }
            existing["id"] = json!(Uuid::now_v7().to_string());

            let inserted = store.insert(BRIEF_PACKS, vec![existing]);
            updated.extend(inserted);
        }
    }

    patch_response(updated)
}

// Brief Pack Paths handlers
pub async fn brief_pack_paths_index_handler(
    Query(params): Query<GetBriefPackPathSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetBriefPackPathSetQuery = params.into();
    let objs = match query {
        GetBriefPackPathSetQuery::ByObjectIds { ids } => store.get_by_ids(BRIEF_PACK_PATHS, &ids),
        GetBriefPackPathSetQuery::All => store.get_all(BRIEF_PACK_PATHS),
    };

    index_response(objs)
}

pub async fn brief_pack_paths_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(BRIEF_PACK_PATHS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Brief Pack Path")),
    }
}
