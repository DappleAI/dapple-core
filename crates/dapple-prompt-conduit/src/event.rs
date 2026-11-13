use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// SSE events for prompt streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptEvent {
    /// Prompt execution has started
    Started {
        request_id: Uuid,
        model: String,
        prompt_id: Uuid,
    },

    /// A token was generated
    Token { content: String },

    /// Prompt execution completed successfully
    Done {
        output_tokens: u32,
        input_tokens: u32,
        latency_ms: u64,
        finish_reason: String,
    },

    /// An error occurred
    Error {
        code: String,
        message: String,
        retryable: bool,
    },

    /// An asset was created (for TTS and other binary outputs)
    Asset {
        asset_id: Uuid,
        asset_kind: String,
        mime_type: String,
        content_length: i64,
        duration_seconds: f32,
        sample_rate: u32,
        text: String,
    },
}

impl PromptEvent {
    /// Create a Started event
    pub fn started(request_id: Uuid, model: impl Into<String>, prompt_id: Uuid) -> Self {
        Self::Started {
            request_id,
            model: model.into(),
            prompt_id,
        }
    }

    /// Create a Token event
    pub fn token(content: impl Into<String>) -> Self {
        Self::Token {
            content: content.into(),
        }
    }

    /// Create a Done event
    pub fn done(
        output_tokens: u32,
        input_tokens: u32,
        latency_ms: u64,
        finish_reason: impl Into<String>,
    ) -> Self {
        Self::Done {
            output_tokens,
            input_tokens,
            latency_ms,
            finish_reason: finish_reason.into(),
        }
    }

    /// Create an Error event
    pub fn error(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
            retryable,
        }
    }

    /// Create an Asset event
    pub fn asset(
        asset_id: Uuid,
        asset_kind: impl Into<String>,
        mime_type: impl Into<String>,
        content_length: i64,
        duration_seconds: f32,
        sample_rate: u32,
        text: impl Into<String>,
    ) -> Self {
        Self::Asset {
            asset_id,
            asset_kind: asset_kind.into(),
            mime_type: mime_type.into(),
            content_length,
            duration_seconds,
            sample_rate,
            text: text.into(),
        }
    }

    /// Serialize to JSON for SSE
    pub fn to_sse_data(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|e| {
            format!(r#"{{"type":"error","code":"serialization_error","message":"{}","retryable":false}}"#, e)
        })
    }
}
