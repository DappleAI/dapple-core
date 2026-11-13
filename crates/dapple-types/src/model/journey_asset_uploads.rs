use crate::{ContentHash, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for a staged operator upload on a journey.
///
/// A durable record of "asset X was uploaded for input `input_key` of journey
/// J before the form was submitted". The asset↔slot *binding* is still written
/// at submit time via dispatch-intent-bindings; this record covers the
/// pre-submit window so clients can rehydrate file fields after a reload and
/// recover the asset id at submit when their local cache was evicted.
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct JourneyAssetUpload {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub asset_id: Uuid,
    /// Raw client input identifier: the journey-pinned `value_slots.id` for
    /// top-level brief fields, or a synthetic composite for repeater rows.
    pub input_key: String,
    /// Resolved when `input_key` is a bare value-slot UUID; `None` for
    /// synthetic keys.
    pub value_slot_id: Option<Uuid>,
    pub file_name: String,
    pub mime_type: String,
    pub content_length: i64,
    pub content_hash: ContentHash,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}
