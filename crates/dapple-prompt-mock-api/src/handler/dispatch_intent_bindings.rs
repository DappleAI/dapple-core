use crate::handler::{get_response, index_response, not_found_response, post_response};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::DISPATCH_INTENT_BINDINGS;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct GetDispatchIntentBindingSetQueryParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub dispatch_id: Option<Uuid>,
}

pub enum GetDispatchIntentBindingSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    ByDispatch { dispatch_id: Uuid },
    All,
}

impl From<GetDispatchIntentBindingSetQueryParams> for GetDispatchIntentBindingSetQuery {
    fn from(params: GetDispatchIntentBindingSetQueryParams) -> Self {
        match params.query.as_deref() {
            Some("by_object_ids") => GetDispatchIntentBindingSetQuery::ByObjectIds {
                ids: params.ids.unwrap_or_default(),
            },
            Some("by_dispatch") => GetDispatchIntentBindingSetQuery::ByDispatch {
                dispatch_id: params.dispatch_id.unwrap_or_default(),
            },
            _ => GetDispatchIntentBindingSetQuery::All,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDispatchIntentBindingSchema {
    pub dispatch_id: Uuid,
    pub value_slot_path_id: Uuid,
    #[serde(default)]
    pub value: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDispatchIntentBindingSetSchema {
    pub objs: Vec<CreateDispatchIntentBindingSchema>,
}

pub async fn dispatch_intent_bindings_index_handler(
    Query(params): Query<GetDispatchIntentBindingSetQueryParams>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let query: GetDispatchIntentBindingSetQuery = params.into();
    let objs = match query {
        GetDispatchIntentBindingSetQuery::ByObjectIds { ids } => {
            store.get_by_ids(DISPATCH_INTENT_BINDINGS, &ids)
        }
        GetDispatchIntentBindingSetQuery::ByDispatch { dispatch_id } => {
            store.get_by_parent(DISPATCH_INTENT_BINDINGS, "dispatch", dispatch_id)
        }
        GetDispatchIntentBindingSetQuery::All => store.get_all(DISPATCH_INTENT_BINDINGS),
    };

    index_response(objs)
}

pub async fn dispatch_intent_bindings_get_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    match store.get_by_id(DISPATCH_INTENT_BINDINGS, id) {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Dispatch Intent Binding")),
    }
}

pub async fn dispatch_intent_bindings_post_handler(
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateDispatchIntentBindingSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            json!({
                "dispatch": { "id": s.dispatch_id.to_string() },
                "value_slot_path": { "id": s.value_slot_path_id.to_string() },
                "value": s.value
            })
        })
        .collect();

    let inserted = store.insert(DISPATCH_INTENT_BINDINGS, entities);
    post_response(inserted)
}
