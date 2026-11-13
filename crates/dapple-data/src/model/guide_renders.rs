//! Models for the guide render cache.
//!
//! Tracks rendered PDFs stored as assets, keyed by guide version
//! and render profile.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Internal model for a cached guide render row.
#[derive(Debug, Clone)]
pub struct GuideRenderModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub render_profile_id: i16,
    pub asset_id: Uuid,
    pub created_at: OffsetDateTime,
}

/// Public DTO for a cached guide render.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideRender {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub created_at: OffsetDateTime,
}

impl From<GuideRenderModel> for GuideRender {
    fn from(m: GuideRenderModel) -> Self {
        Self {
            id: m.id,
            asset_id: m.asset_id,
            created_at: m.created_at,
        }
    }
}

/// Internal model for an inserted guide render row.
#[derive(Debug, Clone)]
pub struct InsertedGuideRenderModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub render_profile_id: i16,
    pub asset_id: Uuid,
    pub created_at: OffsetDateTime,
}
