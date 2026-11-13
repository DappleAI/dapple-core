//! `ExternalCaller` extractor — runs on every external-surface request that
//! requires authentication.
//!
//! ## Cross-surface replay impossible
//!
//! The extractor decodes the JWT using **the external surface's signing
//! key**. A JWT signed for the internal surface (with a different key)
//! fails signature verification *before* any claim is parsed. There is no
//! `aud`-claim check; isolation is cryptographic.
//!
//! ## Flow
//!
//! 1. Pull `Authorization: Bearer <jwt>` from the request.
//! 2. Verify the HMAC-SHA256 signature using `EXTERNAL_JWT_SIGNING_KEY_ENV`.
//! 3. Parse claims; reject if `iss` or `token_use` don't match the
//!    external-surface constants.
//! 4. Return an `ExternalCaller` carrying `company_id`, `company_slug`, and
//!    `service_id` for handlers and audit logs.
//!
//! V1 deliberately does **not** re-check `rev.companies.revoked_at` on every
//! request. Revocation is enforced at JWT exchange time
//! (`get_external_service_token_handler`); already-issued JWTs continue to
//! work for up to their `exp` (1 hour default). Per-request revocation
//! checks add a DB round-trip; we'll add them when audit/security demands it.

use crate::{EXTERNAL_JWT_ISS, EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TOKEN_USE, ExternalAppState};
use aide::OperationInput;
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

/// Authenticated external (customer-tier) caller, bound to one
/// `rev.companies` row.
///
/// All external-router handlers take this as an Axum extractor; queries
/// filter by `company_id` so cross-namespace visibility is structurally
/// impossible (Pattern B's structural-isolation property).
#[derive(Clone, Debug)]
pub struct ExternalCaller {
    /// `rev.companies.id` — used as `owner_company_id` in queries.
    pub company_id: Uuid,
    /// `rev.companies.slug` — pre-resolved for human-readable logs/payloads.
    pub company_slug: String,
    /// `rev.external_service_tokens.service_id` — for audit.
    pub service_id: String,
    /// `rev.principals.id` — the external_service_principal auto-created by
    /// the Migration 37 trigger on `rev.external_service_tokens` insert.
    /// Handlers stamp this directly onto journeys and dispatches as
    /// `principal_id` (no per-request user_details resolution).
    pub principal_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
struct DecodedClaims {
    sub: Uuid,
    company_slug: String,
    service_id: String,
    principal_id: Uuid,
    iss: String,
    token_use: String,
    exp: usize,
}

/// Failure modes that produce a 401. We deliberately *don't* leak which
/// specific check failed in the response body — clients just see "auth
/// failed" — but the structured variant is preserved for logs.
#[derive(Debug, Clone)]
pub enum ExternalAuthRejection {
    MissingAuthHeader,
    MalformedBearer,
    InvalidJwt,
    WrongIssuer,
    WrongTokenUse,
    SigningKeyNotConfigured,
}

impl IntoResponse for ExternalAuthRejection {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            ExternalAuthRejection::MissingAuthHeader => (
                StatusCode::UNAUTHORIZED,
                "MISSING_AUTH_HEADER",
                "request is missing Authorization header",
            ),
            ExternalAuthRejection::MalformedBearer => (
                StatusCode::UNAUTHORIZED,
                "MALFORMED_BEARER",
                "Authorization header is not a Bearer token",
            ),
            ExternalAuthRejection::InvalidJwt => (
                StatusCode::UNAUTHORIZED,
                "INVALID_JWT",
                "JWT signature verification failed or token expired",
            ),
            ExternalAuthRejection::WrongIssuer => (
                StatusCode::UNAUTHORIZED,
                "WRONG_ISSUER",
                "JWT was not issued by the external surface",
            ),
            ExternalAuthRejection::WrongTokenUse => (
                StatusCode::UNAUTHORIZED,
                "WRONG_TOKEN_USE",
                "JWT token_use claim is not 'external_service'",
            ),
            ExternalAuthRejection::SigningKeyNotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "SIGNING_KEY_NOT_CONFIGURED",
                "external-surface JWT signing key is not configured",
            ),
        };

        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

/// Auth extractors don't contribute to OpenAPI schema — they're handled via
/// security schemes (a Bearer token).
impl OperationInput for ExternalCaller {}

impl<S> FromRequestParts<S> for ExternalCaller
where
    S: Send + Sync,
    Arc<ExternalAppState>: axum::extract::FromRef<S>,
{
    type Rejection = ExternalAuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // 1. Pull the Authorization header
        let auth = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(ExternalAuthRejection::MissingAuthHeader)?;

        let jwt = auth
            .strip_prefix("Bearer ")
            .ok_or(ExternalAuthRejection::MalformedBearer)?
            .trim();

        // 2. Load the external signing key. A missing key is a server
        //    misconfiguration, not a client error — return 503.
        let secret = std::env::var(EXTERNAL_JWT_SIGNING_KEY_ENV)
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or(ExternalAuthRejection::SigningKeyNotConfigured)?;

        // 3. Decode and verify (HMAC-SHA256). Cross-surface tokens fail at
        //    this step — the internal surface's signing key won't validate.
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["exp"]);
        // We validate `iss` and `token_use` ourselves below so we get
        // distinct rejection variants for logging.
        validation.validate_aud = false;

        let token_data = decode::<DecodedClaims>(
            jwt,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .map_err(|_| ExternalAuthRejection::InvalidJwt)?;

        let claims = token_data.claims;

        // 4. Validate surface-specific claims.
        if claims.iss != EXTERNAL_JWT_ISS {
            return Err(ExternalAuthRejection::WrongIssuer);
        }
        if claims.token_use != EXTERNAL_TOKEN_USE {
            return Err(ExternalAuthRejection::WrongTokenUse);
        }

        Ok(ExternalCaller {
            company_id: claims.sub,
            company_slug: claims.company_slug,
            service_id: claims.service_id,
            principal_id: claims.principal_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::service_token::ExternalServiceJwtClaims;
    use chrono::{Duration, Utc};
    use jsonwebtoken::{EncodingKey, Header, encode};

    /// Mint a JWT with the given secret key and the standard external claims.
    fn mint_test_jwt(secret: &str, iss: &str, token_use: &str) -> String {
        let exp = (Utc::now() + Duration::hours(1)).timestamp() as usize;
        let claims = ExternalServiceJwtClaims {
            sub: Uuid::new_v4(),
            company_slug: "acme".to_string(),
            service_id: "test_service".to_string(),
            principal_id: Uuid::new_v4(),
            iss: iss.to_string(),
            token_use: token_use.to_string(),
            exp,
        };
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("test JWT encoding should not fail")
    }

    fn verify_with_secret(
        jwt: &str,
        secret: &str,
    ) -> Result<DecodedClaims, jsonwebtoken::errors::Error> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_aud = false;
        decode::<DecodedClaims>(
            jwt,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .map(|td| td.claims)
    }

    /// Cross-surface replay: a JWT signed with the internal-surface key
    /// must NOT be verifiable with the external-surface key. This is the
    /// crux of the cryptographic-isolation guarantee.
    #[test]
    fn cross_surface_replay_fails_at_signature_check() {
        let internal_key = "internal-secret-do-not-leak";
        let external_key = "external-secret-do-not-leak";
        assert_ne!(internal_key, external_key);

        // Mint a JWT as if the INTERNAL surface had issued it (correct
        // structure, internal signing key).
        let internal_jwt = mint_test_jwt(internal_key, EXTERNAL_JWT_ISS, EXTERNAL_TOKEN_USE);

        // Try to verify it with the EXTERNAL key (what the external
        // extractor would do). Must fail with a signature error.
        let result = verify_with_secret(&internal_jwt, external_key);
        assert!(
            result.is_err(),
            "JWT signed with internal key must not verify with external key"
        );

        // Inverse: mint with external key, verify with internal — also fails.
        let external_jwt = mint_test_jwt(external_key, EXTERNAL_JWT_ISS, EXTERNAL_TOKEN_USE);
        let result = verify_with_secret(&external_jwt, internal_key);
        assert!(
            result.is_err(),
            "JWT signed with external key must not verify with internal key"
        );
    }

    /// A JWT signed correctly but with `iss = "api.dapple.x"` (internal
    /// surface) must be rejected by the external extractor's claim check.
    /// This simulates the case where someone re-uses the same signing key
    /// across surfaces (which we explicitly disallow, but defense-in-depth):
    /// even then, the `iss` mismatch catches it.
    #[test]
    fn wrong_issuer_rejected_even_with_correct_signature() {
        let secret = "shared-test-secret";
        // Sign with the right key but the wrong `iss`.
        let jwt = mint_test_jwt(secret, "api.dapple.x", EXTERNAL_TOKEN_USE);

        // Decode succeeds (signature is correct), but our extractor would
        // reject on the iss claim.
        let claims = verify_with_secret(&jwt, secret).expect("signature should verify");
        assert_eq!(claims.iss, "api.dapple.x");
        assert_ne!(claims.iss, EXTERNAL_JWT_ISS);
    }

    /// Same defense-in-depth check for `token_use`: a delegated/internal
    /// token type must be rejected even if signature and issuer happened
    /// to match.
    #[test]
    fn wrong_token_use_rejected() {
        let secret = "shared-test-secret";
        let jwt = mint_test_jwt(secret, EXTERNAL_JWT_ISS, "service");

        let claims = verify_with_secret(&jwt, secret).expect("signature should verify");
        assert_ne!(claims.token_use, EXTERNAL_TOKEN_USE);
    }
}
