//! Handlers for chain-level journey favorites.
//!
//! The endpoints still hang off `/api/journeys/{id}/...` for caller
//! convenience, but every operation resolves the supplied journey id to
//! the chain root via the shared recursive CTE before reading or writing.
//! See Migration 46 for the data-layer rationale.

use crate::extract_path_uuid::ValidatedPathUuid;
use crate::schema::user_favorite_journey_chains::{
    AddFavoriteJourneyChainResponse, CheckFavoriteJourneyChainResponse, FavoriteErrorResponse,
    FavoriteJourneyChainInfo, ListFavoriteJourneyChainsResponse,
    RemoveFavoriteJourneyChainResponse,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::FilterOptions;
use dapple_data::model::journeys::JourneyRef;
use dapple_data::query::principals::select_human_principal_for_user_object_id;
use dapple_data::query::user_favorite_journey_chains::{
    check_favorite_chain, insert_favorite_chain, remove_favorite_chain,
    select_favorite_chains_by_user,
};
use std::sync::Arc;
use time::OffsetDateTime;

fn format_time(dt: OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

/// `POST /api/journeys/{id}/favorite`. Favorites the chain that contains
/// the supplied journey id. Idempotent: 200 when already favorited, 201
/// on the first favorite, 404 when the journey id isn't known.
pub async fn add_favorite_journey_chain_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
    ValidatedPathUuid(starting_journey_id): ValidatedPathUuid,
) -> impl IntoApiResponse {
    match insert_favorite_chain(&state.db, user.user_id(), starting_journey_id).await {
        Ok(Some(inserted)) => (
            StatusCode::CREATED,
            Json(AddFavoriteJourneyChainResponse {
                status: "success".to_string(),
                chain_root: JourneyRef {
                    id: inserted.chain_root_journey_id,
                    name: inserted.chain_root_name,
                },
                favorited_at: format_time(inserted.created_at),
            }),
        )
            .into_response(),
        Ok(None) => {
            // Either already favorited or the journey id doesn't resolve.
            // Disambiguate via a check.
            match check_favorite_chain(&state.db, user.user_id(), starting_journey_id).await {
                Ok(check) if check.is_favorited => (
                    StatusCode::OK,
                    Json(AddFavoriteJourneyChainResponse {
                        status: "success".to_string(),
                        chain_root: JourneyRef {
                            id: check.chain_root_journey_id.unwrap_or(starting_journey_id),
                            name: String::new(),
                        },
                        favorited_at: String::new(),
                    }),
                )
                    .into_response(),
                Ok(_) => (
                    StatusCode::NOT_FOUND,
                    Json(FavoriteErrorResponse::new("NOT_FOUND", "Journey not found")),
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

/// `DELETE /api/journeys/{id}/favorite`. Idempotent — returns 200 whether
/// or not the chain was actually favorited.
pub async fn remove_favorite_journey_chain_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
    ValidatedPathUuid(starting_journey_id): ValidatedPathUuid,
) -> impl IntoApiResponse {
    match remove_favorite_chain(&state.db, user.user_id(), starting_journey_id).await {
        Ok(Some(_)) => (
            StatusCode::OK,
            Json(RemoveFavoriteJourneyChainResponse {
                status: "success".to_string(),
                message: "Chain removed from favorites".to_string(),
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::OK,
            Json(RemoveFavoriteJourneyChainResponse {
                status: "success".to_string(),
                message: "Chain was not in favorites".to_string(),
            }),
        )
            .into_response(),
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

/// `GET /api/journeys/{id}/favorite`. Returns whether the chain containing
/// the supplied journey id is favorited, plus the resolved chain root.
pub async fn check_favorite_journey_chain_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
    ValidatedPathUuid(starting_journey_id): ValidatedPathUuid,
) -> impl IntoApiResponse {
    match check_favorite_chain(&state.db, user.user_id(), starting_journey_id).await {
        Ok(check) => (
            StatusCode::OK,
            Json(CheckFavoriteJourneyChainResponse {
                status: "success".to_string(),
                is_favorited: check.is_favorited,
                chain_root_journey_id: check.chain_root_journey_id,
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

/// `GET /api/journeys/favorites`. Lists all favorite chains for the
/// authenticated user, resolving the chain root's name through the
/// viewer's per-principal display-name preferences.
pub async fn list_favorite_journey_chains_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
) -> impl IntoApiResponse {
    let filter_opts = FilterOptions {
        limit: Some(100),
        page: Some(1),
    };

    let viewer_principal_id =
        match select_human_principal_for_user_object_id(&state.db, user.user_id()).await {
            Ok(Some(pid)) => pid,
            Ok(None) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(FavoriteErrorResponse::new(
                        "NO_PRINCIPAL_FOR_USER",
                        format!("no human_principal for user_id={}", user.user_id()),
                    )),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(FavoriteErrorResponse::new(
                        "QUERY_ERROR",
                        format!("Failed to resolve viewer principal: {}", e),
                    )),
                )
                    .into_response();
            }
        };

    match select_favorite_chains_by_user(
        &state.db,
        user.user_id(),
        viewer_principal_id,
        filter_opts,
    )
    .await
    {
        Ok(favorites) => {
            let infos: Vec<FavoriteJourneyChainInfo> = favorites
                .into_iter()
                .map(|f| FavoriteJourneyChainInfo {
                    chain_root: f.chain_root,
                    favorited_at: format_time(f.favorited_at.into()),
                })
                .collect();

            (
                StatusCode::OK,
                Json(ListFavoriteJourneyChainsResponse {
                    status: "success".to_string(),
                    results: infos.len(),
                    objs: infos,
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
