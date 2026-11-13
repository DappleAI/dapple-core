//! Handler for the service token verification endpoint.

use crate::AppState;
use crate::config::jwt_expiry_secs;
use crate::schema::service_token::{
    GetServiceTokenRequest, GetServiceTokenResponse, ServiceTokenErrorResponse,
};
use crate::service::service_token::{ServiceTokenError, verify_service_token};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{Duration, Utc};
use dapple_data::query::service_tokens::select_service_token_by_service_id;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// JWT claims structure for service tokens (token_use = "service")
#[derive(Debug, Serialize, Deserialize)]
struct ServiceJwtClaims {
    /// Service identifier (sub = service_id for service tokens)
    sub: String,
    /// Service identifier (redundant with sub for clarity)
    service_id: String,
    /// Token type: "service" for service tokens
    token_use: String,
    /// Expiration timestamp (Unix timestamp)
    exp: usize,
}

/// JWTUserIdRef for delegated token claims - matches the API's JWTUserIdRef enum
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
enum JWTUserIdRef {
    AuthUser(i64),
}

/// JWT claims structure for delegated tokens (token_use = "delegated")
#[derive(Debug, Serialize, Deserialize)]
struct DelegatedJwtClaims {
    /// User reference in JWTUserIdRef format
    sub: JWTUserIdRef,
    /// Service identifier
    service_id: String,
    /// Token type: "delegated" for user tokens
    token_use: String,
    /// User roles (optional)
    roles: Option<Vec<String>>,
    /// Expiration timestamp (Unix timestamp)
    exp: usize,
}

/// Handler for `POST /api/get-service-token`
///
/// Verifies an Ed25519-signed service token and issues a JWT for API access.
pub async fn get_service_token_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GetServiceTokenRequest>,
) -> impl IntoApiResponse {
    // 1. Load JWT secret
    let secret = match std::env::var("DAPPLE_PROMPT_API_SIGNING_KEY") {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ServiceTokenErrorResponse::new(
                    "SERVICE_UNAVAILABLE",
                    "JWT secret not configured",
                )),
            )
                .into_response();
        }
    };

    // 2. Look up service token by service_id
    let validation = match select_service_token_by_service_id(&state.db, &req.service_id).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ServiceTokenErrorResponse::new(
                    "SERVICE_NOT_FOUND",
                    format!("service '{}' is not registered", req.service_id),
                )),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ServiceTokenErrorResponse::new(
                    "DATABASE_ERROR",
                    format!("failed to look up service: {}", e),
                )),
            )
                .into_response();
        }
    };

    // 3. Check if service token is revoked
    if validation.revoked_at.is_some() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ServiceTokenErrorResponse::new(
                "SERVICE_TOKEN_REVOKED",
                format!("service token for '{}' has been revoked", req.service_id),
            )),
        )
            .into_response();
    }

    // 4. Verify the signed token
    let _verified =
        match verify_service_token(&req.token, &req.service_id, &validation.public_key_pem) {
            Ok(v) => v,
            Err(e) => {
                let (status, error_code) = match &e {
                    ServiceTokenError::MalformedToken(_)
                    | ServiceTokenError::InvalidVersion { .. }
                    | ServiceTokenError::DurationExceedsMax { .. }
                    | ServiceTokenError::InvalidPublicKey(_) => {
                        (StatusCode::BAD_REQUEST, extract_error_code(&e))
                    }
                    ServiceTokenError::InvalidSignature
                    | ServiceTokenError::NotYetValid { .. }
                    | ServiceTokenError::Expired { .. } => {
                        (StatusCode::UNAUTHORIZED, extract_error_code(&e))
                    }
                    ServiceTokenError::ServiceIdMismatch { .. } => {
                        (StatusCode::FORBIDDEN, extract_error_code(&e))
                    }
                };

                return (
                    status,
                    Json(ServiceTokenErrorResponse::new(error_code, e.to_string())),
                )
                    .into_response();
            }
        };

    // 5. Build JWT claims - either service or delegated based on auth_user_id
    let expiry_secs = jwt_expiry_secs();
    let expires_at = Utc::now() + Duration::seconds(expiry_secs as i64);
    let exp = expires_at.timestamp() as usize;

    // 6. Sign the JWT - branch on whether this is a delegated (user) or service token
    let jwt = if let Some(auth_user_id) = req.auth_user_id {
        // Create delegated token for user
        let claims = DelegatedJwtClaims {
            sub: JWTUserIdRef::AuthUser(auth_user_id),
            service_id: req.service_id.clone(),
            token_use: "delegated".to_string(),
            roles: Some(vec!["user".to_string()]),
            exp,
        };

        match encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        ) {
            Ok(token) => token,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ServiceTokenErrorResponse::new(
                        "JWT_SIGNING_ERROR",
                        format!("failed to sign JWT: {}", e),
                    )),
                )
                    .into_response();
            }
        }
    } else {
        // Create service token (no user)
        let claims = ServiceJwtClaims {
            sub: req.service_id.clone(),
            service_id: req.service_id.clone(),
            token_use: "service".to_string(),
            exp,
        };

        match encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        ) {
            Ok(token) => token,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ServiceTokenErrorResponse::new(
                        "JWT_SIGNING_ERROR",
                        format!("failed to sign JWT: {}", e),
                    )),
                )
                    .into_response();
            }
        }
    };

    // 7. Return success response
    (
        StatusCode::OK,
        Json(GetServiceTokenResponse {
            status: "ok".to_string(),
            jwt,
            service_id: req.service_id,
            expires_at: expires_at.to_rfc3339(),
        }),
    )
        .into_response()
}

/// Extract the error code prefix from a ServiceTokenError
fn extract_error_code(e: &ServiceTokenError) -> &'static str {
    match e {
        ServiceTokenError::MalformedToken(_) => "MALFORMED_TOKEN",
        ServiceTokenError::InvalidSignature => "INVALID_SIGNATURE",
        ServiceTokenError::InvalidVersion { .. } => "INVALID_VERSION",
        ServiceTokenError::DurationExceedsMax { .. } => "DURATION_EXCEEDS_MAX",
        ServiceTokenError::NotYetValid { .. } => "NOT_YET_VALID",
        ServiceTokenError::Expired { .. } => "EXPIRED",
        ServiceTokenError::ServiceIdMismatch { .. } => "SERVICE_ID_MISMATCH",
        ServiceTokenError::InvalidPublicKey(_) => "INVALID_PUBLIC_KEY",
    }
}
