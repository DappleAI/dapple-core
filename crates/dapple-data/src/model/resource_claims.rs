//! Models for resource claims.
//!
//! Resource claims record that a billable resource was consumed during
//! dispatch task execution. Each claim references a resource_type and
//! optionally a dispatch_task_attempt and correlation_id.

use serde::{Deserialize, Serialize};
use sqlx::types::{JsonValue, Uuid};
use time::OffsetDateTime;

/// Internal model for a resource claim.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResourceClaimModel {
    pub id: Uuid,
    pub resource_type_id: i64,
    pub dispatch_task_attempt_id: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
    pub claim_data: JsonValue,
    pub created_at: OffsetDateTime,
}

/// Column-wise insert arguments for resource claims.
#[derive(Clone, Debug)]
pub struct InsertResourceClaimArgs {
    pub resource_type_ids: Vec<i64>,
    pub dispatch_task_attempt_ids: Vec<Option<Uuid>>,
    pub correlation_ids: Vec<Option<Uuid>>,
    pub claim_data: Vec<JsonValue>,
}

/// INSERT RETURNING result for resource claims.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedResourceClaimModel {
    pub id: Uuid,
    pub resource_type_id: i64,
    pub created_at: OffsetDateTime,
}
