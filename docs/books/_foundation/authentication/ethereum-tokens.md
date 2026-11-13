# Ethereum-Signed Auth Tokens

> **Audience:** External Service Developers

Ethereum-signed tokens enable **trusted external services** with Ethereum wallet infrastructure to authenticate and act on behalf of users.

---

<!-- ANCHOR: when-to-use -->
## When to Use

- **Trusted external services** that need to act on behalf of users
- **Delegated authentication** where a backend service is authorized to make API calls
- **Short-lived integrations** where you want automatic expiration
- **Services with Ethereum wallet infrastructure** already in place

## When NOT to Use

- **User-owned actions.** Use web session or API tokens instead.
- **Long-running processes.** Use API tokens.
- **Internal services.** Use Service Tokens (Ed25519) instead.
<!-- ANCHOR_END: when-to-use -->

---

<!-- ANCHOR: how-it-works -->
## How It Works

1. External service generates an auth token with Ethereum signature
2. Service sends token to `POST /api/get-auth-token` to exchange for JWT
3. Service uses JWT for subsequent API calls
4. JWT expires (default 1 hour, token validity max 30 minutes)
<!-- ANCHOR_END: how-it-works -->

---

<!-- ANCHOR: token-structure -->
## Token Structure

The auth token is a 129-byte binary structure (base64-encoded):

| Component | Size | Description |
|-----------|------|-------------|
| Signature (r) | 32 bytes | ECDSA signature r value |
| Signature (s) | 32 bytes | ECDSA signature s value |
| Signature (v) | 1 byte | Recovery ID |
| Version | 1 byte | Must be `0x01` |
| Valid From | 8 bytes | Unix timestamp (big-endian) |
| Valid Until | 8 bytes | Unix timestamp (big-endian) |
| Padding | 47 bytes | Reserved for future use |

**Total:** 129 bytes
<!-- ANCHOR_END: token-structure -->

---

<!-- ANCHOR: generating -->
## Generating an Auth Token

**Python with web3.py:**
```python
from web3 import Web3
from eth_account.messages import encode_defunct
import struct
import time
import base64

def generate_auth_token(private_key: str, valid_for_minutes: int = 5):
    """Generate an Ethereum-signed auth token."""
    w3 = Web3()
    account = w3.eth.account.from_key(private_key)

    version = 0x01
    valid_from = int(time.time())
    valid_until = valid_from + (valid_for_minutes * 60)

    # Version (1) + valid_from (8) + valid_until (8) + padding (47)
    message = struct.pack('>B', version)
    message += struct.pack('>Q', valid_from)
    message += struct.pack('>Q', valid_until)
    message += b'\x00' * 47

    assert len(message) == 64

    # Sign using EIP-191 personal_sign
    signable_message = encode_defunct(message)
    signed = w3.eth.account.sign_message(signable_message, private_key=private_key)

    # Combine signature (65 bytes) + message (64 bytes)
    r = signed.r.to_bytes(32, 'big')
    s = signed.s.to_bytes(32, 'big')
    v = bytes([signed.v])

    token = r + s + v + message
    assert len(token) == 129

    return base64.b64encode(token).decode()
```
<!-- ANCHOR_END: generating -->

---

<!-- ANCHOR: exchanging -->
## Exchanging for JWT

**Request:**
```json
POST /api/get-auth-token
Content-Type: application/json

{
  "token": "base64-encoded-129-byte-signed-token",
  "user_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

**Successful Response:**
```json
{
  "status": "ok",
  "jwt": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "signer": "0x742d35Cc6634C0532925a3b844Bc9e7595f4E3B0",
  "expires_at": "2024-01-15T11:30:00+00:00"
}
```

**JWT Claims:**
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
<!-- ANCHOR_END: exchanging -->

---

<!-- ANCHOR: error-codes -->
## Error Codes

| Error Code | HTTP Status | Description | Fix |
|------------|-------------|-------------|-----|
| `MALFORMED_TOKEN` | 400 | Token structure invalid | Check token generation logic |
| `INVALID_VERSION` | 400 | Version byte not `0x01` | Use version `0x01` |
| `DURATION_EXCEEDS_MAX` | 400 | Token validity > 30 minutes | Reduce validity |
| `INVALID_SIGNATURE` | 401 | Signature verification failed | Check signing logic |
| `NOT_YET_VALID` | 401 | Current time before `valid_from` | Check clock synchronization |
| `EXPIRED` | 401 | Current time after `valid_until` | Generate new token |
| `SIGNER_NOT_AUTHORIZED` | 403 | Signer not in allowlist | Contact admin |
| `SERVICE_UNAVAILABLE` | 503 | Missing server configuration | Contact admin |
<!-- ANCHOR_END: error-codes -->

---

<!-- ANCHOR: configuration -->
## Configuration

The API requires these environment variables:

```bash
# Required: Secret for JWT signing
DAPPLE_PROMPT_API_SIGNING_KEY=your-jwt-secret

# Required: Comma-separated list of authorized Ethereum addresses
AUTH_TOKEN_ALLOWED_SIGNERS=0x742d35Cc6634C0532925a3b844Bc9e7595f4E3B0,0x...

# Optional: Custom JWT expiration in seconds (default: 3600 = 1 hour)
AUTH_TOKEN_JWT_EXPIRY_SECS=3600
```

**Note:** `DAPPLE_PROMPT_API_SIGNING_KEY` must be set for the API to start.
<!-- ANCHOR_END: configuration -->

---

<!-- ANCHOR: security -->
## Security Considerations

- **Allowlist required.** Signer address must be in `AUTH_TOKEN_ALLOWED_SIGNERS`.
- **Short-lived tokens.** Maximum 30 minutes validity.
- **Clock synchronization.** Ensure server and client clocks are in sync.
- **Secure key storage.** Private key must be protected (HSM, secrets manager).
- **Audit logging.** All token exchanges are logged.
- **Rotate allowlist.** Regularly review and update authorized signers.
<!-- ANCHOR_END: security -->
