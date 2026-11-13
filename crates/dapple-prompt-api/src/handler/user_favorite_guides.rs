//! Handlers for user favorite guides.

use crate::extract_path_uuid::ValidatedPathUuid;
use crate::schema::user_favorite_guides::{
    AddFavoriteGuideResponse, CheckFavoriteGuideResponse, FavoriteErrorResponse, FavoriteGuideInfo,
    ListFavoriteGuidesResponse, RemoveFavoriteGuideResponse,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::FilterOptions;
use dapple_data::model::guides::GuideRef;
use dapple_data::query::user_favorite_guides::{
    check_favorite, insert_favorite, remove_favorite, select_favorites_by_user,
};
use std::sync::Arc;
use time::OffsetDateTime;

/// Convert time::OffsetDateTime to ISO 8601 string
fn format_time(dt: OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

/// Handler for `POST /api/guides/{id}/favorite`.
/// Adds a guide to the user's favorites.
pub async fn add_favorite_guide_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
    ValidatedPathUuid(guide_path_id): ValidatedPathUuid,
) -> impl IntoApiResponse {
    match insert_favorite(&state.db, user.user_id(), guide_path_id).await {
        Ok(Some(inserted)) => (
            StatusCode::CREATED,
            Json(AddFavoriteGuideResponse {
                status: "success".to_string(),
                guide: GuideRef {
                    id: inserted.guide_path_id,
                    title: inserted.guide_title,
                },
                created_at: format_time(inserted.created_at),
            }),
        )
            .into_response(),
        Ok(None) => {
            // Either already favorited or guide not found
            match check_favorite(&state.db, user.user_id(), guide_path_id).await {
                Ok(true) => (
                    StatusCode::OK,
                    Json(AddFavoriteGuideResponse {
                        status: "success".to_string(),
                        guide: GuideRef {
                            id: guide_path_id,
                            title: String::new(),
                        },
                        created_at: "".to_string(),
                    }),
                )
                    .into_response(),
                Ok(false) => (
                    StatusCode::NOT_FOUND,
                    Json(FavoriteErrorResponse::new("NOT_FOUND", "Guide not found")),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(FavoriteErrorResponse::new(
                        "CHECK_ERROR",
                        format!("Failed to check favorite status: {}", e),
                    )),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(FavoriteErrorResponse::new(
                "INSERT_ERROR",
                format!("Failed to add favorite: {}", e),
            )),
        )
            .into_response(),
    }
}

/// Handler for `DELETE /api/guides/{id}/favorite`.
/// Removes a guide from the user's favorites.
pub async fn remove_favorite_guide_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
    ValidatedPathUuid(guide_path_id): ValidatedPathUuid,
) -> impl IntoApiResponse {
    match remove_favorite(&state.db, user.user_id(), guide_path_id).await {
        Ok(Some(_)) => (
            StatusCode::OK,
            Json(RemoveFavoriteGuideResponse {
                status: "success".to_string(),
                message: "Guide removed from favorites".to_string(),
            }),
        )
            .into_response(),
        Ok(None) => {
            // Not favorited - return success anyway (idempotent)
            (
                StatusCode::OK,
                Json(RemoveFavoriteGuideResponse {
                    status: "success".to_string(),
                    message: "Guide was not in favorites".to_string(),
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(FavoriteErrorResponse::new(
                "REMOVE_ERROR",
                format!("Failed to remove favorite: {}", e),
            )),
        )
            .into_response(),
    }
}

/// Handler for `GET /api/guides/{id}/favorite`.
/// Checks if a guide is in the user's favorites.
pub async fn check_favorite_guide_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
    ValidatedPathUuid(guide_path_id): ValidatedPathUuid,
) -> impl IntoApiResponse {
    match check_favorite(&state.db, user.user_id(), guide_path_id).await {
        Ok(is_favorited) => (
            StatusCode::OK,
            Json(CheckFavoriteGuideResponse {
                status: "success".to_string(),
                is_favorited,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(FavoriteErrorResponse::new(
                "CHECK_ERROR",
                format!("Failed to check favorite status: {}", e),
            )),
        )
            .into_response(),
    }
}

/// Handler for `GET /api/guides/favorites`.
/// Lists all favorite guides for the authenticated user.
pub async fn list_favorite_guides_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
) -> impl IntoApiResponse {
    let filter_opts = FilterOptions {
        limit: Some(100),
        page: Some(1),
    };

    match select_favorites_by_user(&state.db, user.user_id(), filter_opts).await {
        Ok(favorites) => {
            let favorite_infos: Vec<FavoriteGuideInfo> = favorites
                .into_iter()
                .map(|f| FavoriteGuideInfo {
                    guide: f.guide,
                    created_at: format_time(f.created_at.into()),
                })
                .collect();

            (
                StatusCode::OK,
                Json(ListFavoriteGuidesResponse {
                    status: "success".to_string(),
                    results: favorite_infos.len(),
                    objs: favorite_infos,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(FavoriteErrorResponse::new(
                "QUERY_ERROR",
                format!("Failed to list favorites: {}", e),
            )),
        )
            .into_response(),
    }
}
