//! Handler for the auth token verification endpoint.

use crate::config::{auth_token_enabled, jwt_expiry_secs, jwt_secret, load_allowed_signers};
use crate::schema::auth_token::{
    AuthTokenErrorResponse, GetAuthTokenRequest, GetAuthTokenResponse,
};
use crate::service::auth_token::{AuthTokenError, verify_auth_token};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};

/// JWT claims structure matching the existing RawClaims in lib.rs
#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    /// User UUID (for delegated tokens) or service ID (for service tokens)
    sub: String,
    /// Service identifier (e.g., "web", "worker")
    service_id: String,
    /// Token type: "delegated" or "service"
    token_use: String,
    /// Auth provider user ID (for delegated tokens)
    auth_user_id: Option<String>,
    /// User roles (for delegated tokens)
    roles: Vec<String>,
    /// Expiration timestamp (Unix timestamp)
    exp: usize,
}

/// Handler for `POST /api/get-auth-token`
///
/// Verifies an Ethereum-signed auth token and issues a JWT for API access.
pub async fn get_auth_token_handler(Json(req): Json<GetAuthTokenRequest>) -> impl IntoApiResponse {
    // 1. Check if auth token endpoint is enabled
    if !auth_token_enabled() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AuthTokenErrorResponse::new(
                "SERVICE_UNAVAILABLE",
                "Auth token endpoint is not configured. Set DAPPLE_PROMPT_API_SIGNING_KEY and AUTH_TOKEN_ALLOWED_SIGNERS.",
            )),
        )
            .into_response();
    }

    // 2. Load configuration
    let allowed_signers = load_allowed_signers();
    let secret = match jwt_secret() {
        Some(s) => s,
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

    // 3. Verify the auth token
    let verified = match verify_auth_token(&req.token, Some(&allowed_signers)) {
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
    let expiry_secs = jwt_expiry_secs();
    let expires_at = Utc::now() + Duration::seconds(expiry_secs as i64);
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

/// Extract the error code prefix from an AuthTokenError
fn extract_error_code(e: &AuthTokenError) -> &'static str {
    match e {
        AuthTokenError::MalformedToken(_) => "MALFORMED_TOKEN",
        AuthTokenError::InvalidSignature => "INVALID_SIGNATURE",
        AuthTokenError::InvalidVersion { .. } => "INVALID_VERSION",
        AuthTokenError::DurationExceedsMax { .. } => "DURATION_EXCEEDS_MAX",
        AuthTokenError::NotYetValid { .. } => "NOT_YET_VALID",
        AuthTokenError::Expired { .. } => "EXPIRED",
        AuthTokenError::SignerNotAuthorized { .. } => "SIGNER_NOT_AUTHORIZED",
    }
}
