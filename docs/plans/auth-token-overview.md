
5. **HTTPS only**: Always use TLS in production.
6. **Scope validation**: Backend should enforce scope permissions on service tokens.

---

## Questions for Discussion

1. Should we support key rotation (multiple valid addresses per service)?
2. Do we need audit logging for token issuance?
3. Should scopes be validated against a predefined list, or accept any?
4. Do we want rate limiting on the auth endpoint?
