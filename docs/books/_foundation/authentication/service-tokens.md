# Service Tokens (Ed25519) — Internal Tier

> **Audience:** Backend Developers · DevOps
>
> **Scope:** This document covers **internal** service tokens — the ones used by dapple-team services (`dapple_ui`, `dapple_conduit`, etc.) authenticating to `api.dapple.x` (the cluster-internal API surface). For customer-owned tokens authenticating to `external-api.dapple.x`, see [external-service-tokens.md](./external-service-tokens.md).

Service tokens provide **asymmetric key authentication** for service-to-service communication. Each service generates its own Ed25519 keypair. The private key stays with the service, and the public key is registered with prompt-api.

## Surface Separation

dapple-prompt runs **three logical API surfaces**, each with its own token table, issuance endpoint, and JWT signing key:

| Surface | Hostname | Token table | Doc |
|---|---|---|---|
| **Internal** | `api.dapple.x` (cluster-internal) | `web.service_tokens` | This document |
| **External** | `external-api.dapple.x` (public) | `rev.external_service_tokens` | [external-service-tokens.md](./external-service-tokens.md) |
| **User** (future) | `user-api.dapple.x` | TBD | — |

A JWT issued by one surface **cannot be verified by another** — each surface loads only its own signing key, so cross-surface token replay fails at signature check before any claim is read. This is *cryptographic* isolation, not just an `aud`-claim convention.

When in doubt about which document applies: an *internal-tier* token is for dapple-owned code calling dapple-owned services. A *customer-tier* token is for a third-party (Acme et al) calling our public external API.

---

<!-- ANCHOR: when-to-use -->
## When to Use

- **Internal services** that need to authenticate with prompt-api (dapple-ui, conduit, flow)
- **Service-to-service communication** where you control both ends
- **Automated backend processes** that run without user context
- **Short-lived operations** where automatic expiration is desired

## When NOT to Use

- **User-owned actions.** Use web session or API tokens instead.
- **Long-running processes.** Use API tokens.
- **External services with Ethereum wallets.** Use Ethereum-signed tokens.
<!-- ANCHOR_END: when-to-use -->

---

<!-- ANCHOR: how-it-works -->
## How It Works

1. Service generates Ed25519 keypair (once, during setup)
2. Admin registers public key in `web.service_tokens` table
3. Service builds a signed token with its private key
4. Service sends token to `POST /api/get-service-token` to exchange for JWT
5. Service uses JWT for subsequent API calls
6. JWT expires (default 1 hour)
<!-- ANCHOR_END: how-it-works -->

---

<!-- ANCHOR: token-structure -->
## Token Structure

The service token is a 128-byte binary structure (base64-encoded):

| Component | Offset | Size | Description |
|-----------|--------|------|-------------|
| Signature | 0 | 64 bytes | Ed25519 signature |
| Version | 64 | 1 byte | Must be `0x01` |
| Valid From | 65 | 8 bytes | Unix timestamp (big-endian) |
| Valid Until | 73 | 8 bytes | Unix timestamp (big-endian) |
| Service ID Length | 81 | 1 byte | Length of service_id (max 32) |
| Service ID | 82 | up to 32 bytes | UTF-8 service identifier |
| Padding | 82+len | remaining | Zeros |

**Total:** 128 bytes (64 signature + 64 message)
<!-- ANCHOR_END: token-structure -->

---

<!-- ANCHOR: generating -->
## Generating a Service Token

**TypeScript with @noble/ed25519:**
```typescript
import * as ed25519 from "@noble/ed25519";
import { sha512 } from "@noble/hashes/sha512";

ed25519.etc.sha512Sync = (...m) => sha512(ed25519.etc.concatBytes(...m));

async function signServiceToken(privateKeyHex: string, serviceId: string): Promise<string> {
  const keyBytes = hexToBytes(privateKeyHex);
  const now = Math.floor(Date.now() / 1000);
  const validFrom = now;
  const validUntil = now + 300; // 5 minutes max

  // Build 64-byte message
  const message = new Uint8Array(64);
  message[0] = 0x01; // version
  writeU64BE(message, 1, validFrom);
  writeU64BE(message, 9, validUntil);
  message[17] = serviceId.length;
  new TextEncoder().encode(serviceId).forEach((b, i) => message[18 + i] = b);

  const signature = await ed25519.signAsync(message, keyBytes);

  const token = new Uint8Array(128);
  token.set(signature, 0);
  token.set(message, 64);

  return Buffer.from(token).toString("base64");
}
```

**Rust with ed25519-dalek:**
```rust
use ed25519_dalek::{SigningKey, Signer};
use base64::{Engine, engine::general_purpose::STANDARD};

fn sign_service_token(private_key: &[u8; 32], service_id: &str) -> String {
    let signing_key = SigningKey::from_bytes(private_key);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut message = [0u8; 64];
    message[0] = 0x01;
    message[1..9].copy_from_slice(&now.to_be_bytes());
    message[9..17].copy_from_slice(&(now + 300).to_be_bytes());
    message[17] = service_id.len() as u8;
    message[18..18 + service_id.len()].copy_from_slice(service_id.as_bytes());

    let signature = signing_key.sign(&message);

    let mut token = [0u8; 128];
    token[..64].copy_from_slice(&signature.to_bytes());
    token[64..].copy_from_slice(&message);

    STANDARD.encode(token)
}
```
<!-- ANCHOR_END: generating -->

---

<!-- ANCHOR: exchanging -->
## Exchanging for JWT

**Request:**
```json
POST /api/get-service-token
Content-Type: application/json

{
  "token": "base64-encoded-128-byte-signed-token",
  "service_id": "dapple_ui"
}
```

**Successful Response:**
```json
{
  "status": "ok",
  "jwt": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "service_id": "dapple_ui",
  "expires_at": "2025-01-25T12:30:00+00:00"
}
```

**JWT Claims:**
```json
{
  "sub": "dapple_ui",
  "service_id": "dapple_ui",
  "token_use": "service",
  "roles": ["service"],
  "exp": 1705312200
}
```
<!-- ANCHOR_END: exchanging -->

---

<!-- ANCHOR: delegated -->
## Delegated Tokens (Acting on Behalf of Users)

When an internal service needs to make API calls **on behalf of a logged-in user**, include the `auth_user_id` parameter in the exchange request. This creates a **delegated token** instead of a service token.

> **Agent Note:** Pass `auth_user_id` when making requests on behalf of a user to ensure proper user registration and access control. The delegated JWT carries the user's identity, enabling per-user rate limiting, audit trails, and permission checks.

### When to Use Delegated Tokens

- **User is logged into dapple-ui** and making API calls through the frontend
- **Service needs to act as a specific user** for access control
- **User auto-registration** must be triggered (new users are registered on first delegated request)
- **Audit trails** need to attribute actions to specific users

### When to Use Service Tokens (No User)

- **Background jobs** running without user context
- **System maintenance** operations
- **Health checks** and internal monitoring

### Request Schema

```json
POST /api/get-service-token
Content-Type: application/json

{
  "token": "base64-encoded-128-byte-signed-token",
  "service_id": "dapple_ui",
  "auth_user_id": 123
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `token` | string | Yes | Base64-encoded 128-byte signed token |
| `service_id` | string | Yes | Registered service identifier |
| `auth_user_id` | integer | No | Auth user ID to delegate to (from NextAuth session) |

### Delegated JWT Claims

When `auth_user_id` is provided, the JWT claims change:

```json
{
  "sub": { "type": "auth_user", "id": 123 },
  "service_id": "dapple_ui",
  "token_use": "delegated",
  "roles": ["user"],
  "exp": 1705312200
}
```

**Key differences from service JWT:**
- `sub` becomes an object `{ "type": "auth_user", "id": N }` instead of the service_id string
- `token_use` is `"delegated"` instead of `"service"`
- `roles` includes `["user"]` instead of `["service"]`
<!-- ANCHOR_END: delegated -->

---

<!-- ANCHOR: token-expiry -->
## Token Expiry & Refresh

Understanding token lifetimes is critical for building reliable integrations.

### Lifetime Summary

| Component | Lifetime | Configurable |
|-----------|----------|--------------|
| Ed25519 signed token | Max 5 minutes | No (enforced by API) |
| JWT (service or delegated) | 1 hour default | `AUTH_TOKEN_JWT_EXPIRY_SECS` |

> **Agent Note:** If you receive `401 Unauthorized` after previously successful requests, the JWT has likely expired. Request a new one via `POST /api/get-service-token`. Implement token refresh before expiry (e.g., refresh at 50 minutes) to avoid request failures.

### Refresh Pattern

Services should cache the JWT and refresh it **before** expiry:

```typescript
class ServiceTokenManager {
  private jwt: string | null = null;
  private expiresAt: Date | null = null;

  async getToken(): Promise<string> {
    // Refresh if expired or expiring within 10 minutes
    const refreshThreshold = 10 * 60 * 1000; // 10 minutes in ms
    const shouldRefresh = !this.jwt ||
      !this.expiresAt ||
      Date.now() > this.expiresAt.getTime() - refreshThreshold;

    if (shouldRefresh) {
      const response = await this.exchangeToken();
      this.jwt = response.jwt;
      this.expiresAt = new Date(response.expires_at);
    }

    return this.jwt!;
  }

  private async exchangeToken() {
    const signedToken = await signServiceToken(this.privateKey, this.serviceId);
    const response = await fetch('/api/get-service-token', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        token: signedToken,
        service_id: this.serviceId,
        // Include auth_user_id for delegated tokens
        ...(this.authUserId && { auth_user_id: this.authUserId })
      })
    });
    return response.json();
  }
}
```

### Troubleshooting: Unexpected 401 Errors

When a service starts receiving 401 errors after previously working:

1. **Most likely cause:** JWT expired (1 hour lifetime)
2. **Fix:** Call `POST /api/get-service-token` again to get a fresh JWT
3. **Prevention:** Implement token refresh before expiry (e.g., refresh at 50 minutes)

**Other causes:**
- Service token revoked (check `web.service_tokens.revoked_at`)
- Clock drift between service and API server (check `NOT_YET_VALID` or `EXPIRED` errors)
- Wrong service_id in request
<!-- ANCHOR_END: token-expiry -->

---

<!-- ANCHOR: error-codes -->
## Error Codes

| Error Code | HTTP Status | Description | Fix |
|------------|-------------|-------------|-----|
| `MALFORMED_TOKEN` | 400 | Token structure invalid | Check token generation logic |
| `INVALID_VERSION` | 400 | Version byte not `0x01` | Use version `0x01` |
| `DURATION_EXCEEDS_MAX` | 400 | Token validity > 5 minutes | Reduce validity to ≤ 300s |
| `SERVICE_ID_MISMATCH` | 400 | Token service_id doesn't match request | Ensure both match |
| `SERVICE_NOT_FOUND` | 404 | Service not registered | Register public key in database |
| `INVALID_SIGNATURE` | 401 | Signature verification failed | Check signing logic |
| `NOT_YET_VALID` | 401 | Current time before `valid_from` | Check clock synchronization |
| `EXPIRED` | 401 | Current time after `valid_until` | Generate new token |
| `SERVICE_REVOKED` | 403 | Service token has been revoked | Contact admin |
<!-- ANCHOR_END: error-codes -->

---

<!-- ANCHOR: registration -->
## Registering a Service

**1. Generate an Ed25519 keypair:**
```bash
openssl genpkey -algorithm Ed25519 -out /tmp/service.pem
PRIVATE_KEY=$(openssl pkey -in /tmp/service.pem -outform DER | tail -c 32 | xxd -p -c 32)
echo "Private key (hex): $PRIVATE_KEY"
openssl pkey -in /tmp/service.pem -pubout
```

**2. Register in database:**
```sql
INSERT INTO web.service_tokens (service_id, name, public_key_pem, key_algorithm)
VALUES (
    'dapple_ui',
    'Dapple UI Web Application',
    '-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEA<your-base64-public-key>
-----END PUBLIC KEY-----',
    'Ed25519'
);
```

**3. Configure service environment:**
```bash
DAPPLE_PROMPT_API_INTERNAL_URL=http://dapple-prompt-api
DAPPLE_UI_SAT_SIGNING_KEY=<64-char-hex-private-key>
DAPPLE_UI_SERVICE_ID=dapple_ui
```

**Service ID Format:** 3-32 characters, start with lowercase letter, only lowercase letters, digits, and underscores.
- **Valid:** `dapple_ui`, `conduit_main`, `flow_worker_1`
- **Invalid:** `DappleUI`, `1service`, `my-service`
<!-- ANCHOR_END: registration -->

---

<!-- ANCHOR: security -->
## Security Considerations

- **Private key isolation.** Each service has its own keypair.
- **No shared secrets.** Private keys never travel over network.
- **Short-lived tokens.** Maximum 5 minutes validity.
- **Audit granularity.** Each service has unique identity in logs.
- **Easy rotation.** Generate new keypair, register new public key.
- **Instant revocation.** Set `revoked_at` on service token record.
<!-- ANCHOR_END: security -->
