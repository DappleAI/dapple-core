# Plan 007: Service Tokens with Asymmetric Keys

## Summary

Implement service-to-service authentication using asymmetric keypairs (`dps_` prefix tokens). Each service generates its own keypair - private key stays local, public key is registered with prompt-api. This mirrors the Ethereum-signed token pattern already used for web.

## Motivation

1. **Blast radius isolation** - Compromised service can only affect itself, can't forge tokens for other services
2. **No shared secrets** - Private keys never travel over network or sit in shared k8s secrets
3. **Audit granularity** - Know exactly which service (or script within a service) made each call
4. **Rotation simplicity** - Generate new keypair, register new public key, no coordinated rollout
5. **Consistent pattern** - Same flow as web's Ethereum auth, just with different key types

## Architecture

### Current State (Shared JWT Secret)
```
┌─────────────────────────────────────────────────────────┐
│  DAPPLE_PROMPT_API_JWT_SECRET (shared)                  │
│       ├── prompt-api (signs + validates)                │
│       ├── conduit (validates forwarded JWTs)            │
│       └── flow (validates forwarded JWTs)               │
└─────────────────────────────────────────────────────────┘
Problem: Compromise of any service = compromise of all
```

### Proposed State (Asymmetric Service Tokens)
```
┌─────────────────────────────────────────────────────────┐
│  prompt-api                                             │
│  └── web.service_tokens table                           │
│       ├── dps_conduit_main (public key registered)      │
│       ├── dps_conduit_cleanup (public key registered)   │
│       ├── dps_flow_worker (public key registered)       │
│       └── ...                                           │
└─────────────────────────────────────────────────────────┘

┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│ conduit         │  │ conduit-cleanup │  │ flow            │
│ (private key)   │  │ (private key)   │  │ (private key)   │
└─────────────────┘  └─────────────────┘  └─────────────────┘
```

### Authentication Flow
```
Service (e.g., conduit cleanup script)
    |
    |  1. Build message: version + timestamp + service_id
    |  2. Sign with private key (Ed25519 or ECDSA)
    |  3. Base64 encode: signature + message
    |
    v
POST /api/get-service-token
{
  "token": "base64-signed-message",
  "service_id": "conduit_cleanup"
}
    |
    v
prompt-api
    |
    |  1. Look up service by service_id
    |  2. Verify signature with registered public key
    |  3. Check timestamp (prevent replay)
    |  4. Issue JWT with service identity
    |
    v
{
  "jwt": "...",
  "service_id": "conduit_cleanup",
  "expires_at": "..."
}
```

## Database Schema

```sql
-- Service token registration (public keys only)
CREATE TABLE web.service_tokens (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,

    -- Service identity
    service_id TEXT NOT NULL UNIQUE,  -- e.g., "conduit_main", "conduit_cleanup"
    name TEXT NOT NULL,               -- Human-readable name
    description TEXT,                 -- What this service does

    -- Public key (private key stays with service)
    public_key_pem TEXT NOT NULL,     -- PEM-encoded public key
    key_algorithm TEXT NOT NULL,      -- "Ed25519" or "ES256"

    -- Metadata
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by_user_id BIGINT REFERENCES rev.user_details(user_id),

    -- Status
    revoked_at TIMESTAMPTZ,
    revoked_reason TEXT
);

CREATE INDEX idx_service_tokens_service_id ON web.service_tokens(service_id);

-- Audit log for service token usage
CREATE TABLE web.service_token_usage (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    service_token_id BIGINT NOT NULL REFERENCES web.service_tokens(id),
    used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    source_ip INET,
    user_agent TEXT,
    endpoint_called TEXT
);
```

## Token Format

Similar to Ethereum auth tokens but using Ed25519 or ECDSA:

```
┌─────────────────────────────────────────────────────────┐
│ Signature (64 bytes for Ed25519)                        │
├─────────────────────────────────────────────────────────┤
│ Message (64 bytes)                                      │
│  ├── version (1 byte) = 0x01                           │
│  ├── valid_from (8 bytes, big-endian unix timestamp)   │
│  ├── valid_until (8 bytes, big-endian unix timestamp)  │
│  ├── service_id_len (1 byte)                           │
│  ├── service_id (up to 32 bytes, UTF-8)                │
│  └── padding (remaining bytes)                          │
└─────────────────────────────────────────────────────────┘
Total: 128 bytes, base64 encoded
```

## Implementation Steps

### Phase 1: Database & Models
- [ ] Create migration for `web.service_tokens` table
- [ ] Create migration for `web.service_token_usage` table
- [ ] Add models in `dapple-data/src/model/service_tokens.rs`
- [ ] Add queries in `dapple-data/src/query/service_tokens.rs`

### Phase 2: Service Token Verification
- [ ] Create `src/service/service_token.rs` in prompt-api
  - `verify_service_token(token, service_id)` function
  - Ed25519/ECDSA signature verification
  - Timestamp validation (similar to Ethereum tokens)
- [ ] Add `dps_` prefix detection in auth middleware
- [ ] Create `/api/get-service-token` endpoint (like `/api/get-auth-token`)

### Phase 3: JWT Claims for Services
- [ ] Extend `RawClaims` to include `service_id` for service tokens
- [ ] Add `CallerKind::Service(ServicePrincipal)` variant
- [ ] Service JWTs have `token_use: "service"` and `sub: service_id`

### Phase 4: CLI Tooling
- [ ] Add `dapple service-token generate` command
  - Generates Ed25519 keypair
  - Outputs private key (for service) and public key (for registration)
- [ ] Add `dapple service-token register` command
  - Registers public key with prompt-api
  - Requires admin auth

### Phase 5: Service Integration
- [ ] Update conduit to use service token for internal calls
- [ ] Update flow to use service token
- [ ] Add service token env vars to k8s manifests:
  - `DAPPLE_SERVICE_PRIVATE_KEY` (the private key)
  - `DAPPLE_SERVICE_ID` (which service identity to use)

### Phase 6: Deployment Script Updates
- [ ] Update `deploy_dapple_core.nu` to:
  - Generate keypairs for each service (if not exists)
  - Register public keys with prompt-api
  - Inject private keys as k8s secrets

## Key Algorithm Choice

**Ed25519 (recommended):**
- Faster signing/verification than ECDSA
- Smaller signatures (64 bytes)
- No random nonce needed (deterministic)
- Well-supported in Rust (`ed25519-dalek`)

**ECDSA (P-256/secp256k1):**
- More widely supported in enterprise environments
- Same curve as Ethereum (if we want consistency)
- Slightly larger signatures

Recommend Ed25519 for service tokens since we're not constrained by Ethereum compatibility here.

## Security Considerations

1. **Private key storage** - Service private keys stored as k8s secrets, never in code
2. **Key rotation** - Generate new keypair, register new public key, update k8s secret
3. **Revocation** - Set `revoked_at` on service token, immediately stops working
4. **Replay protection** - Timestamp window validation (same as Ethereum tokens)
5. **Audit logging** - All service token usage logged with IP, endpoint, timestamp

## Migration Path

1. Keep shared JWT secret working (backwards compatible)
2. Add service token support alongside
3. Migrate services one at a time to use service tokens
4. Eventually deprecate shared secret for internal calls

## Open Questions

1. Should service tokens be able to create sub-tokens? (e.g., conduit creates token for cleanup script)
2. Rate limiting per service token?
3. Scopes/permissions per service token? (e.g., conduit can call flow but not vice versa)

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `migrations/YYYYMMDD_service_tokens.sql` | Create | Table definitions |
| `dapple-data/src/model/service_tokens.rs` | Create | Models |
| `dapple-data/src/query/service_tokens.rs` | Create | Queries |
| `dapple-prompt-api/src/service/service_token.rs` | Create | Verification logic |
| `dapple-prompt-api/src/handler/service_token.rs` | Create | Endpoint handler |
| `dapple-prompt-api/src/route/service_token.rs` | Create | Routes |
| `dapple-prompt-api/src/lib.rs` | Modify | Add ServicePrincipal |
| `dapple-prompt-api/src/route.rs` | Modify | Add `dps_` detection |
| `dapple/src/cmd/service_token.rs` | Create | CLI commands |
| `assets/nu/k8s/deploy_*.nu` | Modify | Keypair generation |
| `assets/k8s/registry/*.yaml` | Modify | Service token env vars |

### Documentation Updates

| File | Action | Description |
|------|--------|-------------|
| `docs/journey-planners-guide/authentication.md` | Modify | Add "Service Tokens" section explaining `dps_` tokens, asymmetric auth flow, and when to use |
| `docs/journey-planners-guide/api-patterns.md` | Modify | Add service-to-service auth examples |
| `.env.example` | Modify | Add service token env vars (`DAPPLE_SERVICE_PRIVATE_KEY`, `DAPPLE_SERVICE_ID`) |
| `CLAUDE.md` | Modify | Document service token patterns in "Three auth mechanisms" section |
| `AGENTS.md` (root) | Modify | Add guidance for agents on service token auth patterns and `dps_` prefix handling |
| `crates/dapple-prompt-api/AGENTS.md` | Modify | Document service token verification in API layer |
| `crates/dapple-prompt-api/docs/journey-planners-guide/AGENTS.md` | Modify | Add service token section for API consumers |
| `assets/k8s/AGENTS.md` | Modify | Document service token k8s secrets and keypair management |
| `assets/nu/AGENTS.md` | Modify | Document keypair generation in deployment scripts |
| `README.md` | Modify | Update auth overview if service tokens are user-facing |

## Verification

1. Generate keypair with CLI tool
2. Register public key via API
3. Service signs token, exchanges for JWT
4. Use JWT to call protected endpoint
5. Check audit log shows service identity
6. Revoke token, verify it stops working
