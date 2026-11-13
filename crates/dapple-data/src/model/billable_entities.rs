//! Models for billable entities and user-to-billable-entity associations.
//!
//! Billable entities represent the billing identity for metered usage.
//! Each user is linked to exactly one billable entity via user_billable_entities.

use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Internal model for a billable entity.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BillableEntityModel {
    pub id: Uuid,
    pub lago_customer_external_id: Option<String>,
    pub lago_subscription_external_id: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Billable entity resolved from a user (JOIN across user_billable_entities + billable_entities).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserBillableEntityModel {
    pub id: Uuid,
    pub billable_entity_id: Uuid,
    pub user_id: i64,
    pub lago_customer_external_id: Option<String>,
    pub lago_subscription_external_id: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Column-wise insert arguments for creating a user's billable entity.
/// Creates both the billable_entity and user_billable_entity in a single CTE.
#[derive(Clone, Debug)]
pub struct InsertUserBillableEntityArgs {
    pub user_ids: Vec<i64>,
    pub lago_customer_external_ids: Vec<Option<String>>,
    pub lago_subscription_external_ids: Vec<Option<String>>,
}

/// INSERT RETURNING result for user billable entities.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedBillableEntityModel {
    pub id: Uuid,
    pub billable_entity_id: Uuid,
    pub user_id: i64,
    pub lago_customer_external_id: Option<String>,
    pub lago_subscription_external_id: Option<String>,
    pub created_at: OffsetDateTime,
}
