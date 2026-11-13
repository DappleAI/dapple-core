use crate::handler::{
    get_response, index_response, not_found_response, patch_response, post_response,
};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::{FLUENT_MESSAGE_PATHS, FLUENT_MESSAGES};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetFluentMessageSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetFluentMessageSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetFluentMessageSetQueryParams> for GetFluentMessageSetQuery {
    fn from(params: GetFluentMessageSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetFluentMessageSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetFluentMessageSetQuery::All,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct GetFluentMessagePathSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetFluentMessagePathSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetFluentMessagePathSetQueryParams> for GetFluentMessagePathSetQuery {
    fn from(params: GetFluentMessagePathSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetFluentMessagePathSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetFluentMessagePathSetQuery::All,
        }
    }
}

// Localized body schema
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FluentMessageLocalizedBodySchema {
    pub locale: String,
    pub body: String,
}

// Engine body schema
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FluentMessageEngineBodySchema {
    pub locale: String,
    pub maker: String,
    pub engine_key: String,
    pub engine_version: String,
    pub body: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateFluentMessageSchema {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub localized_bodies: Vec<FluentMessageLocalizedBodySchema>,
    #[serde(default)]
    pub engine_bodies: Vec<FluentMessageEngineBodySchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateFluentMessageSetSchema {
    pub objs: Vec<CreateFluentMessageSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchFluentMessageSchema {
    pub path_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub localized_bodies: Option<Vec<FluentMessageLocalizedBodySchema>>,
    #[serde(default)]
    pub engine_bodies: Option<Vec<FluentMessageEngineBodySchema>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchFluentMessageSetSchema {
    pub objs: Vec<PatchFluentMessageSchema>,
}

pub async fn fluent_messages_index_handler(
    Query(params): Query<GetFluentMessageSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetFluentMessageSetQuery = params.into();
    let objs = match query {
        GetFluentMessageSetQuery::ByObjectIds { ids } => store.get_by_ids(FLUENT_MESSAGES, &ids),
        GetFluentMessageSetQuery::All => store.get_all(FLUENT_MESSAGES),
    };

    index_response(objs)
}

pub async fn fluent_messages_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(FLUENT_MESSAGES, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Fluent Message")),
    }
}

pub async fn fluent_messages_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateFluentMessageSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let path_id = Uuid::now_v7();
            let localized_bodies: Vec<Value> = s
                .localized_bodies
                .iter()
                .map(|lb| {
                    json!({
                        "locale": lb.locale,
                        "body": lb.body
                    })
                })
                .collect();
            let engine_bodies: Vec<Value> = s
                .engine_bodies
                .iter()
                .map(|eb| {
                    json!({
                        "locale": eb.locale,
                        "maker": eb.maker,
                        "engine_key": eb.engine_key,
                        "engine_version": eb.engine_version,
                        "body": eb.body
                    })
                })
                .collect();
            json!({
                "path": { "id": path_id.to_string() },
                "name": s.name,
                "description": s.description.unwrap_or_default(),
                "localized_bodies": localized_bodies,
                "engine_bodies": engine_bodies,
                "version": { "major": 0, "minor": 0, "patch": 1 }
            })
        })
        .collect();

    let inserted = store.insert(FLUENT_MESSAGES, entities);
    post_response(inserted)
}

pub async fn fluent_messages_patch_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<PatchFluentMessageSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let mut updated = vec![];
    for patch in body.objs {
        let existing = store.get_all(FLUENT_MESSAGES).into_iter().find(|p| {
            p.get("path")
                .and_then(|path| path.get("id"))
                .and_then(|id| id.as_str())
                == Some(&patch.path_id.to_string())
        });

        if let Some(mut existing) = existing {
            if let Some(name) = patch.name {
                existing["name"] = json!(name);
            }
            if let Some(description) = patch.description {
                existing["description"] = json!(description);
            }
            if let Some(localized_bodies) = patch.localized_bodies {
                let bodies_json: Vec<Value> = localized_bodies
                    .iter()
                    .map(|lb| {
                        json!({
                            "locale": lb.locale,
                            "body": lb.body
                        })
                    })
                    .collect();
                existing["localized_bodies"] = json!(bodies_json);
            }
            if let Some(engine_bodies) = patch.engine_bodies {
                let bodies_json: Vec<Value> = engine_bodies
                    .iter()
                    .map(|eb| {
                        json!({
                            "locale": eb.locale,
                            "maker": eb.maker,
                            "engine_key": eb.engine_key,
                            "engine_version": eb.engine_version,
                            "body": eb.body
                        })
                    })
                    .collect();
                existing["engine_bodies"] = json!(bodies_json);
            }
            if let Some(version) = existing.get_mut("version")
                && let Some(patch_num) = version.get("patch").and_then(|v| v.as_u64())
            {
                version["patch"] = json!(patch_num + 1);
            }
            existing["id"] = json!(Uuid::now_v7().to_string());

            let inserted = store.insert(FLUENT_MESSAGES, vec![existing]);
            updated.extend(inserted);
        }
    }

    patch_response(updated)
}

// Fluent Message Paths handlers
pub async fn fluent_message_paths_index_handler(
    Query(params): Query<GetFluentMessagePathSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetFluentMessagePathSetQuery = params.into();
    let objs = match query {
        GetFluentMessagePathSetQuery::ByObjectIds { ids } => {
            store.get_by_ids(FLUENT_MESSAGE_PATHS, &ids)
        }
        GetFluentMessagePathSetQuery::All => store.get_all(FLUENT_MESSAGE_PATHS),
    };

    index_response(objs)
}

pub async fn fluent_message_paths_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(FLUENT_MESSAGE_PATHS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Fluent Message Path")),
    }
}
