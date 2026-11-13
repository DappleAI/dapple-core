use crate::handler::{
    get_response, index_response, not_found_response, patch_response, post_response,
};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::PLAYBOOKS;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetPlaybookSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
}

pub enum GetPlaybookSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All,
}

impl From<GetPlaybookSetQueryParams> for GetPlaybookSetQuery {
    fn from(params: GetPlaybookSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetPlaybookSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            _ => GetPlaybookSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePlaybookStageSchema {
    #[serde(default)]
    pub steps: Vec<CreatePlaybookStageStepSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePlaybookStageStepSchema {
    pub step_id: Uuid,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default)]
    pub tasks: Vec<CreatePlaybookTaskSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePlaybookTaskSchema {
    pub task_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePlaybookSchema {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub categories: Vec<Uuid>,
    #[serde(default)]
    pub stages: Vec<CreatePlaybookStageSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePlaybookSetSchema {
    pub objs: Vec<CreatePlaybookSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchPlaybookSchema {
    pub path_id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchPlaybookSetSchema {
    pub objs: Vec<PatchPlaybookSchema>,
}

pub async fn playbooks_index_handler(
    Query(params): Query<GetPlaybookSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetPlaybookSetQuery = params.into();
    let objs = match query {
        GetPlaybookSetQuery::ByObjectIds { ids } => store.get_by_ids(PLAYBOOKS, &ids),
        GetPlaybookSetQuery::All => store.get_all(PLAYBOOKS),
    };

    index_response(objs)
}

pub async fn playbooks_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(PLAYBOOKS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Playbook")),
    }
}

pub async fn playbooks_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreatePlaybookSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let path_id = Uuid::now_v7();

            let categories: Vec<Value> = s
                .categories
                .into_iter()
                .map(|id| json!({ "id": id.to_string() }))
                .collect();

            let stages: Vec<Value> = s
                .stages
                .into_iter()
                .map(|stage| {
                    let steps: Vec<Value> = stage
                        .steps
                        .into_iter()
                        .map(|step| {
                            let tasks: Vec<Value> = step
                                .tasks
                                .into_iter()
                                .map(|task| {
                                    json!({
                                        "id": Uuid::now_v7().to_string(),
                                        "task": { "id": task.task_id.to_string() }
                                    })
                                })
                                .collect();
                            json!({
                                "id": Uuid::now_v7().to_string(),
                                "step": { "id": step.step_id.to_string() },
                                "tasks": tasks
                            })
                        })
                        .collect();

                    json!({
                        "id": Uuid::now_v7().to_string(),
                        "stage": { "id": Uuid::now_v7().to_string() },
                        "steps": steps
                    })
                })
                .collect();

            json!({
                "path": { "id": path_id.to_string() },
                "name": s.name,
                "description": s.description,
                "version": { "major": 0, "minor": 0, "patch": 1 },
                "categories": categories,
                "stages": stages
            })
        })
        .collect();

    let inserted = store.insert(PLAYBOOKS, entities);
    post_response(inserted)
}

pub async fn playbooks_patch_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<PatchPlaybookSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let mut updated = vec![];
    for patch in body.objs {
        // Find existing playbook by path_id
        let existing = store.get_all(PLAYBOOKS).into_iter().find(|p| {
            p.get("path")
                .and_then(|path| path.get("id"))
                .and_then(|id| id.as_str())
                == Some(&patch.path_id.to_string())
        });

        if let Some(mut existing) = existing {
            // Update fields
            if let Some(name) = patch.name {
                existing["name"] = json!(name);
            }
            if let Some(desc) = patch.description {
                existing["description"] = json!(desc);
            }
            // Increment version
            if let Some(version) = existing.get_mut("version")
                && let Some(patch_num) = version.get("patch").and_then(|v| v.as_u64())
            {
                version["patch"] = json!(patch_num + 1);
            }

            // Generate new id for the new version
            let new_id = Uuid::now_v7();
            existing["id"] = json!(new_id.to_string());

            let inserted = store.insert(PLAYBOOKS, vec![existing]);
            updated.extend(inserted);
        }
    }

    patch_response(updated)
}
