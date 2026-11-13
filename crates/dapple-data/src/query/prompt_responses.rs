use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::prompt_responses::{
    InsertPromptResponseArgs, InsertedPromptResponseModel, PromptResponse,
};
use sqlx::types::Uuid;

/// Insert prompt responses after LLM streaming completes
pub async fn insert_prompt_responses(
    pool: &DbPool,
    args: InsertPromptResponseArgs,
) -> Result<Vec<InsertedPromptResponseModel>, DataError> {
    if args.attempt_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = args.attempt_ids.len();

    // Validate all vectors have same length
    let checks = [
        ("prompt_ids", args.prompt_ids.len()),
        ("step_task_ids", args.step_task_ids.len()),
        ("maker_keys", args.maker_keys.len()),
        ("engine_keys", args.engine_keys.len()),
        ("engine_versions", args.engine_versions.len()),
        ("request_messages", args.request_messages.len()),
        ("response_contents", args.response_contents.len()),
        ("input_tokens", args.input_tokens.len()),
        ("output_tokens", args.output_tokens.len()),
        ("latency_ms", args.latency_ms.len()),
        ("finish_reasons", args.finish_reasons.len()),
        ("started_ats", args.started_ats.len()),
        ("completed_ats", args.completed_ats.len()),
    ];

    for (column, found) in checks {
        if found != expected_len {
            return Err(InsertArgColumnMismatch::InvalidColumnSize {
                column: column.into(),
                expected: expected_len,
                found,
            }
            .into());
        }
    }

    let inserted = sqlx::query_file_as!(
        InsertedPromptResponseModel,
        "assets/.sql/prompt_responses/insert_prompt_response.sql",
        &args.attempt_ids,
        &args.prompt_ids,
        &args.step_task_ids,
        &args.maker_keys,
        &args.engine_keys,
        &args.engine_versions as &[Option<String>],
        &args.request_messages,
        &args.response_contents,
        &args.input_tokens,
        &args.output_tokens,
        &args.latency_ms,
        &args.finish_reasons,
        &args.started_ats,
        &args.completed_ats
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted)
}

/// Insert a single prompt response (convenience wrapper)
pub async fn insert_prompt_response(
    pool: &DbPool,
    args: InsertPromptResponseArgs,
) -> Result<PromptResponse, DataError> {
    let inserted = insert_prompt_responses(pool, args).await?;
    inserted
        .into_iter()
        .next()
        .map(|m| m.into())
        .ok_or_else(|| DataError::Custom("No prompt response inserted".into()))
}

/// Select every prompt response recorded for a dispatch, in execution order.
/// Each row is one `prompt send` call: the rendered conduit request, the LLM
/// response, and per-call token/latency metrics.
pub async fn select_prompt_responses_by_dispatch_id(
    pool: &DbPool,
    dispatch_id: Uuid,
) -> Result<Vec<PromptResponse>, DataError> {
    let rows = sqlx::query_file_as!(
        InsertedPromptResponseModel,
        "assets/.sql/prompt_responses/select_by_dispatch_object_id.sql",
        dispatch_id
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// Select every prompt response recorded for a single task attempt, in
/// execution order.
pub async fn select_prompt_responses_by_attempt_id(
    pool: &DbPool,
    attempt_id: Uuid,
) -> Result<Vec<PromptResponse>, DataError> {
    let rows = sqlx::query_file_as!(
        InsertedPromptResponseModel,
        "assets/.sql/prompt_responses/select_by_attempt_object_id.sql",
        attempt_id
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}
