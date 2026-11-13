//! Authentication extractor for the mock API server.
//!
//! When authentication is enabled, this validates the JWT Bearer token.
//! When disabled, requests pass through with a dummy context.

use crate::state::MockState;
use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// JWT claims structure matching the production API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub service_id: String,
    pub token_use: String,
    pub auth_user_id: Option<String>,
    pub roles: Option<Vec<String>>,
    pub exp: usize,
}

/// Authentication context extracted from validated JWT
#[derive(Debug, Clone)]
pub struct MockAuthContext {
    /// The user ID from the JWT `sub` claim
    pub user_id: Option<Uuid>,
    /// Whether authentication was actually performed
    pub authenticated: bool,
    /// The raw claims if authentication was performed
    pub claims: Option<JwtClaims>,
}

impl MockAuthContext {
    /// Create a dummy context for when auth is disabled
    fn unauthenticated() -> Self {
        Self {
            user_id: None,
            authenticated: false,
            claims: None,
        }
    }

    /// Create an authenticated context from JWT claims
    fn authenticated(claims: JwtClaims) -> Self {
        let user_id = Uuid::parse_str(&claims.sub).ok();
        Self {
            user_id,
            authenticated: true,
            claims: Some(claims),
        }
    }
}

/// Error type for authentication failures
pub struct AuthError {
    status: StatusCode,
    message: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "status": "fail",
                "message": self.message
            })),
        )
            .into_response()
    }
}

impl<S> FromRequestParts<S> for MockAuthContext
where
    S: Send + Sync,
    Arc<MockState>: FromRequestParts<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Extract the MockState
        let mock_state = match Arc::<MockState>::from_request_parts(parts, state).await {
            Ok(s) => s,
            Err(_) => {
                // If we can't get state, assume auth is disabled
                return Ok(MockAuthContext::unauthenticated());
            }
        };

        // If auth is disabled, return dummy context
        if !mock_state.is_auth_enabled() {
            return Ok(MockAuthContext::unauthenticated());
        }

        // Get JWT secret
        let secret = match &mock_state.auth_config.jwt_secret {
            Some(s) => s,
            None => return Ok(MockAuthContext::unauthenticated()),
        };

        // Check for public endpoints that don't require auth
        let path = parts.uri.path();
        if is_public_endpoint(path, &parts.method) {
            return Ok(MockAuthContext::unauthenticated());
        }

        // Extract Authorization header
        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok());

        let token = match auth_header.and_then(|s| s.strip_prefix("Bearer ")) {
            Some(t) => t,
            None => {
                return Err(AuthError {
                    status: StatusCode::UNAUTHORIZED,
                    message: "Missing or invalid authorization header".to_string(),
                });
            }
        };

        // Validate JWT
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;

        match decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        ) {
            Ok(token_data) => Ok(MockAuthContext::authenticated(token_data.claims)),
            Err(e) => Err(AuthError {
                status: StatusCode::UNAUTHORIZED,
                message: format!("Invalid token: {}", e),
            }),
        }
    }
}

/// Check if an endpoint is public (doesn't require auth)
fn is_public_endpoint(path: &str, method: &axum::http::Method) -> bool {
    let path_normalized = path.trim_end_matches('/');

    // Health check and docs are always public
    if method == axum::http::Method::GET
        && (path_normalized == "/api/health_check"
            || path.starts_with("/api.json")
            || path.starts_with("/docs")
            || path.starts_with("/mock/"))
    {
        return true;
    }

    // POST to /api/get-auth-token is public (that's how you get a token)
    if method == axum::http::Method::POST && path_normalized == "/api/get-auth-token" {
        return true;
    }

    false
}
