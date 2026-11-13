//! Query functions for billable entities.

use crate::DbPool;
use crate::error::DataError;
use crate::model::billable_entities::*;

/// Insert a user's billable entity (creates both billable_entity and user_billable_entity).
pub async fn insert_user_billable_entity(
    pool: &DbPool,
    args: InsertUserBillableEntityArgs,
) -> Result<Vec<InsertedBillableEntityModel>, DataError> {
    let rows = sqlx::query_file_as!(
        InsertedBillableEntityModel,
        "assets/.sql/billable_entities/insert_user_billable_entity.sql",
        // Alphabetical bind order: lago_customer_external_ids, lago_subscription_external_ids, user_ids
        &args.lago_customer_external_ids as &[Option<String>],
        &args.lago_subscription_external_ids as &[Option<String>],
        &args.user_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Select a user's billable entity by user_id.
pub async fn select_billable_entity_by_user_id(
    pool: &DbPool,
    user_id: i64,
) -> Result<Option<UserBillableEntityModel>, DataError> {
    let row = sqlx::query_file_as!(
        UserBillableEntityModel,
        "assets/.sql/billable_entities/select_billable_entity_by_user_id.sql",
        // Single bind: user_id
        user_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}
