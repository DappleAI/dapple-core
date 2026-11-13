mod config;
pub mod extract_path_uuid;
pub mod extract_qs;
mod extractor;
pub mod handler;
mod openapi_fixups;
pub mod route;
pub mod schema;
mod service;

pub use extractor::{RegisteredUser, RegisteredUserRejection, ServiceOrUser, UserScopedError};

// Re-export test utilities when test-helpers feature is enabled
#[cfg(feature = "test-helpers")]
pub mod test_utils {
    pub use crate::handler::guide_ingest::ingest_guide_directory;
    pub use crate::service::auth_token::test_utils::*;
    pub use crate::service::service_token::test_utils::*;
}

use axum::http::{
    Method, StatusCode,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
};
use route::build_api_router;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

use dapple_data::DbPool;

use aide::openapi::{Info, OpenApi};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::{Extension, Router};
pub use handler::{DeleteResponse, GetResponse, IndexResponse, PostResponse};

// Re-export the Ed25519 service-token verification utility so the
// dapple-prompt-external-api crate can reuse it without duplicating crypto
// code. TODO: extract this and ServiceTokenError to a small shared
// `dapple-auth` crate when we accumulate two or three more pieces of
// surface-agnostic auth logic.
pub use service::service_token::{
    ServiceTokenError, parse_ed25519_public_key, verify_service_token,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::normalize_path::NormalizePathLayer;
use uuid::Uuid;

pub struct AppState {
    pub db: Arc<DbPool>,
    /// Conduit service URL for embedding generation (e.g., "http://dapple-prompt-conduit:8081").
    /// When None, embedding generation is skipped silently after guide ingest.
    pub conduit_url: Option<String>,
    /// Flow service URL for dispatch execution (e.g., "http://dapple-prompt-flow:8082").
    /// When None, dispatches execute in-process via tokio::spawn (legacy fallback).
    pub flow_url: Option<String>,
}

/// Reference to a user identity in JWT claims.
/// Accepts either the internal auth.users.id or the user_details.id (UUID).
///
/// # JSON Representation
///
/// ```json
/// // Auth user ID (from NextAuth - i64)
/// { "type": "auth_user", "id": 123 }
///
/// // User ID (from user_details - UUID)
/// { "type": "object_id", "id": "550e8400-e29b-41d4-a716-446655440000" }
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum JWTUserIdRef {
    /// Internal auth.users.id from NextAuth (i64)
    AuthUser(i64),
    /// User details id (UUID)
    ObjectId(Uuid),
}

/// The `sub` claim in JWT tokens can be either:
/// - A JWTUserIdRef object (for delegated user tokens from dapple-ui)
/// - A string (for service tokens, e.g., "dapple_ui")
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ClaimsSub {
    /// User reference for delegated tokens (object with type/id)
    UserRef(JWTUserIdRef),
    /// Service identifier string for service tokens
    Service(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawClaims {
    pub sub: ClaimsSub,     // user reference (delegated) or service id string
    pub service_id: String, // "web", "worker", etc.
    pub token_use: String,  // "delegated" | "service"
    pub auth_user_id: Option<String>, // if delegated (legacy, kept for backwards compat)
    pub roles: Option<Vec<String>>, // if delegated
    pub exp: usize,         // required for jsonwebtoken expiration checks
}

#[derive(Debug, Clone)]
pub struct ServicePrincipal {
    pub id: String, // "web", "worker", etc.
}

#[derive(Debug, Clone)]
pub struct UserPrincipal {
    pub user_id_ref: JWTUserIdRef,
    pub auth_user_id: Option<String>, // Kept for backwards compat
    pub roles: Vec<String>,
}

/// Principal for API token authentication.
#[derive(Debug, Clone)]
pub struct ApiTokenPrincipal {
    /// User's id (UUID, from user_details)
    pub user_id: Uuid,
    /// Token's id (UUID)
    pub token_id: Uuid,
    /// Token name for identification
    pub token_name: String,
    /// Effective roles for this token (depends on is_admin flag)
    pub roles: Vec<String>,
}

/// Principal for service token authentication (dps_ tokens).
/// Used for internal service-to-service authentication with Ed25519 signatures.
#[derive(Debug, Clone)]
pub struct ServiceTokenPrincipal {
    /// Service token's id (UUID)
    pub service_token_id: Uuid,
    /// Service identifier (e.g., "conduit_main", "flow_worker")
    pub service_id: String,
}

#[derive(Debug, Clone)]
pub enum CallerKind {
    /// Pure service identity, no user involved (legacy shared JWT secret)
    ServiceOnly,
    /// Service acting on behalf of a user (JWT auth)
    DelegatedUser(UserPrincipal),
    /// API token authentication (dpl_ prefix)
    ApiToken(ApiTokenPrincipal),
    /// Service token authentication (dps_ prefix - asymmetric keys)
    ServiceToken(ServiceTokenPrincipal),
}

/// Internal struct passed through request extensions for validated API tokens.
/// This is set by the auth middleware when a dpl_ token is validated.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedApiToken {
    pub user_id: Uuid,
    pub token_id: Uuid,
    pub token_name: String,
    /// Effective roles for this token (computed based on is_admin flag)
    pub roles: Vec<String>,
}

/// Result of auth validation, stored in request extensions.
/// Allows routing to happen before auth rejection, so missing endpoints return 404
/// instead of 401.
#[derive(Debug, Clone)]
pub(crate) enum AuthResult {
    /// JWT token validated successfully
    ValidJwt(RawClaims),
    /// API token validated (ValidatedApiToken is also in extensions)
    ValidApiToken,
    /// Auth failed with reason
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub service: ServicePrincipal,
    pub caller_kind: CallerKind,
}

impl AuthContext {
    /// Get the user's id (UUID) if authenticated as a user with ObjectId variant.
    /// For AuthUser variant, returns None - use jwt_user_id_ref() instead.
    pub fn user_id(&self) -> Option<Uuid> {
        match &self.caller_kind {
            CallerKind::DelegatedUser(u) => match &u.user_id_ref {
                JWTUserIdRef::ObjectId(uuid) => Some(*uuid),
                JWTUserIdRef::AuthUser(_) => None,
            },
            CallerKind::ApiToken(t) => Some(t.user_id),
            CallerKind::ServiceOnly => None,
            CallerKind::ServiceToken(_) => None,
        }
    }

    /// Get the JWTUserIdRef from the caller, if available.
    /// This is the preferred method for resolving user identity.
    pub fn jwt_user_id_ref(&self) -> Option<JWTUserIdRef> {
        match &self.caller_kind {
            CallerKind::DelegatedUser(u) => Some(u.user_id_ref.clone()),
            CallerKind::ApiToken(t) => Some(JWTUserIdRef::ObjectId(t.user_id)),
            CallerKind::ServiceOnly => None,
            CallerKind::ServiceToken(_) => None,
        }
    }

    /// Get the user_id (UUID) for API token auth.
    pub fn api_token_user_id(&self) -> Option<Uuid> {
        match &self.caller_kind {
            CallerKind::ApiToken(t) => Some(t.user_id),
            _ => None,
        }
    }

    pub fn is_web(&self) -> bool {
        self.service.id == "web"
    }

    /// Check if authenticated via API token (dpl_ prefix).
    pub fn is_api_token(&self) -> bool {
        matches!(self.caller_kind, CallerKind::ApiToken(_))
    }

    /// Check if authenticated via service token (dps_ prefix).
    pub fn is_service_token(&self) -> bool {
        matches!(self.caller_kind, CallerKind::ServiceToken(_))
    }

    /// Get the service_id if authenticated via service token.
    pub fn service_token_id(&self) -> Option<&str> {
        match &self.caller_kind {
            CallerKind::ServiceToken(t) => Some(&t.service_id),
            _ => None,
        }
    }

    /// Get the API token's id if authenticated via API token.
    pub fn api_token_id(&self) -> Option<Uuid> {
        match &self.caller_kind {
            CallerKind::ApiToken(t) => Some(t.token_id),
            _ => None,
        }
    }

    /// Get the service token's id if authenticated via service token.
    pub fn service_token_id_uuid(&self) -> Option<Uuid> {
        match &self.caller_kind {
            CallerKind::ServiceToken(t) => Some(t.service_token_id),
            _ => None,
        }
    }
}

impl<S> FromRequestParts<S> for AuthContext
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    #[allow(clippy::manual_async_fn)]
    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        // Return an async block (the required Future). Captured references live long enough
        // for the future returned here because the future will be awaited by the caller.
        async move {
            // Check if middleware already validated auth and stored result
            if let Some(auth_result) = parts.extensions.get::<AuthResult>() {
                match auth_result {
                    AuthResult::Failed(reason) => {
                        // Auth failed - reject with 401
                        log::warn!("Auth failed: {}", reason);
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                    AuthResult::ValidApiToken => {
                        // API token path - ValidatedApiToken should be in extensions
                        if let Some(validated) = parts.extensions.get::<ValidatedApiToken>() {
                            return Ok(AuthContext {
                                service: ServicePrincipal {
                                    id: "api".to_string(),
                                },
                                caller_kind: CallerKind::ApiToken(ApiTokenPrincipal {
                                    user_id: validated.user_id,
                                    token_id: validated.token_id,
                                    token_name: validated.token_name.clone(),
                                    roles: validated.roles.clone(),
                                }),
                            });
                        }
                        // ValidApiToken without ValidatedApiToken is a bug
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                    AuthResult::ValidJwt(raw) => {
                        // Build AuthContext from pre-validated JWT claims
                        let service = ServicePrincipal {
                            id: raw.service_id.clone(),
                        };

                        let caller_kind = match raw.token_use.as_str() {
                            "delegated" => {
                                // Delegated tokens must have a JWTUserIdRef, not a string
                                let user_id_ref = match &raw.sub {
                                    ClaimsSub::UserRef(ref_) => ref_.clone(),
                                    ClaimsSub::Service(_) => {
                                        return Err(StatusCode::UNAUTHORIZED);
                                    }
                                };

                                let user = UserPrincipal {
                                    user_id_ref,
                                    auth_user_id: raw.auth_user_id.clone(),
                                    roles: raw.roles.clone().unwrap_or_default(),
                                };

                                CallerKind::DelegatedUser(user)
                            }
                            "service" => CallerKind::ServiceOnly,
                            _ => {
                                return Err(StatusCode::UNAUTHORIZED);
                            }
                        };

                        return Ok(AuthContext {
                            service,
                            caller_kind,
                        });
                    }
                }
            }

            // No AuthResult means middleware didn't run (public route) or bug
            // Fall back to checking for ValidatedApiToken (backwards compat)
            if let Some(validated) = parts.extensions.get::<ValidatedApiToken>() {
                let api_token_principal = ApiTokenPrincipal {
                    user_id: validated.user_id,
                    token_id: validated.token_id,
                    token_name: validated.token_name.clone(),
                    roles: validated.roles.clone(),
                };

                return Ok(AuthContext {
                    service: ServicePrincipal {
                        id: "api".to_string(),
                    },
                    caller_kind: CallerKind::ApiToken(api_token_principal),
                });
            }

            // No auth result and no validated token - reject
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

pub async fn build_app(_listener: &TcpListener, pool: DbPool) -> Router {
    let mut api = OpenApi {
        info: Info {
            title: "Dapple Prompt API".to_string(),
            description: Some("General Access Dapple Prompt API".to_string()),
            ..Info::default()
        },
        ..OpenApi::default()
    };

    // create the shared state once and use it both for building the router and as the router state
    let conduit_url = std::env::var("DAPPLE_PROMPT_CONDUIT_URL").ok();
    let flow_url = std::env::var("DAPPLE_PROMPT_FLOW_URL").ok();
    let db = Arc::new(pool);

    // Phase B content pipeline: when we're running the API in in-process
    // mode (no remote flow service), spawn the extraction/embedding worker
    // loop here so PDF ingests still get processed. When a remote flow
    // service is configured, the flow process owns the workers.
    if flow_url.is_none() {
        let worker_pool = db.clone();
        let worker_conduit_url = conduit_url.clone();
        tokio::spawn(async move {
            dapple_prompt_flow::run_extraction_workers(worker_pool, 2, worker_conduit_url).await;
        });

        // In-process execution mode: this process owns dispatch execution, so
        // stamp its identity on running events and sweep attempts a previous
        // incarnation left stranded mid-task (eternally `running` otherwise —
        // the recovery drain skips dispatches that already have attempts).
        // When a flow service is configured, the flow process owns both.
        dapple_prompt_flow::dispatch_runner::set_executor_identity("api");
        let sweep_pool = db.clone();
        tokio::spawn(async move {
            dapple_prompt_flow::dispatch_runner::fail_orphaned_attempts(&sweep_pool, "api").await;
        });
    }

    let state = Arc::new(AppState {
        db,
        conduit_url,
        flow_url,
    });
    let app = build_api_router(state.clone());

    // Use mirror_request() to echo back the Origin header from the request.
    // This is required when allow_credentials is true (wildcard * is not allowed).
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::mirror_request())
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_credentials(true)
        .allow_headers([AUTHORIZATION, ACCEPT, CONTENT_TYPE]);

    // Generate the documentation.
    // set the router state so extractors implementing FromRequestParts<S> receive &S
    let router = app.finish_api(&mut api);

    // Post-process: inject OpenAPI discriminator for tagged union schemas
    openapi_fixups::add_discriminators(&mut api);

    router
        // Expose the documentation to the handlers. Wrap in Arc so the
        // Extension layer's per-request clone is a refcount bump rather than
        // a deep copy of the entire schema graph — the latter was adding
        // ~1.4s of latency to every request (measured via /api/health_check
        // on localhost), and the per-request allocation churn was the main
        // contributor to the runaway memory growth under bursty BFF traffic.
        .layer(Extension(Arc::new(api)))
        .layer(cors)
        .layer(NormalizePathLayer::append_trailing_slash())
}

/// Generate the OpenAPI specification without requiring runtime state.
/// Used by mock servers to serve consistent documentation.
pub fn build_openapi_spec() -> OpenApi {
    let mut api = OpenApi {
        info: Info {
            title: "Dapple Prompt API".to_string(),
            description: Some("General Access Dapple Prompt API".to_string()),
            ..Info::default()
        },
        ..OpenApi::default()
    };

    // Build router without state value - type metadata is sufficient for spec generation
    let router = route::build_spec_router();
    let _ = router.finish_api(&mut api);

    // Post-process: inject OpenAPI discriminator for tagged union schemas
    openapi_fixups::add_discriminators(&mut api);

    api
}
