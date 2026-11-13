use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// ============================================================================
// Guide Resource Models
// ============================================================================

/// Internal model for guide resources (SELECT result).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideResourceModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub asset_id: Option<Uuid>,
    pub key: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub url: Option<String>,
    pub retrieved_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

/// INSERT...RETURNING result for guide resources.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideResourceModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub key: String,
    pub created_at: OffsetDateTime,
}

/// Column-wise insert arguments for guide resources.
/// Each vector element corresponds to one resource in the batch.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideResourceArgs {
    pub guide_ids: Vec<Uuid>,
    pub asset_ids: Vec<Option<Uuid>>,
    pub keys: Vec<String>,
    pub titles: Vec<Option<String>>,
    pub authors: Vec<Option<String>>,
    pub urls: Vec<Option<String>>,
    pub retrieved_ats: Vec<Option<OffsetDateTime>>,
}

impl_insert_column_args!(
    InsertGuideResourceArgs,
    [
        guide_ids,
        asset_ids,
        keys,
        titles,
        authors,
        urls,
        retrieved_ats
    ]
);

// ============================================================================
// Guide Block Citation Models
// ============================================================================

/// INSERT...RETURNING result for guide block citations.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideBlockCitationModel {
    pub id: Uuid,
    pub block_id: Uuid,
    pub resource_id: Uuid,
    pub pages: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Column-wise insert arguments for guide block citations.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideBlockCitationArgs {
    pub block_ids: Vec<Uuid>,
    pub resource_ids: Vec<Uuid>,
    pub pages: Vec<Option<String>>,
}

impl_insert_column_args!(
    InsertGuideBlockCitationArgs,
    [block_ids, resource_ids, pages]
);
