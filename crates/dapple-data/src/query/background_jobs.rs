use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::background_jobs::{BackgroundJobModel, InsertBackgroundJobArgs};
use sqlx::types::Uuid;

// ============================================================================
// INSERT
// ============================================================================

/// Batch insert background jobs in the `queued` state.
pub async fn insert_background_jobs(
    pool: &DbPool,
    args: InsertBackgroundJobArgs,
) -> Result<Vec<BackgroundJobModel>, DataError> {
    if args.job_kinds.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.job_kinds.len() != args.target_ids.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "target_ids".into(),
            expected: args.job_kinds.len(),
            found: args.target_ids.len(),
        }
        .into());
    }

    if args.job_kinds.len() != args.payloads.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "payloads".into(),
            expected: args.job_kinds.len(),
            found: args.payloads.len(),
        }
        .into());
    }

    let rows = sqlx::query_file_as!(
        BackgroundJobModel,
        "assets/.sql/background_jobs/insert_background_jobs.sql",
        &args.job_kinds,
        &args.target_ids,
        &args.payloads,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// Claim / transitions
// ============================================================================

/// Atomically claim the next queued background job. When `job_kinds` is empty
/// the claim matches any kind; otherwise only rows with matching `job_kind` are
/// considered. Returns `Ok(None)` when no queued job is ready.
pub async fn claim_next_background_job(
    pool: &DbPool,
    job_kinds: &[String],
) -> Result<Option<BackgroundJobModel>, DataError> {
    let row = sqlx::query_file_as!(
        BackgroundJobModel,
        "assets/.sql/background_jobs/claim_next_background_job.sql",
        job_kinds,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Mark a background job as successfully completed.
pub async fn mark_background_job_ok(
    pool: &DbPool,
    id: Uuid,
) -> Result<Option<BackgroundJobModel>, DataError> {
    let row = sqlx::query_file_as!(
        BackgroundJobModel,
        "assets/.sql/background_jobs/mark_background_job_ok.sql",
        id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// Mark a background job as failed. Automatically retries with exponential
/// backoff when `attempts < max_attempts`.
pub async fn mark_background_job_failed(
    pool: &DbPool,
    id: Uuid,
    error_message: &str,
) -> Result<Option<BackgroundJobModel>, DataError> {
    let row = sqlx::query_file_as!(
        BackgroundJobModel,
        "assets/.sql/background_jobs/mark_background_job_failed.sql",
        id,
        error_message,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
