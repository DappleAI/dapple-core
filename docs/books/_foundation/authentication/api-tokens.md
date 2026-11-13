# Opaque Personal Access Tokens (PAT)

> **Audience:** Developers · Agents · CLI Users

Opaque PATs are **user-scoped, hash-stored tokens** for programmatic API access. Unlike JWTs (which are self-contained and can be validated without database access), opaque tokens are random strings that require a database lookup to validate.

**Why "opaque"?** The token itself contains no readable information. It is just a random string. All metadata (user, expiration, revocation status) is stored server-side and looked up by hashing the token.

---

<!-- ANCHOR: when-to-use -->
## When to Use

- **CLI tools** that need persistent authentication without re-login
- **Mobile applications** that need long-lived access
- **Automation scripts** that run periodically or in CI/CD pipelines
- **Development workflows** where you're testing API endpoints directly
- **Third-party integrations** where you control the integration code
<!-- ANCHOR_END: when-to-use -->

---

<!-- ANCHOR: how-it-works -->
## How It Works

1. User creates an API token via `POST /api/api-tokens/`
2. Token is returned once (store it securely - it won't be shown again)
3. Include token in `Authorization: Bearer <token>` header for API calls
4. Token remains valid until expiration or manual revocation
<!-- ANCHOR_END: how-it-works -->

---

<!-- ANCHOR: creating -->
## Creating an API Token

**Request:**
```json
POST /api/api-tokens/
Authorization: Bearer <your-web-session-jwt>
Content-Type: application/json

{
  "name": "CLI Integration",
  "expires_in_secs": 7776000
}
```

**Response (201 Created):**
```json
{
  "status": "success",
  "object_id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "CLI Integration",
  "token": "dpl_7Kj2mNpQrStUvWxYz3AbCdEfGhIjKlMn",
  "token_prefix": "dpl_7Kj2",
  "expires_at": "2025-04-22T00:00:00Z",
  "created_at": "2025-01-22T00:00:00Z"
}
```

**Important:** The `token` field is only returned once during creation. Store it securely. If lost, create a new token and revoke the old one.

**Common expiration values:**
- `2592000`: 30 days
- `7776000`: 90 days
- `31536000`: 1 year
- Omit `expires_in_secs` for tokens that never expire
<!-- ANCHOR_END: creating -->

---

<!-- ANCHOR: managing -->
## Managing API Tokens

**List your tokens:**
```bash
GET /api/api-tokens/
Authorization: Bearer <your-web-session-jwt>
```

**Response:**
```json
{
  "tokens": [
    {
      "object_id": "550e8400-e29b-41d4-a716-446655440000",
      "name": "CLI Integration",
      "token_prefix": "dpl_7Kj2",
      "expires_at": "2025-04-22T00:00:00Z",
      "revoked_at": null,
      "created_at": "2025-01-22T00:00:00Z"
    }
  ]
}
```

**Revoke a token:**
```bash
DELETE /api/api-tokens/:object_id
Authorization: Bearer <your-web-session-jwt>
Content-Type: application/json

{
  "reason": "No longer needed"
}
```

**Response (200 OK):**
```json
{
  "status": "success",
  "message": "API token has been revoked"
}
```

Revocation is append-only. The token immediately stops working. If already revoked, returns `409 Conflict` with `ALREADY_REVOKED` error.
<!-- ANCHOR_END: managing -->

---

<!-- ANCHOR: using -->
## Using an API Token

Include the token in the Authorization header:

```bash
curl -H "Authorization: Bearer dpl_abc123..." \
     -H "Content-Type: application/json" \
     https://api.example.com/api/tasks/
```
<!-- ANCHOR_END: using -->

---

<!-- ANCHOR: format -->
## Token Format

API tokens use the prefix `dpl_` followed by a base62-encoded random payload:

```
dpl_<32 random bytes as base62>

Example: dpl_7Kj2mNpQrStUvWxYz3AbCdEfGhIjKlMn
         ├──┘└─────────────────────────────────┘
         │              │
      prefix      random payload (43 chars)
      (4 chars)

Total length: 47 characters
Entropy: 256 bits (cryptographically secure)
```

**Why this format:**
- `dpl_` prefix: Easy identification, prevents accidental use as JWT
- Base62 encoding: URL-safe, no special characters
- 256 bits entropy: Cryptographically secure random generation

**Storage:** Tokens are stored as SHA-256 hash in `web.api_tokens.token_hash`. The `token_prefix` column stores the first 8 chars for display. Validation uses constant-time hash comparison.
<!-- ANCHOR_END: format -->

---

<!-- ANCHOR: security -->
## Security Considerations

- **Treat like passwords.** Store in secure credential managers.
- **Use environment variables.** Never hardcode in source code.
- **Rotate regularly.** Create new tokens and revoke old ones periodically.
- **Scope to minimum necessary permissions.** Use different tokens for different purposes.
- **Set appropriate expiration.** Maximum 1 year, but shorter is better.
- **Revoke immediately if compromised.** Use the DELETE endpoint.
- **Rate limit:** 60 requests/minute (vs 300 for web sessions)
<!-- ANCHOR_END: security -->
