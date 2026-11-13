use dapple_data::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use sqlx::types::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct DispatchTaskAttempt {
    pub id: Uuid,
    pub dispatch_step_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    #[schemars(with = "JsonObject")]
    pub context_data: JsonValue,
    #[schemars(with = "JsonObject")]
    pub context_metadata: JsonValue,
    // created_at omitted from API DTO
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct DispatchTaskAttemptEvent {
    pub id: Uuid,
    pub attempt_id: Uuid,
    pub phase_code: String,
    #[schemars(with = "Option<JsonObject>")]
    pub context_data: Option<JsonValue>,
    #[schemars(with = "Option<JsonObject>")]
    pub context_metadata: Option<JsonValue>,
    #[schemars(with = "Option<JsonObject>")]
    pub context_temp_data: Option<JsonValue>,
    // created_at omitted from API DTO
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateDispatchTaskAttempt {
    pub dispatch_step_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    #[schemars(with = "JsonObject")]
    pub context_data: JsonValue,
    #[schemars(with = "JsonObject")]
    pub context_metadata: JsonValue,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateDispatchTaskAttemptEvent {
    pub phase_code: String,
    #[schemars(with = "Option<JsonObject>")]
    pub context_data: Option<JsonValue>,
    #[schemars(with = "Option<JsonObject>")]
    pub context_metadata: Option<JsonValue>,
    #[schemars(with = "Option<JsonObject>")]
    pub context_temp_data: Option<JsonValue>,
}
