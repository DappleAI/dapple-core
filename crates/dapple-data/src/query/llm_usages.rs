//! Query functions for LLM usage records.

use crate::DbPool;
use crate::error::DataError;
use crate::model::llm_usages::*;
use uuid::Uuid;

/// Batch insert LLM usage records.
pub async fn insert_llm_usages(
    pool: &DbPool,
    args: InsertLlmUsageArgs,
) -> Result<Vec<InsertedLlmUsageModel>, DataError> {
    let rows = sqlx::query_file_as!(
        InsertedLlmUsageModel,
        "assets/.sql/llm_usages/insert_llm_usages.sql",
        // Alphabetical bind order: claim_ids, event_ids, input_tokens, model_release_ids,
        // output_tokens, provider_ids, provider_input_tokens, provider_output_tokens
        &args.claim_ids as &[Option<Uuid>],
        &args.event_ids,
        &args.input_tokens,
        &args.model_release_ids,
        &args.output_tokens,
        &args.provider_ids,
        &args.provider_input_tokens as &[Option<i32>],
        &args.provider_output_tokens as &[Option<i32>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Select LLM usage records by event IDs.
pub async fn select_llm_usages_by_event_ids(
    pool: &DbPool,
    event_ids: &[Uuid],
) -> Result<Vec<LlmUsageModel>, DataError> {
    let rows = sqlx::query_file_as!(
        LlmUsageModel,
        "assets/.sql/llm_usages/select_llm_usages_by_event_ids.sql",
        // Single bind: event_ids
        event_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}
