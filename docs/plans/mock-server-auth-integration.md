# Mock API Auth Integration Plan

## Overview

Update `dapple-prompt-mock-api` to fully mirror production authentication:
1. Generate Ethereum keypair on startup (for signing auth tokens)
2. Implement `/api/get-auth-token/` with full signature verification
3. Validate JWTs on protected routes
4. Mirror production behavior as closely as possible

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        dapple-prompt-mock-api                           │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Startup:                                                               │
│  1. Generate fresh Ethereum keypair                                     │
│  2. Log address (add to AUTH_TOKEN_ALLOWED_SIGNERS)                     │
│  3. Load DAPPLE_PROMPT_API_JWT_SECRET from env                                       │
│                                                                         │
│  Endpoints (mirroring production):                                      │
│  ┌───────────────────────────────────────────────────────────────────┐ │
│  │  POST /api/get-auth-token  - Verify signature, issue JWT          │ │
│  │  GET  /api/journeys        - JWT required                         │ │
│  │  POST /api/journeys        - JWT required                         │ │
│  │  ... all other routes      - JWT required                         │ │
│  │  GET  /api/health_check    - No auth                              │ │
│  └───────────────────────────────────────────────────────────────────┘ │
│                                                                         │
│  Test utility endpoints (mock-only):                                    │
│  ┌───────────────────────────────────────────────────────────────────┐ │
│  │  GET  /mock/address        - Get mock's Ethereum address          │ │
│  │  GET  /mock/auth-token     - Get signed auth token (convenience)  │ │
│  └───────────────────────────────────────────────────────────────────┘ │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## Current State

`dapple-prompt-mock-api` has:
- Axum router mirroring production routes
- `MockAuthConfig` with `enabled` and `jwt_secret` fields
- `MockState` with `with_auth()` builder

Missing:
- `/api/get-auth-token/` endpoint
- Ethereum keypair generation
- Auth token verification
- JWT validation on routes

## Implementation

### Phase 1: Add Dependencies

**Update `Cargo.toml`:**

```toml
[dependencies]
# Existing deps...

# Auth token verification (same as dapple-prompt-api)
alloy-primitives = "0.8"
k256 = { version = "0.13", features = ["ecdsa"] }
sha3 = "0.10"
base64 = "0.22"
rand = "0.8"

# JWT
jsonwebtoken = "9"
chrono = { version = "0.4", features = ["serde"] }
```

### Phase 2: Update State with Keypair

**Update `src/state.rs`:**

```rust
use alloy_primitives::Address;
use k256::ecdsa::SigningKey;
use rand::rngs::OsRng;
use sha3::{Digest, Keccak256};

/// Configuration for authentication
#[derive(Clone)]
pub struct MockAuthConfig {
    pub enabled: bool,
    pub jwt_secret: Option<String>,
    pub allowed_signers: Vec<Address>,
}

impl Default for MockAuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            jwt_secret: None,
            allowed_signers: Vec::new(),
        }
    }
}

/// Shared state for the mock API server
pub struct MockState {
    pub store: Arc<RwLock<InMemoryStore>>,
    pub auth_config: MockAuthConfig,
    /// Mock's own signing key for generating test auth tokens
    pub signing_key: SigningKey,
    /// Mock's Ethereum address (derived from signing_key)
    pub address: Address,
}

impl MockState {
    pub fn new(store: InMemoryStore) -> Self {
        let (signing_key, address) = generate_keypair();

        Self {
            store: Arc::new(RwLock::new(store)),
            auth_config: MockAuthConfig::default(),
            signing_key,
            address,
        }
    }

    pub fn with_auth(mut self, config: MockAuthConfig) -> Self {
        // Auto-add mock's own address to allowed signers
        let mut config = config;
        if !config.allowed_signers.contains(&self.address) {
            config.allowed_signers.push(self.address);
        }
        self.auth_config = config;
        self
    }
}

fn generate_keypair() -> (SigningKey, Address) {
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let pk_bytes = verifying_key.to_encoded_point(false);

    let mut hasher = Keccak256::new();
    hasher.update(&pk_bytes.as_bytes()[1..]); // skip 0x04 prefix
    let hash = hasher.finalize();

    (signing_key, Address::from_slice(&hash[12..]))
}
```

### Phase 3: Auth Token Verification Service

Create auth token verification that mirrors production.

**Create `src/service/mod.rs`:**

```rust
pub mod auth_token;
```

**Create `src/service/auth_token.rs`:**

Copy the implementation from `dapple-prompt-api/src/service/auth_token.rs` - it should be identical to ensure behavior matches.

```rust
// Same implementation as production:
// - verify_auth_token()
// - AuthTokenError enum
// - VerifiedToken struct
// - Constants (TOKEN_VERSION, MAX_DURATION_SECS, EXPECTED_TOKEN_LEN)
```

### Phase 4: Auth Token Handler

**Create `src/handler/auth_token.rs`:**

```rust
use crate::service::auth_token::{verify_auth_token, AuthTokenError};
use crate::state::MockState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct GetAuthTokenRequest {
    pub token: String,
    pub user_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct GetAuthTokenResponse {
    pub status: String,
    pub jwt: String,
    pub signer: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct AuthTokenErrorResponse {
    pub error: String,
    pub message: String,
}

impl AuthTokenErrorResponse {
    pub fn new(error: &str, message: impl Into<String>) -> Self {
        Self {
            error: error.to_string(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    sub: String,
    service_id: String,
    token_use: String,
    auth_user_id: Option<String>,
    roles: Vec<String>,
    exp: usize,
}

pub async fn get_auth_token_handler(
    State(state): State<Arc<MockState>>,
    Json(req): Json<GetAuthTokenRequest>,
) -> impl IntoResponse {
    // Check if auth is enabled
    if !state.auth_config.enabled {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AuthTokenErrorResponse::new(
                "SERVICE_UNAVAILABLE",
                "Auth is not enabled on this mock server",
            )),
        )
            .into_response();
    }

    // Get JWT secret
    let jwt_secret = match &state.auth_config.jwt_secret {
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

    // Verify the auth token
    let verified = match verify_auth_token(&req.token, Some(&state.auth_config.allowed_signers)) {
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

    // Build JWT claims
    let expires_at = Utc::now() + Duration::hours(1);
    let claims = JwtClaims {
        sub: req.user_id.to_string(),
        service_id: "web".to_string(),
        token_use: "delegated".to_string(),
        auth_user_id: None,
        roles: vec!["user".to_string()],
        exp: expires_at.timestamp() as usize,
    };

    // Sign JWT
    let jwt = match encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
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
```

### Phase 5: JWT Validation Extractor

**Create `src/auth.rs`:**

```rust
use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::state::MockState;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Claims {
    pub sub: String,
    pub service_id: String,
    pub token_use: String,
    pub auth_user_id: Option<String>,
    pub roles: Vec<String>,
    pub exp: usize,
}

#[derive(Debug, Clone)]
pub struct MockAuthContext {
    pub claims: Claims,
    pub user_id: Option<Uuid>,
}

#[derive(Serialize)]
struct AuthError {
    error: &'static str,
    message: String,
}

#[axum::async_trait]
impl FromRequestParts<Arc<MockState>> for MockAuthContext {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<MockState>,
    ) -> Result<Self, Self::Rejection> {
        // If auth disabled, return dummy context
        if !state.auth_config.enabled {
            return Ok(MockAuthContext {
                claims: Claims {
                    sub: "mock-user".to_string(),
                    service_id: "mock".to_string(),
                    token_use: "delegated".to_string(),
                    auth_user_id: None,
                    roles: vec!["user".to_string()],
                    exp: 0,
                },
                user_id: None,
            });
        }

        let secret = state.auth_config.jwt_secret.as_ref().ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AuthError {
                    error: "CONFIG_ERROR",
                    message: "JWT secret not configured".to_string(),
                }),
            )
                .into_response()
        })?;

        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(AuthError {
                        error: "MISSING_AUTH",
                        message: "Missing Authorization header".to_string(),
                    }),
                )
                    .into_response()
            })?;

        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(AuthError {
                    error: "INVALID_AUTH",
                    message: "Authorization header must be 'Bearer <token>'".to_string(),
                }),
            )
                .into_response()
        })?;

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|e| {
            (
                StatusCode::UNAUTHORIZED,
                Json(AuthError {
                    error: "INVALID_TOKEN",
                    message: format!("JWT validation failed: {}", e),
                }),
            )
                .into_response()
        })?;

        let claims = token_data.claims;
        let user_id = Uuid::parse_str(&claims.sub).ok();

        Ok(MockAuthContext { claims, user_id })
    }
}
```

### Phase 6: Mock Utility Endpoints

For test convenience, add endpoints to get the mock's address and generate auth tokens.

**Create `src/handler/mock_utils.rs`:**

```rust
use crate::state::MockState;
use axum::{extract::State, Json};
use base64::Engine;
use k256::ecdsa::SigningKey;
use serde::Serialize;
use sha3::{Digest, Keccak256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
pub struct AddressResponse {
    pub address: String,
}

pub async fn get_address_handler(
    State(state): State<Arc<MockState>>,
) -> Json<AddressResponse> {
    Json(AddressResponse {
        address: format!("{:#x}", state.address),
    })
}

#[derive(Serialize)]
pub struct AuthTokenResponse {
    pub token: String,
    pub address: String,
    pub valid_for_secs: u64,
}

pub async fn get_mock_auth_token_handler(
    State(state): State<Arc<MockState>>,
) -> Json<AuthTokenResponse> {
    let valid_for_secs = 300; // 5 minutes
    let token = sign_auth_token(&state.signing_key, valid_for_secs);

    Json(AuthTokenResponse {
        token,
        address: format!("{:#x}", state.address),
        valid_for_secs,
    })
}

fn sign_auth_token(signing_key: &SigningKey, valid_for_secs: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Build message (64 bytes)
    let mut msg = vec![0x01u8]; // version
    msg.extend_from_slice(&now.to_be_bytes()); // valid_from
    msg.extend_from_slice(&(now + valid_for_secs).to_be_bytes()); // valid_until
    msg.resize(64, 0); // pad

    // EIP-191 hash
    let prefixed = format!("\x19Ethereum Signed Message:\n{}", msg.len());
    let mut hasher = Keccak256::new();
    hasher.update(prefixed.as_bytes());
    hasher.update(&msg);
    let digest: [u8; 32] = hasher.finalize().into();

    // Sign
    let (sig, rec_id) = signing_key.sign_prehash_recoverable(&digest).unwrap();
    let mut token = sig.to_bytes().to_vec();
    token.push(rec_id.to_byte() + 27);
    token.extend_from_slice(&msg);

    base64::engine::general_purpose::STANDARD.encode(&token)
}
```

### Phase 7: Update Routes

**Update `src/route/mod.rs`:**

```rust
use axum::routing::{get, post};
// ... existing imports

pub fn build_router(state: Arc<MockState>) -> Router {
    // Log mock's address on startup
    tracing::info!("===========================================");
    tracing::info!("Mock server Ethereum address: {:#x}", state.address);
    tracing::info!("===========================================");

    Router::new()
        // Documentation routes
        .route("/api.json", get(serve_api_json))
        .route("/docs", get(serve_docs))
        // Health check (no auth)
        .route("/api/health_check", get(handler::health_checker_handler))
        // Auth token endpoint (no auth required - mirrors production)
        .route("/api/get-auth-token", post(handler::auth_token::get_auth_token_handler))
        // Mock utility endpoints (no auth)
        .route("/mock/address", get(handler::mock_utils::get_address_handler))
        .route("/mock/auth-token", get(handler::mock_utils::get_mock_auth_token_handler))
        // All other routes (require auth when enabled)
        .nest("/api/categories", categories_router())
        .nest("/api/hats", hats_router())
        // ... rest of routes
        .with_state(state)
}
```

### Phase 8: Update Handlers to Use Auth

Add `MockAuthContext` to handlers that need authentication.

```rust
// Example: journeys handler
pub async fn journeys_index_handler(
    _auth: MockAuthContext,  // Validates JWT when auth enabled
    State(state): State<Arc<MockState>>,
) -> impl IntoResponse {
    // ... existing logic
}
```

## Testing Flow

### 1. Start mock with auth enabled

```rust
let state = Arc::new(
    MockState::new(store)
        .with_auth(MockAuthConfig {
            enabled: true,
            jwt_secret: Some("test-secret".to_string()),
            allowed_signers: vec![], // Mock's address auto-added
        })
);
```

### 2. Get mock's auth token

```bash
curl http://localhost:8080/mock/auth-token
# Returns: { "token": "...", "address": "0x...", "valid_for_secs": 300 }
```

### 3. Exchange for JWT

```bash
curl -X POST http://localhost:8080/api/get-auth-token \
  -H "Content-Type: application/json" \
  -d '{"token": "<token-from-step-2>", "user_id": "550e8400-e29b-41d4-a716-446655440000"}'
# Returns: { "status": "ok", "jwt": "...", "signer": "0x...", "expires_at": "..." }
```

### 4. Use JWT for API calls

```bash
curl http://localhost:8080/api/journeys \
  -H "Authorization: Bearer <jwt-from-step-3>"
```

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `Cargo.toml` | Modify | Add crypto and JWT dependencies |
| `src/state.rs` | Modify | Add keypair, update MockAuthConfig |
| `src/service/mod.rs` | Create | Module declaration |
| `src/service/auth_token.rs` | Create | Copy from dapple-prompt-api |
| `src/auth.rs` | Create | JWT validation extractor |
| `src/handler/auth_token.rs` | Create | Auth token endpoint handler |
| `src/handler/mock_utils.rs` | Create | Mock utility handlers |
| `src/handler/mod.rs` | Modify | Add new handler modules |
| `src/route/mod.rs` | Modify | Add new routes |
| `src/lib.rs` | Modify | Export new modules |

## Acceptance Criteria

- [ ] Mock generates Ethereum keypair on startup
- [ ] Mock logs its address for easy allowlist configuration
- [ ] `POST /api/get-auth-token` mirrors production behavior exactly
- [ ] JWT validation on protected routes matches production
- [ ] `GET /mock/address` returns mock's Ethereum address
- [ ] `GET /mock/auth-token` returns signed auth token for testing
- [ ] When auth disabled, all requests pass through (existing behavior)
- [ ] Error responses match production format
- [ ] Full end-to-end flow works: get token → exchange for JWT → make authenticated request
