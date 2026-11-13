use crate::handler::{
    get_response, index_response, not_found_response, patch_response, post_response,
};
use crate::state::MockState;
use crate::store::MockDataStore;
use crate::store::entity_types::BRIEF_UNITS;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefUnitSetQuery {
    ByObjectIds { ids: Vec<Uuid> },
    All {},
}

impl Default for GetBriefUnitSetQuery {
    fn default() -> Self {
        Self::All {}
    }
}

// Output schema for brief units
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateBriefUnitOutputSchema {
    pub source_kind: String,
    #[serde(default)]
    pub source_spec: serde_json::Value,
}

// Subtype schemas for different unit types
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreateBriefUnitSubtypeSchema {
    Markdown {
        #[serde(default)]
        body: Option<String>,
    },
    Notice {
        #[serde(default)]
        notice_type: Option<String>,
        #[serde(default)]
        body: Option<String>,
    },
    Input {
        control_type: String,
        #[serde(default)]
        label_text: Option<String>,
        #[serde(default)]
        control_spec: serde_json::Value,
        #[serde(default)]
        value_slot_path_id: Option<Uuid>,
    },
    Media {
        #[serde(default)]
        media_type: Option<String>,
        #[serde(default)]
        source_spec: serde_json::Value,
    },
    Output {
        #[serde(default)]
        display_format: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBriefUnitSchema {
    pub unit_type: String,
    pub unit_key: String,
    pub ordinal: i32,
    #[serde(default)]
    pub layout: serde_json::Value,
    #[serde(default)]
    pub requires: serde_json::Value,
    #[serde(default)]
    pub extensions: serde_json::Value,
    #[serde(default)]
    pub outputs: Vec<CreateBriefUnitOutputSchema>,
    pub subtype: CreateBriefUnitSubtypeSchema,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBriefUnitSetSchema {
    pub objs: Vec<CreateBriefUnitSchema>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatchBriefUnitSubtypeSchema {
    Markdown {
        #[serde(default)]
        body: Option<String>,
    },
    Notice {
        #[serde(default)]
        notice_type: Option<String>,
        #[serde(default)]
        body: Option<String>,
    },
    Input {
        #[serde(default)]
        control_type: Option<String>,
        #[serde(default)]
        label_text: Option<String>,
        #[serde(default)]
        control_spec: Option<serde_json::Value>,
        #[serde(default)]
        value_slot_path_id: Option<Uuid>,
    },
    Media {
        #[serde(default)]
        media_type: Option<String>,
        #[serde(default)]
        source_spec: Option<serde_json::Value>,
    },
    Output {
        #[serde(default)]
        display_format: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchBriefUnitSchema {
    pub id: Uuid,
    #[serde(default)]
    pub unit_key: Option<String>,
    #[serde(default)]
    pub ordinal: Option<i32>,
    #[serde(default)]
    pub layout: Option<serde_json::Value>,
    #[serde(default)]
    pub requires: Option<serde_json::Value>,
    #[serde(default)]
    pub extensions: Option<serde_json::Value>,
    #[serde(default)]
    pub outputs: Option<Vec<CreateBriefUnitOutputSchema>>,
    #[serde(default)]
    pub subtype: Option<PatchBriefUnitSubtypeSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatchBriefUnitSetSchema {
    pub objs: Vec<PatchBriefUnitSchema>,
}

fn subtype_to_json(subtype: &CreateBriefUnitSubtypeSchema) -> Value {
    match subtype {
        CreateBriefUnitSubtypeSchema::Markdown { body } => json!({
            "kind": "markdown",
            "body": body.clone().unwrap_or_default()
        }),
        CreateBriefUnitSubtypeSchema::Notice { notice_type, body } => json!({
            "kind": "notice",
            "notice_type": notice_type.clone().unwrap_or_else(|| "info".to_string()),
            "body": body.clone().unwrap_or_default()
        }),
        CreateBriefUnitSubtypeSchema::Input {
            control_type,
            label_text,
            control_spec,
            value_slot_path_id,
        } => {
            let mut obj = json!({
                "kind": "input",
                "control_type": control_type,
                "label_text": label_text.clone().unwrap_or_default(),
                "control_spec": control_spec
            });
            if let Some(vs_id) = value_slot_path_id {
                obj["value_slot_path"] = json!({ "id": vs_id.to_string() });
            }
            obj
        }
        CreateBriefUnitSubtypeSchema::Media {
            media_type,
            source_spec,
        } => json!({
            "kind": "media",
            "media_type": media_type.clone().unwrap_or_else(|| "image".to_string()),
            "source_spec": source_spec
        }),
        CreateBriefUnitSubtypeSchema::Output { display_format } => json!({
            "kind": "output",
            "display_format": display_format.clone().unwrap_or_else(|| "text".to_string())
        }),
    }
}

// Brief Units handlers (nested under briefs)
pub async fn brief_units_index_handler(
    Path(brief_id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let objs = store.get_by_parent(BRIEF_UNITS, "brief", brief_id);
    index_response(objs)
}

pub async fn brief_units_get_handler(
    Path((brief_id, unit_id)): Path<(Uuid, Uuid)>,
    State(state): State<Arc<MockState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;

    let unit = store
        .get_by_parent(BRIEF_UNITS, "brief", brief_id)
        .into_iter()
        .find(|u| {
            u.get("id")
                .and_then(|id| id.as_str())
                .map(|s| s == unit_id.to_string())
                .unwrap_or(false)
        });

    match unit {
        Some(obj) => get_response(obj),
        None => Err(not_found_response("Brief Unit")),
    }
}

pub async fn brief_units_post_handler(
    Path(brief_id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
    Json(body): Json<CreateBriefUnitSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let entities: Vec<Value> = body
        .objs
        .into_iter()
        .map(|s| {
            let outputs: Vec<Value> = s
                .outputs
                .iter()
                .map(|o| {
                    json!({
                        "source_kind": o.source_kind,
                        "source_spec": o.source_spec
                    })
                })
                .collect();

            json!({
                "brief": { "id": brief_id.to_string() },
                "unit_type": s.unit_type,
                "unit_key": s.unit_key,
                "ordinal": s.ordinal,
                "layout": if s.layout.is_null() { json!({}) } else { s.layout },
                "requires": if s.requires.is_null() { json!({}) } else { s.requires },
                "extensions": if s.extensions.is_null() { json!({}) } else { s.extensions },
                "outputs": outputs,
                "subtype": subtype_to_json(&s.subtype),
                "created_at": chrono::Utc::now().to_rfc3339()
            })
        })
        .collect();

    let inserted = store.insert(BRIEF_UNITS, entities);
    post_response(inserted)
}

pub async fn brief_units_patch_handler(
    Path(brief_id): Path<Uuid>,
    State(state): State<Arc<MockState>>,
    Json(body): Json<PatchBriefUnitSetSchema>,
) -> (StatusCode, Json<Value>) {
    let mut store = state.store.write().await;

    let mut updated = vec![];
    for patch in body.objs {
        let mut update_fields = json!({});

        if let Some(unit_key) = patch.unit_key {
            update_fields["unit_key"] = json!(unit_key);
        }
        if let Some(ordinal) = patch.ordinal {
            update_fields["ordinal"] = json!(ordinal);
        }
        if let Some(layout) = patch.layout {
            update_fields["layout"] = layout;
        }
        if let Some(requires) = patch.requires {
            update_fields["requires"] = requires;
        }
        if let Some(extensions) = patch.extensions {
            update_fields["extensions"] = extensions;
        }
        if let Some(outputs) = patch.outputs {
            let outputs_json: Vec<Value> = outputs
                .iter()
                .map(|o| {
                    json!({
                        "source_kind": o.source_kind,
                        "source_spec": o.source_spec
                    })
                })
                .collect();
            update_fields["outputs"] = json!(outputs_json);
        }
        if let Some(subtype) = &patch.subtype {
            let subtype_json = match subtype {
                PatchBriefUnitSubtypeSchema::Markdown { body } => json!({
                    "kind": "markdown",
                    "body": body.clone().unwrap_or_default()
                }),
                PatchBriefUnitSubtypeSchema::Notice { notice_type, body } => json!({
                    "kind": "notice",
                    "notice_type": notice_type.clone().unwrap_or_else(|| "info".to_string()),
                    "body": body.clone().unwrap_or_default()
                }),
                PatchBriefUnitSubtypeSchema::Input {
                    control_type,
                    label_text,
                    control_spec,
                    value_slot_path_id,
                } => {
                    let mut obj = json!({
                        "kind": "input",
                        "control_type": control_type.clone().unwrap_or_else(|| "text".to_string()),
                        "label_text": label_text.clone().unwrap_or_default(),
                        "control_spec": control_spec.clone().unwrap_or(json!({}))
                    });
                    if let Some(vs_id) = value_slot_path_id {
                        obj["value_slot_path"] = json!({ "id": vs_id.to_string() });
                    }
                    obj
                }
                PatchBriefUnitSubtypeSchema::Media {
                    media_type,
                    source_spec,
                } => json!({
                    "kind": "media",
                    "media_type": media_type.clone().unwrap_or_else(|| "image".to_string()),
                    "source_spec": source_spec.clone().unwrap_or(json!({}))
                }),
                PatchBriefUnitSubtypeSchema::Output { display_format } => json!({
                    "kind": "output",
                    "display_format": display_format.clone().unwrap_or_else(|| "text".to_string())
                }),
            };
            update_fields["subtype"] = subtype_json;
        }

        if let Some(unit) = store.update(BRIEF_UNITS, patch.id, update_fields) {
            // Verify this unit belongs to the correct brief
            if unit
                .get("brief")
                .and_then(|b| b.get("id"))
                .and_then(|id| id.as_str())
                == Some(&brief_id.to_string())
            {
                updated.push(unit);
            }
        }
    }

    patch_response(updated)
}
