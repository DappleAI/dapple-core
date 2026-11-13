//! Handler for the auth token verification endpoint.
//!
//! This handler matches the production behavior from `dapple-prompt-api`.

use crate::service::auth_token::{AuthTokenError, extract_error_code, verify_auth_token};
use crate::state::MockState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Request body for `POST /api/get-auth-token`
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetAuthTokenRequest {
    /// Base64-encoded 129-byte signed auth token.
    pub token: String,
    /// User UUID to include in the JWT `sub` claim.
    pub user_id: Uuid,
}

/// Successful response from `POST /api/get-auth-token`
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetAuthTokenResponse {
    pub status: String,
    pub jwt: String,
    pub signer: String,
    pub expires_at: String,
}

/// Error response from `POST /api/get-auth-token`
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct AuthTokenErrorResponse {
    pub error: String,
    pub message: String,
}

impl AuthTokenErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}

/// JWT claims structure matching the production API
#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    sub: String,
    service_id: String,
    token_use: String,
    auth_user_id: Option<String>,
    roles: Vec<String>,
    exp: usize,
}

/// Default JWT expiry in seconds (1 hour)
const DEFAULT_JWT_EXPIRY_SECS: u64 = 3600;

/// Handler for `POST /api/get-auth-token`
///
/// Verifies an Ethereum-signed auth token and issues a JWT for API access.
pub async fn get_auth_token_handler(
    State(state): State<Arc<MockState>>,
    Json(req): Json<GetAuthTokenRequest>,
) -> impl IntoResponse {
    // 1. Check if auth is enabled
    if !state.is_auth_enabled() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AuthTokenErrorResponse::new(
                "SERVICE_UNAVAILABLE",
                "Auth token endpoint is not configured. Use MockState::with_auth() to enable.",
            )),
        )
            .into_response();
    }

    // 2. Get configuration
    let secret = match &state.auth_config.jwt_secret {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AuthTokenErrorResponse::new(
                    "SERVICE_UNAVAILABLE",
                    "JWT secret not configured",
                )),
            )
                .into_response();
        }
    };

    let allowed_signers = &state.auth_config.allowed_signers;

    // 3. Verify the auth token
    let verified = match verify_auth_token(&req.token, Some(allowed_signers)) {
        Ok(v) => v,
        Err(e) => {
            let (status, error_code) = match &e {
                AuthTokenError::MalformedToken(_)
                | AuthTokenError::InvalidVersion { .. }
                | AuthTokenError::DurationExceedsMax { .. } => {
                    (StatusCode::BAD_REQUEST, extract_error_code(&e))
                }
                AuthTokenError::InvalidSignature
                | AuthTokenError::NotYetValid { .. }
                | AuthTokenError::Expired { .. } => {
                    (StatusCode::UNAUTHORIZED, extract_error_code(&e))
                }
                AuthTokenError::SignerNotAuthorized { .. } => {
                    (StatusCode::FORBIDDEN, extract_error_code(&e))
                }
            };

            return (
                status,
                Json(AuthTokenErrorResponse::new(error_code, e.to_string())),
            )
                .into_response();
        }
    };

    // 4. Build JWT claims
    let expires_at = Utc::now() + Duration::seconds(DEFAULT_JWT_EXPIRY_SECS as i64);
    let exp = expires_at.timestamp() as usize;

    let claims = JwtClaims {
        sub: req.user_id.to_string(),
        service_id: "web".to_string(),
        token_use: "delegated".to_string(),
        auth_user_id: None,
        roles: vec!["user".to_string()],
        exp,
    };

    // 5. Sign the JWT
    let jwt = match encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    ) {
        Ok(token) => token,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AuthTokenErrorResponse::new(
                    "JWT_SIGNING_ERROR",
                    format!("Failed to sign JWT: {}", e),
                )),
            )
                .into_response();
        }
    };

    // 6. Return success response
    (
        StatusCode::OK,
        Json(GetAuthTokenResponse {
            status: "ok".to_string(),
            jwt,
            signer: format!("{:#x}", verified.signer),
            expires_at: expires_at.to_rfc3339(),
        }),
    )
        .into_response()
}
