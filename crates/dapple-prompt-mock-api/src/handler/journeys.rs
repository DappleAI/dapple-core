use crate::handler::{get_response, index_response, not_found_response, post_response};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::JOURNEYS;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetJourneySetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub playbook_id: Option<Uuid>,
}

pub enum GetJourneySetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    ByPlaybook { playbook_id: Uuid },
    All,
}

impl From<GetJourneySetQueryParams> for GetJourneySetQuery {
    fn from(params: GetJourneySetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetJourneySetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            Some("by_playbook") => GetJourneySetQuery::ByPlaybook {
                playbook_id: params.playbook_id.unwrap_or_default(),
            },
            _ => GetJourneySetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateJourneySchema {
    pub name: String,
    pub playbook_id: Uuid,
    #[serde(default)]
    pub brief_pack_ids: Vec<Uuid>,
    #[serde(default)]
    pub inference_profile_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateJourneySetSchema {
    pub objs: Vec<CreateJourneySchema>,
}

pub async fn journeys_index_handler(
    Query(params): Query<GetJourneySetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetJourneySetQuery = params.into();
    let objs = match query {
        GetJourneySetQuery::ByObjectIds { ids } => store.get_by_ids(JOURNEYS, &ids),
        GetJourneySetQuery::ByPlaybook { playbook_id } => {
            store.get_by_parent(JOURNEYS, "playbook_id", playbook_id)
        }
        GetJourneySetQuery::All => store.get_all(JOURNEYS),
    };

    index_response(objs)
}

pub async fn journeys_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(JOURNEYS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Journey")),
    }
}

pub async fn journeys_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateJourneySetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let brief_packs: Vec<Value> = s
                .brief_pack_ids
                .iter()
                .map(|id| json!({"id": id.to_string()}))
                .collect();

            let inference_profiles: Vec<Value> = s
                .inference_profile_ids
                .iter()
                .map(|id| json!({"id": id.to_string()}))
                .collect();

            json!({
                "name": s.name,
                "playbook_id": s.playbook_id.to_string(),
                "brief_packs": brief_packs,
                "inference_profiles": inference_profiles
            })
        })
        .collect();

    let inserted = store.insert(JOURNEYS, entities);
    post_response(inserted)
}
