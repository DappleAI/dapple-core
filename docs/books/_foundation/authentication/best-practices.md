# Authentication Best Practices

> **Audience:** All Developers

Security guidelines and best practices for authentication in dapple-prompt.

---

<!-- ANCHOR: by-use-case -->
## Best Practices by Use Case

### For Web Applications

- Use NextAuth.js session tokens for all user-facing operations
- Never expose session tokens outside the browser
- Implement proper logout flows to invalidate sessions

### For CLI Tools and Scripts

- Use opaque PATs for persistent authentication
- Store tokens in secure credential managers (e.g., `pass`, `keychain`, `1Password CLI`)
- Use environment variables, never hardcode
- Set expiration dates appropriate to use case (shorter is better)
- Implement token refresh workflows for long-running processes

### For Internal Services

- Use Ed25519 service tokens for service-to-service authentication
- Keep tokens short-lived (5 minutes maximum)
- Implement automatic token refresh before expiration
- Store private keys as Kubernetes secrets or in secure secrets management
- Each service should have its own unique keypair
- Rotate keys periodically by registering new public key, then updating service config

### For External Services

- Use Ethereum-signed tokens only when integrating with services that have existing Ethereum wallet infrastructure
- Keep tokens short-lived (5-15 minutes recommended)
- Implement automatic token refresh before expiration
- Store private keys in HSMs or secure secrets management systems
- Audit all token exchanges
- Maintain strict allowlist of authorized signers
<!-- ANCHOR_END: by-use-case -->

---

<!-- ANCHOR: security-checklist -->
## Security Checklist

- [ ] Never commit tokens or private keys to version control
- [ ] Use environment variables for all credentials
- [ ] Set appropriate expiration times
- [ ] Implement token rotation policies
- [ ] Revoke compromised tokens immediately
- [ ] Monitor authentication logs for anomalies
- [ ] Use HTTPS for all API communications
- [ ] Implement rate limiting on authentication endpoints
<!-- ANCHOR_END: security-checklist -->

---

<!-- ANCHOR: common-issues -->
## Common Issues

### "401 Unauthorized"

- **Cause:** Missing or invalid JWT
- **Fix:** Ensure `Authorization: Bearer <token>` header is present and token is valid

### "403 Forbidden - Signer not authorized"

- **Cause:** Ethereum address not in allowlist
- **Fix:** Contact admin to add signer to `AUTH_TOKEN_ALLOWED_SIGNERS`

### "Token expired"

- **Cause:** JWT or auth token has passed expiration time
- **Fix:** Generate new token or refresh existing JWT

### "Invalid signature"

- **Cause:** Signature verification failed
- **Fix:** Check signing logic, ensure correct private key is used

### "Clock skew errors"

- **Cause:** Client and server clocks out of sync
- **Fix:** Synchronize clocks (use NTP), allow for small time drift tolerance
<!-- ANCHOR_END: common-issues -->

---

<!-- ANCHOR: token-storage -->
## Token Storage Recommendations

| Token Type | Recommended Storage |
|------------|---------------------|
| Web Session JWT | Browser cookies (automatic via NextAuth) |
| Opaque PAT | Credential manager (`pass`, `keychain`, `1Password`) |
| Service Token Private Key | Kubernetes secrets, HashiCorp Vault |
| Ethereum Private Key | HSM, hardware wallet, secure secrets manager |

### Environment Variables

Always use environment variables for tokens in applications:

```bash
# Good
export DAPPLE_API_TOKEN="dpl_abc123..."
curl -H "Authorization: Bearer $DAPPLE_API_TOKEN" ...

# Bad - hardcoded in script
curl -H "Authorization: Bearer dpl_abc123..." ...
```
<!-- ANCHOR_END: token-storage -->

---

<!-- ANCHOR: rotation -->
## Token Rotation

### Opaque PATs

1. Create new token with appropriate expiration
2. Update applications to use new token
3. Verify new token works in all contexts
4. Revoke old token via `DELETE /api/api-tokens/:id`

### Service Tokens (Ed25519)

1. Generate new Ed25519 keypair
2. Register new public key in `web.service_tokens`
3. Update service configuration with new private key
4. Verify service authenticates successfully
5. Revoke old service token record

### Scheduled Rotation

Consider implementing automated rotation:

- **PATs:** Every 90 days
- **Service keys:** Every 180 days
- **Ethereum signers:** Review allowlist quarterly
<!-- ANCHOR_END: rotation -->
