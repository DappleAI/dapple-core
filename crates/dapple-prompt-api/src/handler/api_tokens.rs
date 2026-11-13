//! Handlers for API token management.
//!
//! These endpoints require JWT authentication (not API token auth)
//! to prevent tokens from being used to create more tokens.

use crate::schema::api_tokens::{
    ApiTokenErrorResponse, ApiTokenInfo, CreateApiTokenResponse, CreateApiTokenSchema,
    ListApiTokensResponse, RevokeApiTokenResponse, RevokeApiTokenSchema,
};
use crate::service::api_token_crypto;
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::FilterOptions;
use dapple_data::model::api_tokens::InsertApiTokenArgs;
use dapple_data::query::api_tokens::{
    insert_api_tokens, revoke_api_token, select_api_token_by_object_id, select_api_tokens_by_user,
};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

/// Convert time::OffsetDateTime to ISO 8601 string
fn format_time(dt: OffsetDateTime) -> String {
    // Use time's formatting
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

/// Handler for `POST /api/api-tokens/`.
/// Creates a new API token for the authenticated user.
/// Requires JWT auth (not API token auth).
pub async fn create_api_token_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
    Json(req): Json<CreateApiTokenSchema>,
) -> impl IntoApiResponse {
    // Only allow JWT-authenticated users (not API tokens) to create tokens
    if user.is_api_token() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiTokenErrorResponse::new(
                "FORBIDDEN",
                "Cannot create API tokens using API token authentication. Use JWT authentication.",
            )),
        )
            .into_response();
    }

    // Validate name length
    if req.name.is_empty() || req.name.len() > 255 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiTokenErrorResponse::new(
                "INVALID_NAME",
                "Token name must be between 1 and 255 characters",
            )),
        )
            .into_response();
    }

    // Generate token and hash
    let (plaintext_token, token_hash) = api_token_crypto::generate_token();
    let token_prefix = api_token_crypto::token_prefix(&plaintext_token);

    // Calculate expiration time
    let expires_at = req
        .expires_in_secs
        .map(|secs| OffsetDateTime::now_utc() + time::Duration::seconds(secs as i64));

    // Insert the token
    let args = InsertApiTokenArgs {
        user_ids: vec![user.user_id()],
        names: vec![req.name.clone()],
        token_hashes: vec![token_hash],
        token_prefixes: vec![token_prefix.clone()],
        expires_ats: vec![expires_at],
    };

    let inserted = match insert_api_tokens(&state.db, args).await {
        Ok(tokens) => tokens,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiTokenErrorResponse::new(
                    "INSERT_ERROR",
                    format!("Failed to create token: {}", e),
                )),
            )
                .into_response();
        }
    };

    let token = match inserted.into_iter().next() {
        Some(t) => t,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiTokenErrorResponse::new(
                    "INSERT_ERROR",
                    "No token returned from insert",
                )),
            )
                .into_response();
        }
    };

    (
        StatusCode::CREATED,
        Json(CreateApiTokenResponse {
            status: "success".to_string(),
            id: token.id,
            name: token.name,
            token: plaintext_token,
            token_prefix,
            expires_at: token.expires_at.map(format_time),
            created_at: format_time(token.created_at),
        }),
    )
        .into_response()
}

/// Handler for `GET /api/api-tokens/`.
/// Lists all API tokens for the authenticated user.
/// Requires JWT auth.
pub async fn list_api_tokens_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
) -> impl IntoApiResponse {
    // Only allow JWT-authenticated users
    if user.is_api_token() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiTokenErrorResponse::new(
                "FORBIDDEN",
                "Cannot list API tokens using API token authentication. Use JWT authentication.",
            )),
        )
            .into_response();
    }

    // Get all tokens for the user
    let tokens = match select_api_tokens_by_user(
        &state.db,
        user.user_id(),
        FilterOptions {
            limit: Some(100),
            page: Some(1),
        },
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiTokenErrorResponse::new(
                    "QUERY_ERROR",
                    format!("Failed to list tokens: {}", e),
                )),
            )
                .into_response();
        }
    };

    let token_infos: Vec<ApiTokenInfo> = tokens
        .into_iter()
        .map(|t| ApiTokenInfo {
            id: t.id,
            name: t.name,
            token_prefix: t.token_prefix,
            expires_at: t.expires_at.map(|dt| format_time(dt.into())),
            created_at: format_time(t.created_at.into()),
            revoked_at: t.revoked_at.map(|dt| format_time(dt.into())),
        })
        .collect();

    (
        StatusCode::OK,
        Json(ListApiTokensResponse {
            status: "success".to_string(),
            results: token_infos.len(),
            tokens: token_infos,
        }),
    )
        .into_response()
}

/// Handler for `DELETE /api/api-tokens/:object_id`.
/// Revokes an API token.
/// Requires JWT auth.
pub async fn revoke_api_token_handler(
    State(state): State<Arc<AppState>>,
    user: RegisteredUser,
    Path(object_id): Path<Uuid>,
    body: Option<Json<RevokeApiTokenSchema>>,
) -> impl IntoApiResponse {
    // Only allow JWT-authenticated users
    if user.is_api_token() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiTokenErrorResponse::new(
                "FORBIDDEN",
                "Cannot revoke API tokens using API token authentication. Use JWT authentication.",
            )),
        )
            .into_response();
    }

    // Look up the token to verify ownership
    let token = match select_api_token_by_object_id(&state.db, object_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiTokenErrorResponse::new(
                    "TOKEN_NOT_FOUND",
                    "API token not found",
                )),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiTokenErrorResponse::new(
                    "QUERY_ERROR",
                    format!("Failed to look up token: {}", e),
                )),
            )
                .into_response();
        }
    };

    // Verify ownership
    if token.user_id != user.user_id() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiTokenErrorResponse::new(
                "FORBIDDEN",
                "Cannot revoke tokens belonging to other users",
            )),
        )
            .into_response();
    }

    // Check if already revoked
    if token.revoked_at.is_some() {
        return (
            StatusCode::CONFLICT,
            Json(ApiTokenErrorResponse::new(
                "ALREADY_REVOKED",
                "Token has already been revoked",
            )),
        )
            .into_response();
    }

    // Extract reason from body
    let reason = body.and_then(|b| b.reason.clone());

    // Revoke the token
    match revoke_api_token(&state.db, object_id, reason).await {
        Ok(Some(_)) => (
            StatusCode::OK,
            Json(RevokeApiTokenResponse {
                status: "success".to_string(),
                message: "API token has been revoked".to_string(),
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiTokenErrorResponse::new(
                "TOKEN_NOT_FOUND",
                "API token not found",
            )),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiTokenErrorResponse::new(
                "REVOKE_ERROR",
                format!("Failed to revoke token: {}", e),
            )),
        )
            .into_response(),
    }
}
