mod api_tokens;
mod auth_token;
mod companies;
mod dispatch_phases;
mod dispatch_task_attempts;
mod dispatches;
mod external_service_tokens;
mod service_token;

mod playbooks;

mod hats;

mod categories;
mod stages;
mod tasks;

mod user_details;

mod journeys;

mod steps;

mod fluent_messages;
mod guide_search;
mod guides;

mod assets;
mod brief_groups;
mod brief_packs;
mod brief_units;
mod briefs;
mod canonical;
mod deprecations;
mod directives;
mod dispatch_intent_bindings;
mod drafts;
mod inference_profiles;

mod prompts;
mod value_slots;
mod versioned_objects;

use crate::handler::serve_api_json;
use crate::service::api_token_crypto;
use crate::{AppState, AuthResult, RawClaims, ValidatedApiToken, handler::health_checker_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use aide::redoc::Redoc;
use dapple_data::query::api_tokens::select_api_token_by_hash;
use std::sync::Arc;
use time::OffsetDateTime;

use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Method, Request};
use axum::middleware::{Next, from_fn};
use axum::response::IntoResponse;
// Note: jsonwebtoken is kept for potential future use, but we use manual HMAC verification
// due to a bug in jsonwebtoken's claims parsing with adjacently-tagged serde enums.
// See: the auth_middleware function for the workaround.

// Auth middleware that validates tokens but stores results in extensions instead of rejecting.
// This allows routing to happen first, so missing endpoints return 404 (not 401).
// The AuthContext extractor checks for AuthResult and rejects with 401 if auth failed.

async fn auth_middleware(
    state: Arc<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> impl IntoResponse {
    // Allow unauthenticated access to health and docs routes
    let path = req.uri().path();
    let path_normalized = path.trim_end_matches('/');

    // Only allow public access for safe GET requests to the docs/api.json and health check
    // Normalize path to handle trailing slashes added by NormalizePathLayer
    if req.method() == Method::GET
        && (path_normalized == "/api/health_check"
            || path.starts_with("/api.json")
            || path.starts_with("/docs"))
    {
        return next.run(req).await;
    }

    // Allow POST to /api/get-auth-token without auth (public endpoint for obtaining JWTs)
    if req.method() == Method::POST && path_normalized == "/api/get-auth-token" {
        return next.run(req).await;
    }

    // Allow POST to /api/get-service-token without auth (public endpoint for service authentication)
    if req.method() == Method::POST && path_normalized == "/api/get-service-token" {
        return next.run(req).await;
    }

    // Quick Authorization header check
    let auth_header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = match auth_header.and_then(|s| s.strip_prefix("Bearer ")) {
        Some(t) => t.trim(),
        None => {
            // Store failure, let router decide if endpoint exists
            req.extensions_mut().insert(AuthResult::Failed(
                "missing or invalid authorization header".to_string(),
            ));
            return next.run(req).await;
        }
    };

    // Check if this is an API token (dpl_ prefix)
    if api_token_crypto::is_api_token(token) {
        // Validate API token format
        if !api_token_crypto::is_valid_token_format(token) {
            req.extensions_mut()
                .insert(AuthResult::Failed("invalid api token format".to_string()));
            return next.run(req).await;
        }

        // Hash the token and look it up in the database
        let token_hash = api_token_crypto::hash_token(token);
        let validation_result = select_api_token_by_hash(&state.db, &token_hash).await;

        match validation_result {
            Ok(Some(token_info)) => {
                // Check if token is revoked
                if token_info.revoked_at.is_some() {
                    req.extensions_mut()
                        .insert(AuthResult::Failed("api token has been revoked".to_string()));
                    return next.run(req).await;
                }

                // Check if token is expired
                if let Some(expires_at) = token_info.expires_at
                    && expires_at < OffsetDateTime::now_utc()
                {
                    req.extensions_mut()
                        .insert(AuthResult::Failed("api token has expired".to_string()));
                    return next.run(req).await;
                }

                // Determine effective roles based on token's is_admin flag
                let effective_roles = if token_info.is_admin {
                    // Admin token: use all user's roles from web.user_roles
                    token_info.user_roles.clone()
                } else {
                    // Regular token: only "user" role regardless of what user has
                    vec!["user".to_string()]
                };

                // Token is valid - store both markers
                let validated = ValidatedApiToken {
                    user_id: token_info.user_id,
                    token_id: token_info.id,
                    token_name: token_info.name,
                    roles: effective_roles,
                };
                req.extensions_mut().insert(validated);
                req.extensions_mut().insert(AuthResult::ValidApiToken);
                return next.run(req).await;
            }
            Ok(None) => {
                req.extensions_mut()
                    .insert(AuthResult::Failed("api token not found".to_string()));
                return next.run(req).await;
            }
            Err(e) => {
                req.extensions_mut().insert(AuthResult::Failed(format!(
                    "error validating api token: {}",
                    e
                )));
                return next.run(req).await;
            }
        }
    }

    // Verify the token as a signed JWT - secret must be configured
    let secret = match std::env::var("DAPPLE_PROMPT_API_SIGNING_KEY") {
        Ok(s) => s,
        Err(_) => {
            log::warn!("auth_middleware: DAPPLE_PROMPT_API_SIGNING_KEY not configured");
            req.extensions_mut()
                .insert(AuthResult::Failed("jwt secret not configured".to_string()));
            return next.run(req).await;
        }
    };

    // Manual JWT verification to work around jsonwebtoken's claims parsing issues.
    // jsonwebtoken has internal JSON parsing that fails on adjacently-tagged enums.
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        req.extensions_mut().insert(AuthResult::Failed(
            "malformed token: expected 3 parts".to_string(),
        ));
        return next.run(req).await;
    }

    let (header_b64, payload_b64, signature_b64) = (parts[0], parts[1], parts[2]);

    // Verify HMAC-SHA256 signature
    let message = format!("{}.{}", header_b64, payload_b64);
    let signature = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(signature_b64) {
        Ok(s) => s,
        Err(_) => {
            req.extensions_mut()
                .insert(AuthResult::Failed("invalid signature encoding".to_string()));
            return next.run(req).await;
        }
    };

    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(message.as_bytes());

    if mac.verify_slice(&signature).is_err() {
        log::warn!("auth_middleware: JWT signature verification failed");
        req.extensions_mut()
            .insert(AuthResult::Failed("invalid signature".to_string()));
        return next.run(req).await;
    }

    // Signature valid - decode and parse claims with serde_json
    let payload_bytes = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload_b64) {
        Ok(b) => b,
        Err(_) => {
            req.extensions_mut()
                .insert(AuthResult::Failed("invalid payload encoding".to_string()));
            return next.run(req).await;
        }
    };

    let payload_str = match String::from_utf8(payload_bytes) {
        Ok(s) => s,
        Err(_) => {
            req.extensions_mut()
                .insert(AuthResult::Failed("invalid UTF-8 in payload".to_string()));
            return next.run(req).await;
        }
    };

    // Check expiration manually
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload_str)
        && let Some(exp) = value.get("exp").and_then(|e| e.as_u64())
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if exp < now {
            log::warn!("auth_middleware: token expired (exp={}, now={})", exp, now);
            req.extensions_mut()
                .insert(AuthResult::Failed("token expired".to_string()));
            return next.run(req).await;
        }
    }

    // Parse claims
    let claims = match serde_json::from_str::<RawClaims>(&payload_str) {
        Ok(c) => c,
        Err(_) => {
            log::warn!(
                "auth_middleware: failed to parse claims, payload: {}",
                payload_str
            );
            req.extensions_mut()
                .insert(AuthResult::Failed("invalid claims format".to_string()));
            return next.run(req).await;
        }
    };

    log::debug!(
        "auth_middleware: parsed claims, method={} path={} token_use={}, sub={:?}",
        req.method(),
        req.uri().path(),
        claims.token_use,
        claims.sub
    );

    // Note: User existence is NOT checked here. The RegisteredUser extractor
    // handles user resolution (including auto-creation for new AuthUser refs via
    // resolve_user_id_ref). Checking here would block new users from bootstrapping.

    req.extensions_mut().insert(AuthResult::ValidJwt(claims));
    next.run(req).await
}

pub fn build_api_router(app_state: Arc<AppState>) -> ApiRouter {
    ApiRouter::new()
        .api_route("/api/health_check", get(health_checker_handler))
        .route("/api.json", get(serve_api_json))
        .route("/docs", Redoc::new("/api.json").axum_route())
        .nest("/api/playbooks/", playbooks::router())
        .nest("/api/hats/", hats::router())
        .nest("/api/steps/", steps::router())
        .nest("/api/tasks/", tasks::router())
        .nest("/api/categories/", categories::router())
        .nest("/api/stages/", stages::router())
        .nest("/api/journeys/", journeys::router())
        .nest("/api/dispatches/", dispatches::router())
        .nest(
            "/api/fluent-message-paths/",
            fluent_messages::fluent_message_paths_router(),
        )
        .nest(
            "/api/fluent-messages/",
            fluent_messages::fluent_messages_router(),
        )
        .nest(
            "/api/value-slot-paths/",
            value_slots::value_slot_paths_router(),
        )
        .nest("/api/value-slots/", value_slots::value_slots_router())
        .nest("/api/asset-paths/", assets::asset_paths_router())
        .nest("/api/assets/", assets::assets_router())
        .nest("/api/drafts/", drafts::drafts_router())
        .nest("/api/brief-paths/", briefs::brief_paths_router())
        .nest("/api/briefs/", briefs::briefs_router())
        .nest(
            "/api/brief-group-paths/",
            brief_groups::brief_group_paths_router(),
        )
        .nest("/api/brief-groups/", brief_groups::brief_groups_router())
        .nest(
            "/api/brief-pack-paths/",
            brief_packs::brief_pack_paths_router(),
        )
        .nest("/api/brief-packs/", brief_packs::brief_packs_router())
        .nest(
            "/api/dispatch-intent-bindings/",
            dispatch_intent_bindings::dispatch_intent_bindings_router(),
        )
        .nest("/api/prompt-paths/", prompts::prompt_paths_router())
        .nest("/api/prompts/", prompts::prompts_router())
        .nest(
            "/api/directive-paths/",
            directives::directive_paths_router(),
        )
        .nest("/api/directives/", directives::directives_router())
        .nest("/api/guides/", guides::router())
        .nest("/api/guide-search/", guide_search::router())
        .nest("/api/canonical/", canonical::router())
        .nest("/api/deprecations/", deprecations::router())
        .nest("/api/inference-profiles/", inference_profiles::router())
        .nest("/api/versioned-objects/", versioned_objects::router())
        .nest("/api/get-auth-token/", auth_token::router())
        .nest("/api/get-service-token", service_token::router())
        .nest("/api/api-tokens/", api_tokens::router())
        .nest("/api/companies/", companies::router())
        .nest(
            "/api/external-service-tokens/",
            external_service_tokens::router(),
        )
        .nest("/api/user-details/", user_details::router())
        // apply auth middleware by capturing the state in a closure (avoids from_fn_with_state inference issues)
        .layer(from_fn({
            let value = app_state.clone();
            move |req, next| {
                let s = value.clone();
                async move { auth_middleware(s, req, next).await }
            }
        }))
        .with_state(app_state.clone())
}

/// Build router structure for OpenAPI spec generation only.
/// Does not include auth middleware or state - used by mock servers.
pub fn build_spec_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route("/api/health_check", get(health_checker_handler))
        .nest("/api/playbooks/", playbooks::router())
        .nest("/api/hats/", hats::router())
        .nest("/api/steps/", steps::router())
        .nest("/api/tasks/", tasks::router())
        .nest("/api/categories/", categories::router())
        .nest("/api/stages/", stages::router())
        .nest("/api/journeys/", journeys::router())
        .nest("/api/dispatches/", dispatches::router())
        .nest(
            "/api/fluent-message-paths/",
            fluent_messages::fluent_message_paths_router(),
        )
        .nest(
            "/api/fluent-messages/",
            fluent_messages::fluent_messages_router(),
        )
        .nest(
            "/api/value-slot-paths/",
            value_slots::value_slot_paths_router(),
        )
        .nest("/api/value-slots/", value_slots::value_slots_router())
        .nest("/api/asset-paths/", assets::asset_paths_router())
        .nest("/api/assets/", assets::assets_router())
        .nest("/api/drafts/", drafts::drafts_router())
        .nest("/api/brief-paths/", briefs::brief_paths_router())
        .nest("/api/briefs/", briefs::briefs_router())
        .nest(
            "/api/brief-group-paths/",
            brief_groups::brief_group_paths_router(),
        )
        .nest("/api/brief-groups/", brief_groups::brief_groups_router())
        .nest(
            "/api/brief-pack-paths/",
            brief_packs::brief_pack_paths_router(),
        )
        .nest("/api/brief-packs/", brief_packs::brief_packs_router())
        .nest(
            "/api/dispatch-intent-bindings/",
            dispatch_intent_bindings::dispatch_intent_bindings_router(),
        )
        .nest("/api/prompt-paths/", prompts::prompt_paths_router())
        .nest("/api/prompts/", prompts::prompts_router())
        .nest(
            "/api/directive-paths/",
            directives::directive_paths_router(),
        )
        .nest("/api/directives/", directives::directives_router())
        .nest("/api/guides/", guides::router())
        .nest("/api/guide-search/", guide_search::router())
        .nest("/api/canonical/", canonical::router())
        .nest("/api/deprecations/", deprecations::router())
        .nest("/api/inference-profiles/", inference_profiles::router())
        .nest("/api/versioned-objects/", versioned_objects::router())
        .nest("/api/get-auth-token/", auth_token::router())
        .nest("/api/get-service-token", service_token::router())
        .nest("/api/api-tokens/", api_tokens::router())
        .nest("/api/companies/", companies::router())
        .nest(
            "/api/external-service-tokens/",
            external_service_tokens::router(),
        )
        .nest("/api/user-details/", user_details::router())
}
