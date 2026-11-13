//! Request/response schemas for the external-surface `GET /api/value-slots`.
//!
//! Two query shapes are supported:
//!   * Bare `?page=&limit=` lists every value_slot in the caller's namespace.
//!   * `?query=by_object_ids&ids=<uuid>&ids=<uuid>...` batch-fetches
//!     metadata for specific value_slots — typically the slot UUIDs that
//!     came back on a prior `GET /api/journeys/{id}/brief-form` response.
//!     Foreign IDs are silently dropped from the result (no existence leak).
//!
//! The `by_object_ids` variant matches the internal surface's
//! `GetValueSlotSetSchema::ByObjectIds` so the wire vocabulary is the
//! same across both surfaces.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Listing variant: bare paging over the caller's namespace.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ListExternalValueSlotsQuery {
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub page: Option<u32>,
}

/// Tagged-enum query shape that the index handler routes on. The `query`
/// discriminator matches the internal surface's `GetValueSlotSetSchema`.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetExternalValueSlotSetSchema {
    /// Batch-fetch slots by ID. IDs not in the caller's namespace are
    /// silently dropped.
    ByObjectIds {
        #[serde(default)]
        ids: Vec<Uuid>,
    },
}
