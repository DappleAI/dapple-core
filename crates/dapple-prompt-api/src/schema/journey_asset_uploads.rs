//! Schemas for staged operator uploads (`/api/journeys/{id}/uploads`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One staged upload: an already-created asset attached to a journey input
/// during the pre-submit window.
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
pub struct CreateJourneyAssetUploadSchema {
    /// `rev.assets.id` of the uploaded asset (created via `POST /api/assets/`).
    pub asset_id: Uuid,
    /// Original file name, for display on rehydrated file fields.
    pub file_name: String,
    /// Raw client input identifier: the journey-pinned value-slot id for
    /// top-level brief fields, or a synthetic composite for repeater rows.
    pub input_key: String,
}

/// Batch wrapper for staged-upload creation.
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
pub struct CreateJourneyAssetUploadSetSchema {
    pub objs: Vec<CreateJourneyAssetUploadSchema>,
}
