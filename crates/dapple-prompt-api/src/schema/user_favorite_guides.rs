//! Request and response schemas for user favorite guides.

use dapple_data::model::guides::GuideRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Response for `POST /api/guides/{id}/favorite`.
/// Returned when a guide is added to favorites.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct AddFavoriteGuideResponse {
    pub status: String,
    pub guide: GuideRef,
    pub created_at: String,
}

/// Response for `DELETE /api/guides/{id}/favorite`.
/// Returned when a guide is removed from favorites.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct RemoveFavoriteGuideResponse {
    pub status: String,
    pub message: String,
}

/// Response for `GET /api/guides/{id}/favorite`.
/// Returns whether the guide is favorited.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CheckFavoriteGuideResponse {
    pub status: String,
    pub is_favorited: bool,
}

/// Response for `GET /api/guides/favorites`.
/// Lists all favorite guides for the authenticated user.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ListFavoriteGuidesResponse {
    pub status: String,
    pub results: usize,
    pub objs: Vec<FavoriteGuideInfo>,
}

/// Information about a favorited guide.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct FavoriteGuideInfo {
    pub guide: GuideRef,
    /// When the guide was favorited (ISO 8601).
    pub created_at: String,
}

/// Error response for favorite operations.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct FavoriteErrorResponse {
    pub error: String,
    pub message: String,
}

impl FavoriteErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}
