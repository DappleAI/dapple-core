//! Query functions for Lago event synchronization.

use crate::DbPool;
use crate::error::DataError;
use crate::model::lago_event_sync::*;
use sqlx::types::JsonValue;
use time::OffsetDateTime;
use uuid::Uuid;

/// Batch insert Lago event sync records.
/// Also fires a pg_notify on the 'lago_event_sync' channel per inserted row.
pub async fn insert_lago_event_syncs(
    pool: &DbPool,
    args: InsertLagoEventSyncArgs,
) -> Result<Vec<InsertedLagoEventSyncModel>, DataError> {
    let rows = sqlx::query_file_as!(
        InsertedLagoEventSyncModel,
        "assets/.sql/lago_event_sync/insert_lago_event_syncs.sql",
        // Alphabetical bind order: lago_transaction_ids, resource_ids, resource_type_ids
        &args.lago_transaction_ids,
        &args.resource_ids,
        &args.resource_type_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Select pending (unsent) Lago event sync records ready for delivery.
pub async fn select_pending_syncs(
    pool: &DbPool,
    limit: i64,
) -> Result<Vec<PendingSyncModel>, DataError> {
    let rows = sqlx::query_file_as!(
        PendingSyncModel,
        "assets/.sql/lago_event_sync/select_pending_syncs.sql",
        // Single bind: limit
        limit,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Update a Lago event sync record after attempting to send.
/// This is one of the rare justified UPDATEs (operational sync state, not domain data).
pub async fn update_lago_event_sync_result(
    pool: &DbPool,
    id: Uuid,
    success: bool,
    lago_response_status: Option<i32>,
    lago_response_body: Option<JsonValue>,
    next_retry_at: Option<OffsetDateTime>,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/lago_event_sync/update_lago_event_sync_result.sql",
        // Alphabetical bind order: id, lago_response_body, lago_response_status, next_retry_at, success
        id,
        lago_response_body,
        lago_response_status,
        next_retry_at,
        success,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(())
}
