//! Models for user favorite guides.

use crate::model::guides::GuideRef;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::user_favorite_guides::*;

/// Internal model with all database fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserFavoriteGuideModel {
    pub id: Uuid,
    pub user_id: Uuid,
    pub guide_path_id: Uuid,
    pub created_at: OffsetDateTime,
}

/// Model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedUserFavoriteGuideModel {
    pub id: Uuid,
    pub user_id: Uuid,
    pub guide_path_id: Uuid,
    pub guide_title: String,
    pub created_at: OffsetDateTime,
}

/// Model returned from remove favorite INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedUserFavoriteGuideRemovedModel {
    pub user_favorite_guides_id: Uuid,
    pub removed_at: OffsetDateTime,
}

/// Model for checking if a guide is favorited.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IsFavoritedModel {
    pub is_favorited: Option<bool>,
}

/// Query result with guide path id and title for DTO conversion.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserFavoriteGuideWithObjectIdModel {
    pub id: Uuid,
    pub user_id: Uuid,
    pub guide_path_id: Uuid,
    pub guide_title: String,
    pub created_at: OffsetDateTime,
}

impl From<UserFavoriteGuideWithObjectIdModel> for UserFavoriteGuide {
    fn from(m: UserFavoriteGuideWithObjectIdModel) -> Self {
        Self {
            guide: GuideRef {
                id: m.guide_path_id,
                title: m.guide_title,
            },
            created_at: m.created_at.into(),
        }
    }
}
