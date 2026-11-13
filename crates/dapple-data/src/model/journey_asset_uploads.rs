//! Models for staged operator uploads on a journey.
//!
//! `rev.journey_asset_uploads` records "asset X was uploaded for input I of
//! journey J" during the pre-submit window; the real asset↔slot binding is
//! still written at submit time via dispatch-intent-bindings. Append-only with
//! a `_removed` companion table (favorites precedent).

use crate::ContentHash;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::journey_asset_uploads::*;

/// Query result row (INSERT...RETURNING and by-journey SELECT share the same
/// shape: upload columns plus asset metadata joined in).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JourneyAssetUploadModel {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub asset_id: Uuid,
    pub input_key: String,
    pub value_slot_id: Option<Uuid>,
    pub file_name: String,
    pub mime_type: String,
    pub content_length: i64,
    pub content_hash: String,
    pub created_at: OffsetDateTime,
}

impl From<JourneyAssetUploadModel> for JourneyAssetUpload {
    fn from(m: JourneyAssetUploadModel) -> Self {
        Self {
            id: m.id,
            journey_id: m.journey_id,
            asset_id: m.asset_id,
            input_key: m.input_key,
            value_slot_id: m.value_slot_id,
            file_name: m.file_name,
            mime_type: m.mime_type,
            content_length: m.content_length,
            content_hash: ContentHash::new(m.content_hash),
            created_at: m.created_at.into(),
        }
    }
}

/// Model returned from the soft-remove INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RemovedJourneyAssetUploadModel {
    pub journey_asset_upload_id: Uuid,
    pub removed_at: OffsetDateTime,
}

/// Column-wise insert arguments. All vectors must have identical length.
#[derive(Clone, Debug)]
pub struct InsertJourneyAssetUploadArgs {
    pub journey_id: Uuid,
    pub asset_ids: Vec<Uuid>,
    pub file_names: Vec<String>,
    pub input_keys: Vec<String>,
}
