//! Models for Lago event synchronization.
//!
//! Tracks the lifecycle of billing events sent to Lago:
//! creation -> send attempt -> success/retry. The UPDATE on this table
//! is one of the rare justified exceptions to the append-only pattern,
//! as it represents operational sync state rather than domain data.

use serde::{Deserialize, Serialize};
use sqlx::types::{JsonValue, Uuid};
use time::OffsetDateTime;

/// Internal model for Lago event sync records.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LagoEventSyncModel {
    pub id: Uuid,
    pub resource_type_id: i64,
    pub resource_id: Uuid,
    pub lago_transaction_id: Uuid,
    pub sent_at: Option<OffsetDateTime>,
    pub lago_response_status: Option<i32>,
    pub lago_response_body: Option<JsonValue>,
    pub retry_count: i32,
    pub next_retry_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

/// Column-wise insert arguments for Lago event sync records.
#[derive(Clone, Debug)]
pub struct InsertLagoEventSyncArgs {
    pub resource_type_ids: Vec<i64>,
    pub resource_ids: Vec<Uuid>,
    pub lago_transaction_ids: Vec<Uuid>,
}

/// INSERT RETURNING result for Lago event sync records.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedLagoEventSyncModel {
    pub id: Uuid,
    pub resource_type_id: i64,
    pub resource_id: Uuid,
    pub lago_transaction_id: Uuid,
    pub created_at: OffsetDateTime,
}

/// Pending sync record (not yet sent to Lago).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PendingSyncModel {
    pub id: Uuid,
    pub resource_type_id: i64,
    pub resource_id: Uuid,
    pub lago_transaction_id: Uuid,
    pub retry_count: i32,
    pub created_at: OffsetDateTime,
}
