# Issue: Implement Auth Token Verification Endpoint

## Summary

Add `POST /api/get-auth-token` endpoint that verifies Ethereum-signed auth tokens from the web team and issues JWTs for API access.

## Context

The web team authenticates users via wallet signatures. When they need to call our API on behalf of a user, they:
1. Sign an auth token with their private key (proves they are the authorized web service)
2. POST to `/api/get-auth-token` with the token and the user's UUID
3. Receive a JWT to use for subsequent API calls

The auth token is proof of caller identity. The JWT is a session token.

## Implementation Plan

See: `docs/plans/auth-token-verification.md`

## Tasks

- [ ] Phase 1: Core Service (verification logic, config)
- [ ] Phase 2: API Endpoint (schemas, handler, route)
- [ ] Phase 3: Mock/Test Support

## Environment Variables Needed

```bash
DAPPLE_PROMPT_API_JWT_SECRET=<secret-for-jwt-signing>
AUTH_TOKEN_ALLOWED_SIGNERS=0xaddr1,0xaddr2
AUTH_TOKEN_JWT_EXPIRY_SECS=3600  # optional, default 1 hour
```
