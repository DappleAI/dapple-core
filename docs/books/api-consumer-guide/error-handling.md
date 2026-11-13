# Error Handling

> **Audience:** API Consumers · Integration Developers

This guide covers error responses, troubleshooting, and best practices for handling API errors.

## Error Response Format

### Standard Error Response

```json
{
  "status": "fail",
  "message": "Detailed error description"
}
```

### Auth Token Error Response

The `/api/get-auth-token` endpoint uses a different format:

```json
{
  "error": "ERROR_CODE",
  "message": "Detailed explanation"
}
```

## Error Types and Resolution

### Validation Errors (400)

**Response:**
```json
{
  "status": "fail",
  "message": "name: field required"
}
```

**Causes:**
- Missing required fields
- Invalid field types
- Constraint violations

**Resolution:**
1. Check request schema against OpenAPI spec
2. Ensure all required fields are present
3. Verify field types match expected types
4. Check string length and format constraints

### Unauthorized (401)

**Response:**
```json
{
  "status": "fail",
  "message": "Missing or invalid authorization token"
}
```

**Causes:**
- Missing `Authorization` header
- Expired JWT token
- Invalid token format
- Revoked API token

**Resolution:**
1. Include `Authorization: Bearer <token>` header
2. Check token expiration and refresh if needed
3. Verify token format (JWT or `dpl_` prefix)
4. Generate new token if revoked

### Forbidden (403)

**Response:**
```json
{
  "error": "SIGNER_NOT_AUTHORIZED",
  "message": "SIGNER_NOT_AUTHORIZED: address 0x... is not in the allowlist"
}
```

**Causes:**
- Ethereum signer not in allowlist
- Insufficient permissions for action
- Token type doesn't permit operation

**Resolution:**
1. Contact admin to add signer to authorized list
2. Use appropriate token type for operation
3. Request elevated permissions if needed

### Not Found (404)

**Response:**
```json
{
  "status": "fail",
  "message": "Resource not found"
}
```

**Causes:**
- Resource doesn't exist
- Incorrect `id`
- Referenced resource was deleted
- User doesn't have access to resource

**Resolution:**
1. Verify resource exists before referencing
2. Check `id` is correct (UUID format)
3. Ensure dependencies are created first
4. Verify access permissions

### Conflict (409)

**Response:**
```json
{
  "status": "fail",
  "message": "duplicate key value violates unique constraint"
}
```

**Causes:**
- Duplicate name in unique-constrained field
- Duplicate associative relationship
- Batch contains duplicate entries

**Resolution:**
1. Use unique names for resources
2. Check existing resources before creating
3. Remove duplicates from batch requests

### Internal Server Error (500)

**Response:**
```json
{
  "status": "fail",
  "message": "Internal server error"
}
```

**Causes:**
- Unexpected server error
- Database connection issue
- Configuration problem

**Resolution:**
1. Retry the request after a short delay
2. Check server logs if available
3. Report issue to development team
4. Include request details in bug report

### Service Unavailable (503)

**Response:**
```json
{
  "status": "fail",
  "message": "Service temporarily unavailable"
}
```

**Causes:**
- Missing server configuration
- Dependent service unavailable
- Server maintenance

**Resolution:**
1. Wait and retry with exponential backoff
2. Check service status page
3. Contact operations team

## Auth Token Specific Errors

### EXPIRED

```json
{
  "error": "EXPIRED",
  "message": "EXPIRED: token is only valid between 1705312000 and 1705312300"
}
```

**Resolution:** Generate new token with current timestamp

### NOT_YET_VALID

```json
{
  "error": "NOT_YET_VALID",
  "message": "NOT_YET_VALID: token valid_from 1705312000 is in the future"
}
```

**Resolution:** Check system clock synchronization, reduce `valid_from` buffer

### INVALID_SIGNATURE

```json
{
  "error": "INVALID_SIGNATURE",
  "message": "INVALID_SIGNATURE: signature verification failed"
}
```

**Resolution:**
1. Verify signing key matches registered signer
2. Check signature algorithm (EIP-191 personal_sign)
3. Ensure message format matches specification

### SIGNER_NOT_AUTHORIZED

```json
{
  "error": "SIGNER_NOT_AUTHORIZED",
  "message": "SIGNER_NOT_AUTHORIZED: address 0x... is not in the allowlist"
}
```

**Resolution:** Contact admin to add signer to `AUTH_TOKEN_ALLOWED_SIGNERS`

## Guide Ingest Errors

Guide ingest has its own error categories. For full details, see [Guide Ingest API - Error Responses](./ingest-api.md#error-responses).

### Tarball Errors (400 Bad Request)

Returned when the uploaded tarball is invalid:

```json
{
  "status": "fail",
  "message": "Tarball exceeds maximum size of 50MB"
}
```

**Causes:**
- No file field in multipart request
- Tarball exceeds 50 MB limit
- Symlinks detected inside tarball (use `tar czh` to follow symlinks at creation time)
- Path traversal (`..`) detected in tarball entries
- No `guide.toml` found in tarball root

### Parse Errors (422 Unprocessable Entity)

Returned when the guide content is invalid:

```json
{
  "status": "fail",
  "message": "guide.toml: missing required field 'title'"
}
```

**Causes:**
- Invalid TOML syntax in `guide.toml`
- Missing required `title` field
- Invalid semver in `version` field
- Invalid deprecation level (must be `deprecated`, `retired`, or `quarantined`)
- Invalid SUMMARY.md link-list format
- Malformed `:::` directive blocks
- Forward-referencing `parent_index` in chapters

### Materialization Errors (500 Internal Server Error)

Returned when database operations fail during ingest:

**Causes:**
- Asset upload pipeline failure (draft/chunk error)
- Duplicate title/slug constraint violation
- Database connection issue during atomic transaction

**Resolution:**
1. For duplicate title errors: choose a different guide title
2. For asset errors: verify file contents are not corrupted
3. For transient database errors: retry the ingest

---

## Error Handling Best Practices

### Retry Strategy

Implement exponential backoff for retryable errors:

```javascript
async function fetchWithRetry(url, options, maxRetries = 3) {
  for (let i = 0; i < maxRetries; i++) {
    try {
      const response = await fetch(url, options);

      if (response.status === 500 || response.status === 503) {
        // Retryable errors
        const delay = Math.pow(2, i) * 1000; // 1s, 2s, 4s
        await sleep(delay);
        continue;
      }

      return response;
    } catch (error) {
      if (i === maxRetries - 1) throw error;
    }
  }
}
```

### Token Refresh

Handle token expiration gracefully:

```javascript
async function apiRequest(url, options) {
  let response = await fetch(url, options);

  if (response.status === 401) {
    // Token expired, refresh and retry
    const newToken = await refreshToken();
    options.headers.Authorization = `Bearer ${newToken}`;
    response = await fetch(url, options);
  }

  return response;
}
```

### Validation Before Request

Prevent client-side errors:

```javascript
function validateCreateRequest(data) {
  const errors = [];

  if (!data.name || data.name.length < 2) {
    errors.push("name: must be at least 2 characters");
  }

  if (data.ids && !Array.isArray(data.ids)) {
    errors.push("ids: must be an array");
  }

  return errors;
}
```

### Error Logging

Log errors with context for debugging:

```javascript
function handleApiError(error, context) {
  console.error({
    timestamp: new Date().toISOString(),
    endpoint: context.endpoint,
    method: context.method,
    requestId: context.requestId,
    status: error.status,
    message: error.message,
    requestBody: context.body
  });
}
```

## Debugging Tips

### Check Request Format

1. Verify JSON syntax is valid
2. Ensure arrays are wrapped in `{ "objs": [...] }`
3. Check UUID format (lowercase with hyphens)

### Verify Dependencies

1. Create parent resources before children
2. Ensure referenced `id` values exist
3. Check associative table requirements

### Inspect Response Details

1. Read the full error message
2. Check for field-specific validation errors
3. Look for constraint violation details

### Use OpenAPI Spec

1. Reference `/api.json` for exact schemas
2. Check required vs optional fields
3. Verify enum values and constraints
