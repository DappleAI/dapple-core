# Error Handling

> **Audience:** Customer Integration Engineers · Backend Developers

This chapter covers the error responses you will see on the external surface and how to handle them. The most important thing to internalize is that **the surface is deliberately tight-lipped**: it will not tell you whether a resource you cannot see exists.

## Error Response Format

```json
{ "status": "fail", "message": "Detailed error description" }
```

## Status Codes at a Glance

| Code | Meaning | Typical cause |
|------|---------|---------------|
| 400 | Bad Request | Validation error; `GUIDE_PLAYBOOK_MISMATCH` |
| 401 | Unauthorized | Missing/invalid/expired JWT; revoked token or company |
| 403 | Forbidden | Valid JWT presented to a disallowed operation |
| 404 | Not Found | `RESOURCE_NOT_FOUND` — also returned for foreign ids (no leak) |
| 500 | Internal Server Error | Unexpected server error |
| 503 | Service Unavailable | The external signing key is not configured |

## 401 Unauthorized

```json
{ "status": "fail", "message": "Missing or invalid authorization token" }
```

**Causes:**
- No `Authorization: Bearer <jwt>` header.
- The JWT expired (default lifetime is 1 hour).
- The JWT was minted on the **internal** surface — its signature fails verification here.
- Your token or company was revoked (takes effect on the next token exchange; existing JWTs work until they expire).

**Resolution:**
1. Include the `Authorization: Bearer <jwt>` header.
2. Re-run `POST /api/get-service-token` to mint a fresh JWT before expiry — aim for the 50-minute mark.
3. Confirm you are using a JWT from `external-api.dapple.x`, not one from the internal surface. The two are cryptographically isolated; a token from one cannot be used on the other.
4. If exchange itself returns `401`, your signed token, public key registration, or company status is the problem — contact your dapple admin.

## 403 Forbidden

```json
{ "status": "fail", "message": "Operation not permitted for this caller" }
```

A valid JWT presented to an operation the external surface does not allow. The external surface is intentionally narrow — there is no admin or cross-company action behind it. If you expected an endpoint that is not in the [Endpoint Reference](./endpoint-reference.md) V1 table, it does not exist on this surface.

## 404 Not Found — The No-Leak Rule

```json
{ "status": "fail", "message": "RESOURCE_NOT_FOUND" }
```

This is the response you get for a resource that does not exist **and** for a resource that exists but belongs to another company. **They are deliberately indistinguishable.**

> **Why:** if a foreign id returned `403` while an unknown id returned `404`, you could probe for the existence of other customers' guides, journeys, and dispatches. Returning the same `404 RESOURCE_NOT_FOUND` for both closes that side channel. This is a feature, not a bug.

**What this means for you:**
- Only reference ids your own company created. There is no "discover other companies' resources" path.
- A `404` on something you are sure you created usually means a typo'd id or you are using a different company's JWT than you think.

This rule applies uniformly: foreign or unknown `journey_id`, `dispatch_id`, `guide_id`, and PATCH `path_id` all return the same `404`.

## 400 GUIDE_PLAYBOOK_MISMATCH

```json
{ "status": "fail", "message": "GUIDE_PLAYBOOK_MISMATCH" }
```

You passed a `guide_id` and `playbook_id` that are inconsistent — the guide version does not expose that playbook version. The backend validates the pair; it never resolves "latest" for you.

**Resolution:** pass the exact pair you got from guide browse. A matching-but-non-canonical pair (an older version, an experiment) is allowed — only an inconsistent pair is rejected. A guide outside your namespace returns `404` rather than this `400`.

## 503 Service Unavailable

```json
{ "status": "fail", "message": "Service temporarily unavailable" }
```

On the external surface this most commonly means the surface's **JWT signing key is not configured** server-side, so token exchange cannot mint JWTs. This is an operational condition on the dapple side, not something your client can fix.

**Resolution:** retry with backoff; if it persists, contact your dapple operations contact.

## 500 Internal Server Error

```json
{ "status": "fail", "message": "Internal server error" }
```

An unexpected server-side failure. Retry with exponential backoff; if it persists, report it with the request details (endpoint, method, and request body) to your dapple contact.

## Handling Patterns

### Retry with Backoff

Retry only the retryable codes (`500`, `503`):

```javascript
async function fetchWithRetry(url, options, maxRetries = 3) {
  for (let i = 0; i < maxRetries; i++) {
    const response = await fetch(url, options);
    if (response.status === 500 || response.status === 503) {
      await sleep(Math.pow(2, i) * 1000); // 1s, 2s, 4s
      continue;
    }
    return response;
  }
}
```

### Refresh the JWT on 401

```javascript
async function apiRequest(url, options) {
  let response = await fetch(url, options);
  if (response.status === 401) {
    const newJwt = await exchangeServiceToken(); // POST /api/get-service-token
    options.headers.Authorization = `Bearer ${newJwt}`;
    response = await fetch(url, options);
  }
  return response;
}
```

Refresh proactively before expiry (around 50 minutes) rather than reacting to every `401`.

### Don't Probe on 404

Because foreign ids and unknown ids both return `404`, retrying or fanning out across ids tells you nothing and looks like enumeration. Treat `404` as terminal: fix the id, or use the correct company's JWT.

## Guide Ingest Errors

Ingest has its own failure modes — see [Guide Ingest](./guide-ingest.md). The common ones:

- **400** — missing `file` multipart field, tarball over 50 MB, symlink or path traversal in the tarball, or no `guide.toml` at the root.
- **404** — PATCH against a `path_id` that is foreign or unknown (no leak).
- **422** — invalid guide content (bad TOML, missing `title`, malformed directives).
- **500** — a materialization/database failure during the atomic ingest transaction (retry).
