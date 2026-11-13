use crate::{DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for a background job row.
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BackgroundJob {
    pub id: Uuid,
    pub job_kind: String,
    pub target_id: Uuid,
    #[schemars(with = "crate::JsonObject")]
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub error_message: Option<String>,
    #[schemars(with = "String")]
    pub scheduled_at: DateTime,
    #[schemars(with = "Option<String>")]
    pub claimed_at: Option<DateTime>,
    #[schemars(with = "Option<String>")]
    pub completed_at: Option<DateTime>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for BackgroundJob {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
