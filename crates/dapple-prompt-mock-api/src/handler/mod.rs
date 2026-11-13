pub mod assets;
pub mod auth_token;
pub mod brief_groups;
pub mod brief_packs;
pub mod brief_units;
pub mod briefs;
pub mod categories;
pub mod dispatch_intent_bindings;
pub mod dispatches;
pub mod fluent_messages;
pub mod hats;
pub mod journeys;
pub mod mock_utils;
pub mod playbooks;
pub mod prompts;
pub mod stages;
pub mod steps;
pub mod task_prompts;
pub mod tasks;
pub mod value_slots;

use axum::Json;
use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Health check response
#[derive(Serialize, JsonSchema)]
pub struct HealthResponse {
    pub status: String,
    pub message: String,
}

/// Health check handler
pub async fn health_checker_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "success".into(),
        message: "Mock API server for dapple-prompt".into(),
    })
}

/// GET single object response envelope
#[derive(Clone, Eq, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct GetResponse<O> {
    pub status: String,
    pub results: usize,
    pub obj: O,
}

/// GET multiple objects response envelope
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct IndexResponse<O> {
    pub status: String,
    pub results: usize,
    pub objs: Vec<O>,
}

/// POST response envelope
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PostResponse<O> {
    pub status: String,
    pub results: usize,
    pub objs: Vec<O>,
}

/// Error response
#[derive(Serialize, JsonSchema)]
pub struct ErrorResponse {
    pub status: String,
    pub message: String,
}

/// Helper to create a successful GET response for a single object
pub fn get_response(obj: Value) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    Ok(Json(json!({
        "status": "success",
        "results": 1,
        "obj": obj
    })))
}

/// Helper to create a not found error response
pub fn not_found_response(entity_type: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "status": "fail",
            "message": format!("{} not found", entity_type)
        })),
    )
}

/// Helper to create a successful index response for multiple objects
pub fn index_response(objs: Vec<Value>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if objs.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "status": "fail",
                "message": "No items found"
            })),
        ));
    }

    Ok(Json(json!({
        "status": "success",
        "results": objs.len(),
        "objs": objs
    })))
}

/// Helper to create a successful POST response
pub fn post_response(objs: Vec<Value>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::CREATED,
        Json(json!({
            "status": "success",
            "results": objs.len(),
            "objs": objs
        })),
    )
}

/// Helper to create a successful PATCH response
pub fn patch_response(objs: Vec<Value>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "results": objs.len(),
            "objs": objs
        })),
    )
}
