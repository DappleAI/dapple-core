use crate::handler::{
    get_response, index_response, not_found_response, patch_response, post_response,
};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::{ASSET_PATHS, ASSETS};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetAssetSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetAssetSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetAssetSetQueryParams> for GetAssetSetQuery {
    fn from(params: GetAssetSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetAssetSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetAssetSetQuery::All,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct GetAssetPathSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetAssetPathSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetAssetPathSetQueryParams> for GetAssetPathSetQuery {
    fn from(params: GetAssetPathSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetAssetPathSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetAssetPathSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateAssetSchema {
    pub name: String,
    pub asset_kind: String,
    pub contents: String, // Base64-encoded content
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateAssetSetSchema {
    pub objs: Vec<CreateAssetSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchAssetSchema {
    pub path_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub contents: Option<String>, // Base64-encoded content
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchAssetSetSchema {
    pub objs: Vec<PatchAssetSchema>,
}

pub async fn assets_index_handler(
    Query(params): Query<GetAssetSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetAssetSetQuery = params.into();
    let objs = match query {
        GetAssetSetQuery::ByObjectIds { ids } => store.get_by_ids(ASSETS, &ids),
        GetAssetSetQuery::All => store.get_all(ASSETS),
    };

    index_response(objs)
}

pub async fn assets_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(ASSETS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Asset")),
    }
}

pub async fn assets_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateAssetSetSchema>,
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
                "asset_kind": s.asset_kind,
                "contents": s.contents,
                "version": { "major": 0, "minor": 0, "patch": 1 }
            })
        })
        .collect();

    let inserted = store.insert(ASSETS, entities);
    post_response(inserted)
}

pub async fn assets_patch_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<PatchAssetSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let mut updated = vec![];
    for patch in body.objs {
        let existing = store.get_all(ASSETS).into_iter().find(|p| {
            p.get("path")
                .and_then(|path| path.get("id"))
                .and_then(|id| id.as_str())
                == Some(&patch.path_id.to_string())
        });

        if let Some(mut existing) = existing {
            if let Some(name) = patch.name {
                existing["name"] = json!(name);
            }
            if let Some(contents) = patch.contents {
                existing["contents"] = json!(contents);
            }
            if let Some(version) = existing.get_mut("version")
                && let Some(patch_num) = version.get("patch").and_then(|v| v.as_u64())
            {
                version["patch"] = json!(patch_num + 1);
            }
            existing["id"] = json!(Uuid::now_v7().to_string());

            let inserted = store.insert(ASSETS, vec![existing]);
            updated.extend(inserted);
        }
    }

    patch_response(updated)
}

// Asset Paths handlers
pub async fn asset_paths_index_handler(
    Query(params): Query<GetAssetPathSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetAssetPathSetQuery = params.into();
    let objs = match query {
        GetAssetPathSetQuery::ByObjectIds { ids } => store.get_by_ids(ASSET_PATHS, &ids),
        GetAssetPathSetQuery::All => store.get_all(ASSET_PATHS),
    };

    index_response(objs)
}

pub async fn asset_paths_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(ASSET_PATHS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Asset Path")),
    }
}
