//! Models for LLM usage records.
//!
//! Tracks token consumption per dispatch task attempt event for billing
//! and analytics. Each record links to an event, an optional resource claim,
//! and resolves to a specific provider + model release.

use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Internal model for LLM usage records.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LlmUsageModel {
    pub id: Uuid,
    pub event_id: Uuid,
    pub claim_id: Option<Uuid>,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub provider_input_tokens: Option<i32>,
    pub provider_output_tokens: Option<i32>,
    pub provider_id: i16,
    pub model_release_id: Uuid,
    pub created_at: OffsetDateTime,
}

/// Column-wise insert arguments for LLM usage.
#[derive(Clone, Debug)]
pub struct InsertLlmUsageArgs {
    pub event_ids: Vec<Uuid>,
    pub claim_ids: Vec<Option<Uuid>>,
    pub input_tokens: Vec<i32>,
    pub output_tokens: Vec<i32>,
    pub provider_input_tokens: Vec<Option<i32>>,
    pub provider_output_tokens: Vec<Option<i32>>,
    pub provider_ids: Vec<i16>,
    pub model_release_ids: Vec<Uuid>,
}

/// INSERT RETURNING result for LLM usage.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedLlmUsageModel {
    pub id: Uuid,
    pub event_id: Uuid,
    pub created_at: OffsetDateTime,
}
