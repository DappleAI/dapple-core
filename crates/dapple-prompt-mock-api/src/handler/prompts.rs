use crate::handler::{
    get_response, index_response, not_found_response, patch_response, post_response,
};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::{PROMPT_PATHS, PROMPTS};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetPromptSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetPromptSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetPromptSetQueryParams> for GetPromptSetQuery {
    fn from(params: GetPromptSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetPromptSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetPromptSetQuery::All,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct GetPromptPathSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetPromptPathSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetPromptPathSetQueryParams> for GetPromptPathSetQuery {
    fn from(params: GetPromptPathSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetPromptPathSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetPromptPathSetQuery::All,
        }
    }
}

// Fluent message reference schema with optional role
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PromptFluentMessageRefSchema {
    pub fluent_message_id: Uuid,
    #[serde(default)]
    pub role: Option<String>, // e.g., "system", "user", "assistant"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePromptSchema {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fluent_messages: Vec<PromptFluentMessageRefSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePromptSetSchema {
    pub objs: Vec<CreatePromptSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchPromptSchema {
    pub path_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fluent_messages: Option<Vec<PromptFluentMessageRefSchema>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchPromptSetSchema {
    pub objs: Vec<PatchPromptSchema>,
}

pub async fn prompts_index_handler(
    Query(params): Query<GetPromptSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetPromptSetQuery = params.into();
    let objs = match query {
        GetPromptSetQuery::ByObjectIds { ids } => store.get_by_ids(PROMPTS, &ids),
        GetPromptSetQuery::All => store.get_all(PROMPTS),
    };

    index_response(objs)
}

pub async fn prompts_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(PROMPTS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Prompt")),
    }
}

pub async fn prompts_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreatePromptSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let path_id = Uuid::now_v7();
            let fluent_messages: Vec<Value> = s
                .fluent_messages
                .into_iter()
                .enumerate()
                .map(|(idx, fm)| {
                    json!({
                        "fluent_message_id": fm.fluent_message_id.to_string(),
                        "role": fm.role,
                        "ordinal": idx
                    })
                })
                .collect();

            json!({
                "path": { "id": path_id.to_string() },
                "name": s.name,
                "description": s.description.unwrap_or_default(),
                "version": { "major": 0, "minor": 0, "patch": 1 },
                "fluent_messages": fluent_messages
            })
        })
        .collect();

    let inserted = store.insert(PROMPTS, entities);
    post_response(inserted)
}

pub async fn prompts_patch_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<PatchPromptSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let mut updated = vec![];
    for patch in body.objs {
        let existing = store.get_all(PROMPTS).into_iter().find(|p| {
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
            if let Some(fluent_msgs) = patch.fluent_messages {
                let fluent_messages: Vec<Value> = fluent_msgs
                    .into_iter()
                    .enumerate()
                    .map(|(idx, fm)| {
                        json!({
                            "fluent_message_id": fm.fluent_message_id.to_string(),
                            "role": fm.role,
                            "ordinal": idx
                        })
                    })
                    .collect();
                existing["fluent_messages"] = json!(fluent_messages);
            }
            if let Some(version) = existing.get_mut("version")
                && let Some(patch_num) = version.get("patch").and_then(|v| v.as_u64())
            {
                version["patch"] = json!(patch_num + 1);
            }
            existing["id"] = json!(Uuid::now_v7().to_string());

            let inserted = store.insert(PROMPTS, vec![existing]);
            updated.extend(inserted);
        }
    }

    patch_response(updated)
}

// Prompt Paths handlers
pub async fn prompt_paths_index_handler(
    Query(params): Query<GetPromptPathSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetPromptPathSetQuery = params.into();
    let objs = match query {
        GetPromptPathSetQuery::ByObjectIds { ids } => store.get_by_ids(PROMPT_PATHS, &ids),
        GetPromptPathSetQuery::All => store.get_all(PROMPT_PATHS),
    };

    index_response(objs)
}

pub async fn prompt_paths_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(PROMPT_PATHS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Prompt Path")),
    }
}
