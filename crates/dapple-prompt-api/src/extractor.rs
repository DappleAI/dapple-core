//! Custom Axum extractors for authentication and authorization.

use crate::{AppState, AuthContext, CallerKind, JWTUserIdRef};
use aide::OperationInput;
use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use dapple_data::model::user_details::UserDetail;
use dapple_data::query::user_details::{UserIdRef, resolve_user_id_ref};
use schemars::JsonSchema;
use serde::Serialize;
use std::sync::Arc;

/// Standard error response for user-scoped handlers.
#[derive(JsonSchema, Serialize, Debug)]
pub struct UserScopedError {
    pub error: String,
    pub message: String,
}

impl UserScopedError {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new("UNAUTHORIZED", message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new("FORBIDDEN", message)
    }
}

/// Rejection type for RegisteredUser extractor.
pub enum RegisteredUserRejection {
    Unauthorized(String),
    Forbidden(String),
}

impl IntoResponse for RegisteredUserRejection {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                Json(UserScopedError::unauthorized(msg)),
            )
                .into_response(),
            Self::Forbidden(msg) => {
                (StatusCode::FORBIDDEN, Json(UserScopedError::forbidden(msg))).into_response()
            }
        }
    }
}

/// Extractor that validates authentication AND user registration.
///
/// Use this in handlers that require a registered user. It combines:
/// 1. Authentication (JWT or API token)
/// 2. User registration verification (user exists in `user_details` table)
///
/// # Example
/// ```ignore
/// pub async fn my_handler(
///     State(state): State<Arc<AppState>>,
///     user: RegisteredUser,
/// ) -> impl IntoApiResponse {
///     // user.user_detail and user.auth are immediately available
///     let user_id = user.user_id();
///     // ... handler logic
/// }
/// ```
#[derive(Debug, Clone)]
pub struct RegisteredUser {
    pub user_detail: UserDetail,
    pub auth: AuthContext,
}

impl RegisteredUser {
    /// Get the user's id (UUID) from user_details.
    pub fn user_id(&self) -> uuid::Uuid {
        self.user_detail.id
    }

    /// Alias for user_id() -- returns the same UUID.
    /// Retained for backward compatibility with callers that used the old object_id() method.
    pub fn object_id(&self) -> uuid::Uuid {
        self.user_detail.id
    }

    /// Check if authenticated via API token (dpl_ prefix).
    pub fn is_api_token(&self) -> bool {
        self.auth.is_api_token()
    }
}

/// Implement OperationInput for aide OpenAPI documentation.
/// Auth extractors don't contribute to OpenAPI schema - they're handled via security schemes.
impl OperationInput for RegisteredUser {}

impl FromRequestParts<Arc<AppState>> for RegisteredUser {
    type Rejection = RegisteredUserRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // Extract AuthContext first
        let auth = AuthContext::from_request_parts(parts, state)
            .await
            .map_err(|rejection| {
                log::warn!(
                    "RegisteredUser extractor: AuthContext rejected: {:?}",
                    rejection
                );
                RegisteredUserRejection::Unauthorized("Authentication required".into())
            })?;

        // Get JWTUserIdRef from auth context
        let jwt_user_id_ref = auth.jwt_user_id_ref().ok_or_else(|| {
            log::warn!("RegisteredUser extractor: AuthContext present but no jwt_user_id_ref");
            RegisteredUserRejection::Unauthorized("User authentication required".into())
        })?;

        // Convert to UserIdRef for the data layer
        let user_id_ref = match jwt_user_id_ref {
            JWTUserIdRef::AuthUser(id) => UserIdRef::AuthUser(id),
            JWTUserIdRef::ObjectId(uuid) => UserIdRef::ObjectId(uuid),
        };

        // Resolve to UserDetail (auto-creates if AuthUser type)
        let user_detail = resolve_user_id_ref(&state.db, &user_id_ref)
            .await
            .map_err(|e| {
                log::warn!(
                    "RegisteredUser extractor: resolve_user_id_ref({:?}) failed: {}",
                    user_id_ref,
                    e
                );
                RegisteredUserRejection::Unauthorized(
                    "User not found. Please re-authenticate.".into(),
                )
            })?;

        Ok(RegisteredUser { user_detail, auth })
    }
}

// ---------------------------------------------------------------------------
// ServiceOrUser — accepts either a service JWT or a registered user.
// ---------------------------------------------------------------------------

/// Extractor that accepts either a service identity (`CallerKind::ServiceOnly`
/// / `ServiceToken`) or a full registered user. Use on internal endpoints that
/// both human users (Studio) and services (conduit) need to reach.
#[derive(Debug, Clone)]
pub struct ServiceOrUser(pub AuthContext);

impl OperationInput for ServiceOrUser {}

impl FromRequestParts<Arc<AppState>> for ServiceOrUser {
    type Rejection = RegisteredUserRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthContext::from_request_parts(parts, state)
            .await
            .map_err(|_| RegisteredUserRejection::Unauthorized("Authentication required".into()))?;

        match &auth.caller_kind {
            // Service identities are accepted without user resolution.
            CallerKind::ServiceOnly | CallerKind::ServiceToken(_) => Ok(ServiceOrUser(auth)),
            // For user callers, verify registration (same as RegisteredUser).
            _ => {
                let jwt_ref = auth.jwt_user_id_ref().ok_or_else(|| {
                    RegisteredUserRejection::Unauthorized("User authentication required".into())
                })?;
                let user_id_ref = match jwt_ref {
                    JWTUserIdRef::AuthUser(id) => UserIdRef::AuthUser(id),
                    JWTUserIdRef::ObjectId(uuid) => UserIdRef::ObjectId(uuid),
                };
                let _user = resolve_user_id_ref(&state.db, &user_id_ref)
                    .await
                    .map_err(|_| {
                        RegisteredUserRejection::Unauthorized(
                            "User not found. Please re-authenticate.".into(),
                        )
                    })?;
                Ok(ServiceOrUser(auth))
            }
        }
    }
}
