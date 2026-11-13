//! Token-exchange handler for the external surface.
//!
//! The on-the-wire token format is identical to internal-tier tokens (the
//! same Ed25519 verification utility is reused via `dapple-prompt-api::
//! verify_service_token`). The differences are:
//!
//! - **Lookup table:** `rev.external_service_tokens` (joined with
//!   `rev.companies`) instead of `web.service_tokens`.
//! - **Revocation:** rejects if EITHER the token is in
//!   `rev.external_service_token_revocations` OR the owning company has
//!   `revoked_at` set.
//! - **JWT signing key:** loaded from `DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY`.
//!   A JWT signed with this key cannot be verified by the internal surface
//!   (cryptographic isolation, not just an `aud`-claim convention).
//! - **JWT claims:** `sub` is the company UUID; includes `company_slug`,
//!   `iss = "external-api.dapple.x"`, `token_use = "external_service"`.
//! - **No delegation:** there is no `auth_user_id`; customer-tier tokens
//!   do not act on behalf of dapple users.

use crate::schema::service_token::{
    ExternalServiceTokenErrorResponse, GetExternalServiceTokenRequest,
    GetExternalServiceTokenResponse,
};
use crate::{EXTERNAL_JWT_ISS, EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TOKEN_USE, ExternalAppState};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{Duration, Utc};
use dapple_data::query::external_service_tokens::select_external_service_token_by_service_id;
use dapple_prompt_api::{ServiceTokenError, verify_service_token};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// JWT lifetime for external service tokens. Same default as internal-tier
/// (1 hour). Configurable via `EXTERNAL_JWT_EXPIRY_SECS` env var.
const DEFAULT_JWT_EXPIRY_SECS: i64 = 3600;

fn jwt_expiry_secs() -> i64 {
    std::env::var("EXTERNAL_JWT_EXPIRY_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_JWT_EXPIRY_SECS)
}

/// JWT claims for external service tokens.
///
/// `sub` is the company UUID (not a service_id string) — handlers in the
/// external router use this directly as `owner_company_id` when filtering
/// queries.
///
/// `service_user_object_id` is the UUID of the service user in
/// `rev.user_details`. It's pre-resolved at JWT-mint time so handlers can
/// pass it directly as `user_object_id` to `insert_journeys` without a
/// per-request DB lookup.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExternalServiceJwtClaims {
    /// Company UUID (the owning org). Used as `owner_company_id`.
    pub sub: Uuid,
    /// Pre-resolved company slug (for human-readable logs / payloads).
    pub company_slug: String,
    /// Service identifier from `rev.external_service_tokens`.
    pub service_id: String,
    /// `rev.principals.id` — the external_service_principal for this token,
    /// auto-created by the Migration 37 trigger.
    pub principal_id: Uuid,
    /// Issuer claim. Always `EXTERNAL_JWT_ISS`.
    pub iss: String,
    /// Token type marker. Always `EXTERNAL_TOKEN_USE`.
    pub token_use: String,
    /// Expiration (Unix timestamp).
    pub exp: usize,
}

/// Handler for `POST /api/get-service-token` on the external surface.
pub async fn get_external_service_token_handler(
    State(state): State<Arc<ExternalAppState>>,
    Json(req): Json<GetExternalServiceTokenRequest>,
) -> impl IntoApiResponse {
    // 1. Load the external surface's JWT signing key. A missing key is
    //    operationally fatal — surface returns 503 so the caller knows the
    //    server is misconfigured rather than that their token is bad.
    let secret = match std::env::var(EXTERNAL_JWT_SIGNING_KEY_ENV) {
        Ok(s) if !s.is_empty() => s,
        _ => {
            log::error!(
                "{} not configured; external surface cannot mint JWTs",
                EXTERNAL_JWT_SIGNING_KEY_ENV
            );
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ExternalServiceTokenErrorResponse::new(
                    "SIGNING_KEY_NOT_CONFIGURED",
                    "external-surface JWT signing key is not configured",
                )),
            )
                .into_response();
        }
    };

    // 2. Look up the token by service_id, joined with the owning company
    //    and the most-recent revocation timestamp (if any).
    let lookup = match select_external_service_token_by_service_id(&state.db, &req.service_id).await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ExternalServiceTokenErrorResponse::new(
                    "SERVICE_NOT_FOUND",
                    format!("external service '{}' is not registered", req.service_id),
                )),
            )
                .into_response();
        }
        Err(e) => {
            log::error!("external service-token lookup failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ExternalServiceTokenErrorResponse::new(
                    "DATABASE_ERROR",
                    "failed to look up service token",
                )),
            )
                .into_response();
        }
    };

    // 3. Reject if the company has been revoked. (Revoking a company is
    //    the cheap way to cut off ALL of that customer's tokens at once.)
    if lookup.company_revoked_at.is_some() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ExternalServiceTokenErrorResponse::new(
                "COMPANY_REVOKED",
                format!("company '{}' has been revoked", lookup.company_slug),
            )),
        )
            .into_response();
    }

    // 4. Reject if this specific token has been revoked.
    if lookup.token_revoked_at.is_some() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ExternalServiceTokenErrorResponse::new(
                "SERVICE_TOKEN_REVOKED",
                format!(
                    "external service token '{}' has been revoked",
                    req.service_id
                ),
            )),
        )
            .into_response();
    }

    // 5. Verify the Ed25519 signature against the registered public key.
    //    Reuses the same crypto utility as the internal surface.
    if let Err(e) = verify_service_token(&req.token, &req.service_id, &lookup.public_key_pem) {
        let (status, error_code) = match &e {
            ServiceTokenError::MalformedToken(_)
            | ServiceTokenError::InvalidVersion { .. }
            | ServiceTokenError::DurationExceedsMax { .. }
            | ServiceTokenError::InvalidPublicKey(_) => (StatusCode::BAD_REQUEST, error_code(&e)),
            ServiceTokenError::InvalidSignature
            | ServiceTokenError::NotYetValid { .. }
            | ServiceTokenError::Expired { .. } => (StatusCode::UNAUTHORIZED, error_code(&e)),
            ServiceTokenError::ServiceIdMismatch { .. } => (StatusCode::FORBIDDEN, error_code(&e)),
        };
        return (
            status,
            Json(ExternalServiceTokenErrorResponse::new(
                error_code,
                e.to_string(),
            )),
        )
            .into_response();
    }

    // 6. Mint the JWT.
    let expires_at = Utc::now() + Duration::seconds(jwt_expiry_secs());
    let exp = expires_at.timestamp() as usize;

    let claims = ExternalServiceJwtClaims {
        sub: lookup.company_id,
        company_slug: lookup.company_slug.clone(),
        service_id: req.service_id.clone(),
        principal_id: lookup.principal_id,
        iss: EXTERNAL_JWT_ISS.to_string(),
        token_use: EXTERNAL_TOKEN_USE.to_string(),
        exp,
    };

    let jwt = match encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    ) {
        Ok(token) => token,
        Err(e) => {
            log::error!("external JWT signing failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ExternalServiceTokenErrorResponse::new(
                    "JWT_SIGNING_ERROR",
                    "failed to sign JWT",
                )),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        Json(GetExternalServiceTokenResponse {
            status: "ok".to_string(),
            jwt,
            service_id: req.service_id,
            company_slug: lookup.company_slug,
            expires_at: expires_at.to_rfc3339(),
        }),
    )
        .into_response()
}

fn error_code(e: &ServiceTokenError) -> &'static str {
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
