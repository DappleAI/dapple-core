use crate::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::{JsonValue, Uuid};
use time::OffsetDateTime;

#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
pub struct DispatchTaskAttemptEventModel {
    pub id: Uuid,
    pub attempt_id: Uuid,
    pub phase_code: String,
    pub context_data: Option<JsonValue>,
    pub context_metadata: Option<JsonValue>,
    pub context_temp_data: Option<JsonValue>,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize, JsonSchema)]
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
    #[schemars(with = "String")]
    pub created_at: OffsetDateTime,
}

impl crate::traits::DataTransferObject for DispatchTaskAttemptEvent {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl From<DispatchTaskAttemptEventModel> for DispatchTaskAttemptEvent {
    fn from(m: DispatchTaskAttemptEventModel) -> Self {
        DispatchTaskAttemptEvent {
            id: m.id,
            attempt_id: m.attempt_id,
            phase_code: m.phase_code,
            context_data: m.context_data,
            context_metadata: m.context_metadata,
            context_temp_data: m.context_temp_data,
            created_at: m.created_at,
        }
    }
}
