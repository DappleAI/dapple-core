//! Query functions for resource claims.

use crate::DbPool;
use crate::error::DataError;
use crate::model::resource_claims::*;

/// Batch insert resource claims.
pub async fn insert_resource_claims(
    pool: &DbPool,
    args: InsertResourceClaimArgs,
) -> Result<Vec<InsertedResourceClaimModel>, DataError> {
    let rows = sqlx::query_file_as!(
        InsertedResourceClaimModel,
        "assets/.sql/resource_claims/insert_resource_claims.sql",
        // Alphabetical bind order: claim_data, correlation_ids, dispatch_task_attempt_ids, resource_type_ids
        &args.claim_data,
        &args.correlation_ids as &[Option<uuid::Uuid>],
        &args.dispatch_task_attempt_ids as &[Option<uuid::Uuid>],
        &args.resource_type_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}
