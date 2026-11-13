use crate::DateTime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::{JsonValue, Uuid};
use time::OffsetDateTime;

// ============================================================================
// InsertPromptResponseArgs (column-wise insert parameters)
// ============================================================================

/// Column-wise insert arguments for prompt responses
#[derive(Clone, Debug)]
pub struct InsertPromptResponseArgs {
    pub attempt_ids: Vec<Uuid>,
    pub prompt_ids: Vec<Uuid>,
    pub step_task_ids: Vec<Uuid>,
    pub maker_keys: Vec<String>,
    pub engine_keys: Vec<String>,
    pub engine_versions: Vec<Option<String>>,
    pub request_messages: Vec<JsonValue>,
    pub response_contents: Vec<String>,
    pub input_tokens: Vec<i32>,
    pub output_tokens: Vec<i32>,
    pub latency_ms: Vec<i32>,
    pub finish_reasons: Vec<String>,
    pub started_ats: Vec<OffsetDateTime>,
    pub completed_ats: Vec<OffsetDateTime>,
}

// ============================================================================
// InsertedPromptResponseModel (INSERT...RETURNING result)
// ============================================================================

/// Result from INSERT...RETURNING for prompt response
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedPromptResponseModel {
    pub id: Uuid,
    pub attempt_id: Uuid,
    pub prompt_id: Uuid,
    pub step_task_id: Uuid,
    pub maker_key: String,
    pub engine_key: String,
    pub engine_version: Option<String>,
    pub request_messages: JsonValue,
    pub response_content: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub latency_ms: i32,
    pub finish_reason: String,
    pub started_at: OffsetDateTime,
    pub completed_at: OffsetDateTime,
}

// ============================================================================
// PromptResponse (public DTO)
// ============================================================================

/// Public DTO for prompt response
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq, Eq)]
pub struct PromptResponse {
    pub id: Uuid,
    pub attempt_id: Uuid,
    pub prompt_id: Uuid,
    pub step_task_id: Uuid,
    pub maker_key: String,
    pub engine_key: String,
    pub engine_version: Option<String>,
    pub request_messages: JsonValue,
    pub response_content: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub latency_ms: i32,
    pub finish_reason: String,
    #[schemars(with = "String")]
    pub started_at: DateTime,
    #[schemars(with = "String")]
    pub completed_at: DateTime,
}

impl From<InsertedPromptResponseModel> for PromptResponse {
    fn from(m: InsertedPromptResponseModel) -> Self {
        PromptResponse {
            id: m.id,
            attempt_id: m.attempt_id,
            prompt_id: m.prompt_id,
            step_task_id: m.step_task_id,
            maker_key: m.maker_key,
            engine_key: m.engine_key,
            engine_version: m.engine_version,
            request_messages: m.request_messages,
            response_content: m.response_content,
            input_tokens: m.input_tokens,
            output_tokens: m.output_tokens,
            latency_ms: m.latency_ms,
            finish_reason: m.finish_reason,
            started_at: m.started_at.into(),
            completed_at: m.completed_at.into(),
        }
    }
}

impl crate::traits::DataTransferObject for PromptResponse {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
