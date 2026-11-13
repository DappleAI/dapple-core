# Auth Token Verification Endpoint

**Dependencies:** `alloy-primitives`, `k256`

## Objective

Add a `POST /api/get-auth-token` endpoint that verifies Ethereum-signed auth tokens and issues JWTs. The auth token proves the caller (web service) is authorized; the JWT is a session token for subsequent API calls.

## Flow

```
┌─────────────┐     POST /api/get-auth-token      ┌──────────────────┐
│  Web Team   │ ──────────────────────────────────▶│  dapple-prompt   │
│  (caller)   │   { token: "...", user_id: "..." } │       API        │
└─────────────┘                                    └────────┬─────────┘
                                                           │
                                              1. Verify signature
                                              2. Check signer in allowlist
                                              3. Check token not expired
                                              4. Issue JWT with user_id as sub
                                                           │
                                                           ▼
┌─────────────┐         { jwt: "..." }            ┌──────────────────┐
│  Web Team   │ ◀──────────────────────────────────│  dapple-prompt   │
│             │                                    │       API        │
└─────────────┘                                    └──────────────────┘
```

**Key concept:** The auth token is proof of identity (signed by web team's private key). The `user_id` they provide is trusted once we verify the auth token. The JWT is a session token.

## Token Binary Format

The signed auth token is base64-encoded, 129 bytes:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Signature (65 bytes)                            │
├─────────────────────┬─────────────────────┬────────────────────────────┤
│      r (32)         │       s (32)        │         v (1)              │
└─────────────────────┴─────────────────────┴────────────────────────────┘
┌────────────────────────────────────────────────────────────────────────┐
│                         Message (64 bytes)                             │
├──────────┬───────────────────────┬───────────────────────┬─────────────┤
│ ver (1)  │    valid_from (8)     │    valid_until (8)    │ padding (47)│
└──────────┴───────────────────────┴───────────────────────┴─────────────┘
```

- **version:** 1 byte, must be `0x01`
- **valid_from:** 8 bytes, Unix timestamp (seconds), big-endian
- **valid_until:** 8 bytes, Unix timestamp (seconds), big-endian
- **max duration:** 30 minutes (enforced)

## Request/Response

### Request

```json
POST /api/get-auth-token
Content-Type: application/json

{
  "token": "base64-encoded-129-byte-signed-token",
  "user_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

- `token`: The Ethereum-signed auth token (proof of caller identity)
- `user_id`: UUID of the user the caller is acting on behalf of (from `rev.user_details.object_id`)

### Response (Success)

```json
{
  "status": "ok",
  "jwt": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "signer": "0x1234...abcd",
  "expires_at": "2024-01-15T10:30:00+00:00"
}
```

Note: `expires_at` is a string in RFC 3339/ISO 8601 format.

### Response (Error)

```json
{
  "error": "SIGNER_NOT_AUTHORIZED",
  "message": "SIGNER_NOT_AUTHORIZED: address 0x1234...abcd is not in the allowlist"
}
```

Error codes:
- `MALFORMED_TOKEN` - Token structure invalid (includes details like "invalid base64" or "expected 129 bytes, got N")
- `INVALID_SIGNATURE` - Signature verification failed
- `INVALID_VERSION` - Version byte not supported (includes expected and actual values)
- `DURATION_EXCEEDS_MAX` - Token validity > 30 minutes (includes duration and max)
- `NOT_YET_VALID` - Current time before valid_from (includes timestamps)
- `EXPIRED` - Current time after valid_until (includes timestamps)
- `SIGNER_NOT_AUTHORIZED` - Recovered address not in allowlist (includes the address)
- `SERVICE_UNAVAILABLE` - Missing configuration

## JWT Claims

```json
{
  "sub": "550e8400-e29b-41d4-a716-446655440000",
  "service_id": "web",
  "token_use": "delegated",
  "auth_user_id": null,
  "roles": ["user"],
  "exp": 1705312200
}
```

- `sub`: The `user_id` from the request (UUID)
- `service_id`: Always "web" for this endpoint
- `token_use`: "delegated" (service acting on behalf of user)
- `exp`: JWT expiry (configurable, default 1 hour)

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `DAPPLE_PROMPT_API_JWT_SECRET` | Yes | Secret for signing JWTs |
| `AUTH_TOKEN_ALLOWED_SIGNERS` | Yes | Comma-separated Ethereum addresses |
| `AUTH_TOKEN_JWT_EXPIRY_SECS` | No | JWT lifetime (default: 3600) |

## Implementation Status

**Status: Core implementation complete**

- 6 unit tests passing (4 in service, 2 in config)
- All files created as planned
- Endpoint functional and ready for integration testing

### Files Created

| File | Description |
|------|-------------|
| `src/service/mod.rs` | Module declaration for services |
| `src/service/auth_token.rs` | Token verification logic (277 lines) |
| `src/config.rs` | Environment config helpers |
| `src/schema/auth_token.rs` | Request/response schemas |
| `src/handler/auth_token.rs` | HTTP handler |
| `src/route/auth_token.rs` | Route registration |

### Remaining Work

- [ ] Mock/test utilities for generating valid tokens
- [ ] Integration tests with full HTTP stack

## Implementation Reference

### Token Verification Service

```rust
// crates/dapple-prompt-api/src/service/auth_token.rs

use alloy_primitives::Address;
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use sha3::{Digest, Keccak256};

pub const TOKEN_VERSION: u8 = 0x01;
pub const MAX_DURATION_SECS: u64 = 1800; // 30 minutes
pub const EXPECTED_TOKEN_LEN: usize = 129;

#[derive(Debug, thiserror::Error)]
pub enum AuthTokenError {
    #[error("MALFORMED_TOKEN: {0}")]
    MalformedToken(String),

    #[error("INVALID_SIGNATURE: failed to recover signer from signature")]
    InvalidSignature,

    #[error("INVALID_VERSION: expected version {expected}, got {actual}")]
    InvalidVersion { expected: u8, actual: u8 },

    #[error("DURATION_EXCEEDS_MAX: token duration {duration_secs}s exceeds maximum {max_secs}s")]
    DurationExceedsMax { duration_secs: u64, max_secs: u64 },

    #[error("NOT_YET_VALID: token becomes valid at {valid_from}, current time is {current}")]
    NotYetValid { valid_from: u64, current: u64 },

    #[error("EXPIRED: token expired at {valid_until}, current time is {current}")]
    Expired { valid_until: u64, current: u64 },

    #[error("SIGNER_NOT_AUTHORIZED: address {signer} is not in the allowlist")]
    SignerNotAuthorized { signer: Address },
}

/// Result of successful token verification
pub struct VerifiedToken {
    pub signer: Address,
    pub valid_from: u64,
    pub valid_until: u64,
}

pub fn verify_auth_token(
    token: &str,
    allowed_signers: Option<&[Address]>,
) -> Result<VerifiedToken, AuthTokenError> {
    // Implementation verifies base64, length, version, timing, signature, allowlist
    // ...
}
```

### Handler (No State Required)

```rust
// crates/dapple-prompt-api/src/handler/auth_token.rs

/// Handler for `POST /api/get-auth-token`
/// Note: Does not take State - reads configuration from environment directly
pub async fn get_auth_token_handler(
    Json(req): Json<GetAuthTokenRequest>,
) -> impl IntoApiResponse {
    // 1. Check if endpoint is enabled
    // 2. Load configuration from env
    // 3. Verify auth token
    // 4. Build JWT claims with req.user_id as sub
    // 5. Sign and return JWT
}
```

### Request/Response Schemas

```rust
// crates/dapple-prompt-api/src/schema/auth_token.rs

#[derive(JsonSchema, Serialize, Deserialize)]
pub struct GetAuthTokenRequest {
    pub token: String,
    pub user_id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize)]
pub struct GetAuthTokenResponse {
    pub status: String,
    pub jwt: String,
    pub signer: String,
    pub expires_at: String,  // RFC 3339 format
}

#[derive(JsonSchema, Serialize, Deserialize)]
pub struct AuthTokenErrorResponse {
    pub error: String,
    pub message: String,
}
```

### RecoveryId Handling

The implementation uses `RecoveryId::new(is_y_odd, is_x_reduced)` with two boolean arguments:

```rust
// Convert v to recovery id (Ethereum uses 27/28, we need 0/1)
// RecoveryId::new(is_y_odd, is_x_reduced)
// For standard Ethereum signatures, is_x_reduced is always false
let recovery_id = match v {
    27 | 0 => RecoveryId::new(false, false),
    28 | 1 => RecoveryId::new(true, false),
    _ => return Err(AuthTokenError::InvalidSignature),
};
```

## Acceptance Criteria

- [x] `POST /api/get-auth-token` endpoint exists
- [x] Verifies EIP-191 (personal_sign) signatures
- [x] Checks signer against `AUTH_TOKEN_ALLOWED_SIGNERS` allowlist
- [x] Validates token version, timing, and duration
- [x] Issues JWT with `user_id` from request as `sub` claim
- [x] Returns appropriate error codes for all failure cases
- [x] Endpoint is public (no JWT required to call it)
- [ ] Mock/test utilities can generate valid tokens (deferred)
- [ ] Integration tests pass (unit tests pass, integration tests deferred)

## Security Notes

- The auth token proves the caller is who they claim (signature verification)
- The `user_id` is trusted after auth token verification
- Allowed signers list should contain only trusted service addresses
- JWT secret must be kept secure
- Token max duration (30 min) limits replay window
