use crate::handler::{
    get_response, index_response, not_found_response, patch_response, post_response,
};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::{VALUE_SLOT_PATHS, VALUE_SLOTS};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetValueSlotSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetValueSlotSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetValueSlotSetQueryParams> for GetValueSlotSetQuery {
    fn from(params: GetValueSlotSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetValueSlotSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetValueSlotSetQuery::All,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct GetValueSlotPathSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetValueSlotPathSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetValueSlotPathSetQueryParams> for GetValueSlotPathSetQuery {
    fn from(params: GetValueSlotPathSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetValueSlotPathSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetValueSlotPathSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateValueSlotSchema {
    pub name: String,
    pub value_kind: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub constraints: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateValueSlotSetSchema {
    pub objs: Vec<CreateValueSlotSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchValueSlotSchema {
    pub path_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub constraints: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchValueSlotSetSchema {
    pub objs: Vec<PatchValueSlotSchema>,
}

// Value Slots handlers
pub async fn value_slots_index_handler(
    Query(params): Query<GetValueSlotSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetValueSlotSetQuery = params.into();
    let objs = match query {
        GetValueSlotSetQuery::ByObjectIds { ids } => store.get_by_ids(VALUE_SLOTS, &ids),
        GetValueSlotSetQuery::All => store.get_all(VALUE_SLOTS),
    };

    index_response(objs)
}

pub async fn value_slots_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(VALUE_SLOTS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Value Slot")),
    }
}

pub async fn value_slots_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateValueSlotSetSchema>,
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
                "value_kind": s.value_kind,
                "title": s.title,
                "description": s.description.unwrap_or_default(),
                "constraints": if s.constraints.is_null() { json!({}) } else { s.constraints },
                "version": { "major": 0, "minor": 0, "patch": 1 }
            })
        })
        .collect();

    let inserted = store.insert(VALUE_SLOTS, entities);
    post_response(inserted)
}

pub async fn value_slots_patch_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<PatchValueSlotSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let mut updated = vec![];
    for patch in body.objs {
        let existing = store.get_all(VALUE_SLOTS).into_iter().find(|p| {
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
            if let Some(constraints) = patch.constraints {
                existing["constraints"] = constraints;
            }
            if let Some(version) = existing.get_mut("version")
                && let Some(patch_num) = version.get("patch").and_then(|v| v.as_u64())
            {
                version["patch"] = json!(patch_num + 1);
            }
            existing["id"] = json!(Uuid::now_v7().to_string());

            let inserted = store.insert(VALUE_SLOTS, vec![existing]);
            updated.extend(inserted);
        }
    }

    patch_response(updated)
}

// Value Slot Paths handlers
pub async fn value_slot_paths_index_handler(
    Query(params): Query<GetValueSlotPathSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetValueSlotPathSetQuery = params.into();
    let objs = match query {
        GetValueSlotPathSetQuery::ByObjectIds { ids } => store.get_by_ids(VALUE_SLOT_PATHS, &ids),
        GetValueSlotPathSetQuery::All => store.get_all(VALUE_SLOT_PATHS),
    };

    index_response(objs)
}

pub async fn value_slot_paths_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(VALUE_SLOT_PATHS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Value Slot Path")),
    }
}
