# API Tokens Plan

## Overview

Implement opaque API tokens for programmatic access (mobile apps, CLI tools, integrations). These are distinct from the delegated JWT flow used for web sessions.

**Key characteristics:**
- Opaque tokens (random string, stored hashed in DB)
- User-scoped (company scope added later)
- User-named for easy management
- User-selectable expiration
- Instant revocation via DB deletion
- Different rate limits than web sessions

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Token Creation Flow                              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Web UI (authenticated via JWT)                                         │
│       │                                                                 │
│       ▼                                                                 │
│  POST /api/api-tokens                                                   │
│  { "name": "My Mobile App", "expires_in": "90_days" }                   │
│       │                                                                 │
│       ▼                                                                 │
│  API generates token: dpl_a1b2c3d4e5f6...                               │
│  Stores SHA-256 hash in DB                                              │
│       │                                                                 │
│       ▼                                                                 │
│  Response: { "token": "dpl_a1b2c3...", "id": "...", ... }               │
│  (Token shown ONCE, user must copy)                                     │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────┐
│                         Token Usage Flow                                 │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Mobile App / API Client                                                │
│       │                                                                 │
│       ▼                                                                 │
│  GET /api/journeys                                                      │
│  Authorization: Bearer dpl_a1b2c3d4e5f6...                              │
│       │                                                                 │
│       ▼                                                                 │
│  API detects "dpl_" prefix → API token path                             │
│  Hash token → lookup in api_tokens table                                │
│  Check: not revoked, not expired, user active                           │
│       │                                                                 │
│       ▼                                                                 │
│  Apply API token rate limits                                            │
│  Execute request as user                                                │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## Data Model

### Table: `web.api_tokens`

```sql
CREATE TABLE web.api_tokens (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_id BIGINT NOT NULL REFERENCES auth.users(id),

    -- User-provided metadata
    name TEXT NOT NULL,

    -- Token data (we only store the hash)
    token_hash BYTEA NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL CHECK (LENGTH(token_prefix) = 8),  -- "dpl_xxxx" for display

    -- Lifecycle
    expires_at TIMESTAMPTZ,  -- NULL = never expires
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_api_tokens_user_id ON web.api_tokens(user_id);
CREATE INDEX idx_api_tokens_token_hash ON web.api_tokens(token_hash);
```

### Table: `web.api_token_revocations` (append-only)

```sql
CREATE TABLE web.api_token_revocations (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    api_token_id BIGINT NOT NULL REFERENCES web.api_tokens(id),
    reason TEXT,  -- optional: "user_requested", "security_concern", etc.
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_api_token_revocations_api_token_id ON web.api_token_revocations(api_token_id);
```

**Notes:**
- `token_hash`: SHA-256 hash of the full token
- `token_prefix`: First 8 chars (e.g., "dpl_a1b2") for display in UI listings
- `expires_at`: NULL means never expires
- Revocation is append-only: INSERT into `api_token_revocations` to revoke
- DTO exposes `revoked_at: Option<DateTime<Utc>>` (nullable timestamp, not boolean) via LEFT JOIN
- Token is revoked when `revoked_at.is_some()`
- Future: Add `company_id` column when company model lands

### Token Format

```
dpl_<32 random bytes as base62>

Example: dpl_7Kj2mNpQrStUvWxYz3AbCdEfGhIjKlMn
         ├──┘└─────────────────────────────────┘
         │              │
      prefix      random payload (43 chars)
      (4 chars)

Total length: 47 characters
```

**Why this format:**
- `dpl_` prefix: Easy identification, prevents accidental use as JWT
- Base62 encoding: URL-safe, no special characters
- 32 bytes entropy: 256 bits, cryptographically secure

## API Endpoints

### Create Token

```
POST /api/api-tokens
Authorization: Bearer <jwt>  (web session required)

Request:
{
    "name": "My Mobile App",
    "expires_in": "90_days"  // "30_days" | "90_days" | "1_year" | "never"
}

Response (201 Created):
{
    "object_id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "My Mobile App",
    "token": "dpl_7Kj2mNpQrStUvWxYz3AbCdEfGhIjKlMn",  // ONLY TIME THIS IS RETURNED
    "token_prefix": "dpl_7Kj2",
    "expires_at": "2025-04-21T00:00:00Z",
    "created_at": "2025-01-21T00:00:00Z"
}
```

**Important:** The `token` field is only returned on creation. After this, only `token_prefix` is available.

### List Tokens

```
GET /api/api-tokens
Authorization: Bearer <jwt>  (web session required)

Response (200 OK):
{
    "tokens": [
        {
            "object_id": "550e8400-e29b-41d4-a716-446655440000",
            "name": "My Mobile App",
            "token_prefix": "dpl_7Kj2",
            "expires_at": "2025-04-21T00:00:00Z",
            "revoked_at": null,
            "created_at": "2025-01-21T00:00:00Z"
        },
        {
            "object_id": "661e8400-e29b-41d4-a716-446655440001",
            "name": "CI Pipeline",
            "token_prefix": "dpl_xYz9",
            "expires_at": null,
            "revoked_at": "2025-01-15T00:00:00Z",
            "created_at": "2024-12-01T00:00:00Z"
        }
    ]
}
```

### Revoke Token

```
DELETE /api/api-tokens/:object_id
Authorization: Bearer <jwt>  (web session required)

Response (204 No Content)
```

INSERTs a row into `api_token_revocations`. Token immediately stops working.

## Authentication Flow

### Header Detection

```rust
// In auth middleware/extractor
fn extract_auth(header: &str) -> AuthMethod {
    if let Some(token) = header.strip_prefix("Bearer ") {
        if token.starts_with("dpl_") {
            AuthMethod::ApiToken(token.to_string())
        } else {
            AuthMethod::Jwt(token.to_string())
        }
    } else {
        AuthMethod::None
    }
}
```

### Token Validation

```rust
pub async fn validate_api_token(
    pool: &PgPool,
    token: &str,
) -> Result<ApiTokenContext, AuthError> {
    // 1. Verify prefix
    if !token.starts_with("dpl_") {
        return Err(AuthError::InvalidToken);
    }

    // 2. Hash the token
    let hash = sha256(token.as_bytes());

    // 3. Lookup in DB (query joins with revocations table)
    let row = sqlx::query_file_as!(
        ApiTokenRow,
        "assets/sql/api_tokens/select_by_hash.sql",
        &hash
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AuthError::InvalidToken)?;

    // 4. Check not revoked (revoked_at computed via LEFT JOIN in query)
    if row.revoked_at.is_some() {
        return Err(AuthError::TokenRevoked);
    }

    // 5. Check not expired
    if let Some(expires_at) = row.expires_at {
        if expires_at < Utc::now() {
            return Err(AuthError::TokenExpired);
        }
    }

    // 6. Return context
    Ok(ApiTokenContext {
        user_id: row.user_id,
        user_object_id: row.user_object_id,
        token_object_id: row.object_id,
        token_name: row.name,
    })
}
```

### Unified Auth Context

```rust
/// Authentication context available to handlers
pub enum AuthContext {
    /// Web session via JWT (delegated from wallet signature)
    WebSession {
        user_id: i64,
        user_object_id: Uuid,
        service_id: String,
        roles: Vec<String>,
    },
    /// API token (programmatic access)
    ApiToken {
        user_id: i64,
        user_object_id: Uuid,
        token_object_id: Uuid,
        token_name: String,
    },
}

impl AuthContext {
    pub fn user_id(&self) -> i64 {
        match self {
            Self::WebSession { user_id, .. } => *user_id,
            Self::ApiToken { user_id, .. } => *user_id,
        }
    }

    pub fn is_api_token(&self) -> bool {
        matches!(self, Self::ApiToken { .. })
    }
}
```

## Rate Limiting

API tokens have separate rate limit buckets from web sessions.

| Auth Type | Requests/min | Requests/hour | Burst |
|-----------|-------------|---------------|-------|
| Web Session (JWT) | 300 | 10,000 | 50 |
| API Token | 60 | 1,000 | 10 |

**Implementation notes:**
- Rate limit key: `api_token:{token_object_id}` vs `session:{user_id}`
- Consider per-endpoint limits for expensive operations
- Return `429 Too Many Requests` with `Retry-After` header

## Implementation Phases

### Phase 1: Data Layer

1. Create migration for `web.api_tokens` and `web.api_token_revocations` tables
2. Create models in `dapple-data`:
   - `ApiToken` (DTO, includes `revoked_at: Option<DateTime<Utc>>`)
   - `ApiTokenModel` (internal)
   - `FlatApiTokenModel` (includes `revoked_at` from LEFT JOIN)
   - `InsertApiTokenArgs`
   - `InsertedApiTokenModel`
   - `ApiTokenRevocationModel`
3. Create queries:
   - `insert_api_tokens.sql`
   - `select_api_tokens_by_user.sql` (LEFT JOIN revocations for `revoked_at`)
   - `select_api_token_by_hash.sql` (LEFT JOIN revocations for `revoked_at`)
   - `insert_api_token_revocation.sql` (append-only revocation)

### Phase 2: Token Generation

1. Add `rand` and `sha2` dependencies
2. Create `src/service/api_token.rs`:
   - `generate_token() -> (String, Vec<u8>)` - returns (plaintext, hash)
   - `hash_token(token: &str) -> Vec<u8>`

### Phase 3: API Endpoints

1. Create handler `src/handler/api_tokens.rs`:
   - `create_api_token` - POST, requires JWT auth
   - `list_api_tokens` - GET, requires JWT auth
   - `revoke_api_token` - DELETE, requires JWT auth
2. Add routes to router
3. Create schemas in `src/schema/api_tokens.rs`

### Phase 4: Auth Middleware Update

1. Update auth extractor to detect `dpl_` prefix
2. Add `validate_api_token` function
3. Create unified `AuthContext` enum
4. Update existing handlers to use `AuthContext`

### Phase 5: Rate Limiting

1. Add rate limit middleware/extractor
2. Configure separate buckets for API tokens
3. Add `Retry-After` header support

### Phase 6: Testing

1. Unit tests for token generation/hashing
2. Integration tests for full CRUD flow
3. Test token validation (valid, expired, revoked)
4. Test rate limiting buckets

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `migrations/YYYYMMDD_api_tokens.sql` | Create | Tables: `web.api_tokens`, `web.api_token_revocations` |
| `dapple-data/src/model/api_tokens.rs` | Create | Models |
| `dapple-data/src/query/api_tokens.rs` | Create | Query functions |
| `dapple-data/assets/sql/api_tokens/*.sql` | Create | SQL queries |
| `dapple-prompt-api/src/service/api_token.rs` | Create | Token generation |
| `dapple-prompt-api/src/handler/api_tokens.rs` | Create | Endpoints |
| `dapple-prompt-api/src/schema/api_tokens.rs` | Create | Request/response schemas |
| `dapple-prompt-api/src/auth.rs` | Modify | Add API token validation path |
| `dapple-prompt-api/src/route/mod.rs` | Modify | Add routes |

## Security Considerations

1. **Token entropy**: 256 bits from cryptographically secure RNG
2. **Storage**: Only SHA-256 hash stored, never plaintext
3. **Transmission**: HTTPS only, token shown once on creation
4. **Validation**: Constant-time hash comparison to prevent timing attacks
5. **Revocation**: Immediate effect, no cache to invalidate
6. **Rate limiting**: Prevents brute force and abuse

## Future Considerations

- **Company scoping**: Add `company_id` FK when company model lands
- **Scopes/permissions**: Could add `scopes` column for fine-grained access
- **Token usage tracking**: Separate `api_token_usage` table for insert-only audit
- **IP allowlisting**: Optional per-token IP restrictions
- **Webhook on suspicious activity**: Alert user on unusual usage patterns

## Acceptance Criteria

- [ ] User can create named API token from web UI
- [ ] Token displayed once on creation, never retrievable again
- [ ] User can list their tokens (metadata only, no secrets)
- [ ] User can revoke tokens, immediate effect
- [ ] User can select expiration (30 days, 90 days, 1 year, never)
- [ ] API requests with `dpl_` prefix validate against token table
- [ ] Expired tokens rejected
- [ ] Revoked tokens rejected
- [ ] Rate limits applied per token, separate from web session limits
- [ ] Token prefix shown in listings for identification
