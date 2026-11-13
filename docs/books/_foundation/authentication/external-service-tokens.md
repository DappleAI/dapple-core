# External Service Tokens (Ed25519) — Customer Tier

> **Audience:** Backend Developers · DevOps · Customer Integration Engineers
>
> **Scope:** This document covers **external** service tokens — the ones used by *customer companies* (third parties such as Acme) to authenticate to `external-api.dapple.x`. The implementation lives in the `dapple-prompt-external-api` crate; see [`crates/dapple-prompt-external-api/AGENTS.md`](../../../../crates/dapple-prompt-external-api/AGENTS.md) for crate-level invariants. For internal-tier tokens used by dapple-owned services, see [service-tokens.md](./service-tokens.md).

External service tokens use the same Ed25519 cryptography as internal service tokens, but are **structurally separated** at every layer: separate database table, separate issuance endpoint, separate signing key, separate router/extractor.

## Why a Separate Tier

Internal services (`dapple_ui`, `dapple_conduit`) and customer companies (Acme et al) have fundamentally different lifecycles, threat models, and audit needs:

- **Different ownership.** Internal tokens belong to dapple; external tokens belong to a customer company.
- **Different audit profile.** Internal tokens carry no per-company billing; external tokens are tied to a `rev.companies` row that may be billable.
- **Different blast radius if compromised.** An internal-token compromise is a dapple incident; an external-token compromise is a customer-isolated incident.
- **Different revocation cadence.** Customer onboarding/offboarding is far more frequent than dapple-team service rotation.

Combining the two in one table behind a CHECK constraint would be the same kind of "feature flag as auth boundary" mistake we reject elsewhere. Separate tables means separate issuance paths, separate extractors, and a hard failure mode if a token from one tier is presented to the wrong surface.

## Surface and Auth Topology

| Surface | Hostname | Token table | Signing key |
|---|---|---|---|
| Internal | `api.dapple.x` (cluster-internal) | `web.service_tokens` | internal-only |
| **External** | **`external-api.dapple.x` (public)** | **`rev.external_service_tokens`** | **external-only** |
| User (future) | `user-api.dapple.x` | TBD | user-only |

A JWT issued by one surface cannot be verified by another. Each surface loads only its own signing key; cross-surface replay fails at signature check, before any claim is read. This is *cryptographic* isolation — not just an `aud`-claim convention.

## Token Storage

External tokens live in **`rev.external_service_tokens`** (note: `rev` schema, not `web` — these are TUI-managed admin artifacts tied to companies, not web-app session data):

```sql
CREATE TABLE rev.external_service_tokens (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    service_id TEXT NOT NULL UNIQUE,
    company_id UUID NOT NULL REFERENCES rev.companies (id),
    name TEXT NOT NULL,
    description TEXT,
    public_key_pem TEXT NOT NULL,
    key_algorithm TEXT NOT NULL DEFAULT 'Ed25519',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by_user_id BIGINT REFERENCES rev.user_details (user_id),
    CONSTRAINT external_service_id_format
        CHECK (service_id ~ '^[a-z][a-z0-9_]{2,31}$')
);

CREATE TABLE rev.external_service_token_revocations (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    external_service_token_id UUID NOT NULL
        REFERENCES rev.external_service_tokens (id),
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_by_user_id BIGINT REFERENCES rev.user_details (user_id),
    reason TEXT
);
```

Each token references **exactly one company** via `company_id`. A company may have multiple service tokens (e.g., acme's production client + their staging client) but every token has exactly one owner.

## Token Structure

The on-the-wire token format is identical to internal tokens — the same 128-byte Ed25519-signed binary structure described in [service-tokens.md](./service-tokens.md#token-structure). The differences are in **where** the public key is registered (`rev.external_service_tokens` instead of `web.service_tokens`) and **which** signing key the issuance endpoint uses.

## Exchanging for JWT

External clients call **`external-api.dapple.x`**, not `api.dapple.x`:

```
POST https://external-api.dapple.x/api/get-service-token
Content-Type: application/json

{
  "token": "base64-encoded-128-byte-signed-token",
  "service_id": "acme_prod_client"
}
```

The handler verifies the signature against `rev.external_service_tokens.public_key_pem`, looks up the associated `rev.companies` row, refuses if `companies.revoked_at IS NOT NULL`, and issues a JWT signed with the **external surface's signing key**.

### External JWT Claims

```json
{
  "sub": "<company_id UUID>",
  "company_slug": "acme",
  "service_id": "acme_prod_client",
  "token_use": "external_service",
  "iss": "external-api.dapple.x",
  "exp": 1705312200
}
```

**Key claim differences from internal JWTs:**

- `sub` is the **company UUID**, not a service_id string.
- `company_slug` is included for human-readable display (logs, audit). UUID is the load-bearing identifier; slug is decoration.
- `token_use` is `"external_service"` (vs `"service"` for internal).
- `iss` is **`external-api.dapple.x`** (vs `api.dapple.x` for internal). Validators on a different surface refuse a JWT with the wrong `iss`, but signing-key separation makes the check redundant — verification fails first.
- No `roles` claim. Customer-tier roles are not modeled in V1; the surface itself is the authorization boundary.

### Delegated Tokens

External service tokens **do not support delegation**. There is no `auth_user_id` parameter; the customer's API does not act on behalf of dapple users. If a customer needs to track its own end-users, that's a concern of the customer's own service, not of dapple-prompt-external-api.

## Customer Onboarding

V1 onboarding is a manual admin operation (likely via the TUI). The flow:

1. Customer generates an Ed25519 keypair on their side. Private key stays with the customer.
2. dapple admin inserts a row into `rev.companies` (with the customer's slug, name, and the fixed dapple system org's UUID is preserved as a separate row).
3. dapple admin inserts one or more rows into `rev.external_service_tokens` with the customer's public key(s) and the `company_id` from step 2.
4. dapple admin shares `service_id` with the customer.
5. Customer signs a token, exchanges it at `external-api.dapple.x/api/get-service-token`, and uses the returned JWT for subsequent requests.

A future `POST /api/companies` admin endpoint may automate steps 2–3, but it is **not** a V1 deliverable. Until it exists, customer onboarding is a manual SQL or TUI action.

## What External Tokens Can Do

- Authenticate requests to **`external-api.dapple.x`**, and only that surface.
- Surface guides whose `object_paths.namespace_company_id` matches the token's `sub` (company_id) and whose `scope` is `'external'` or `'common'`.
- Create journeys, fetch briefs, post dispatch-intent-bindings, run dispatches against the customer's own content.

## What External Tokens Cannot Do

- ❌ Authenticate to `api.dapple.x`. Wrong surface; signature verification fails because the internal API loads a different signing key.
- ❌ Read or write content in another company's namespace. The `ExternalCaller` extractor binds every query to `WHERE namespace_company_id = caller.company_id`; there is no code path that ignores this binding.
- ❌ Use scope values `'public'` or `'internal'` (those are dapple-namespace-only; external tokens never produce them).
- ❌ Ingest guides, manage other companies, or hit any admin surface. The external API is deliberately narrow (V1: list-guides, create-journey, get-briefs, post-bindings, create-dispatch, poll-dispatch).
- ❌ Act on behalf of dapple users (no delegation).

## Revocation

Revocation is recorded by inserting into `rev.external_service_token_revocations`. The `ExternalCaller` extractor checks both that:

1. The token's `service_id` exists in `rev.external_service_tokens` and has no row in `rev.external_service_token_revocations`.
2. The associated `rev.companies` row has `revoked_at IS NULL`.

Either failure produces a 401 response. Revocation takes effect on the next token-exchange call (existing JWTs continue to work until they expire — typically within 1 hour).

To revoke an entire customer (all their tokens at once), set `revoked_at` on the `rev.companies` row. Individual token revocation (one of a customer's many tokens) uses the revocation table.

## Security Considerations

- **Cryptographic surface separation.** Each API surface has its own signing key infrastructure. Compromise of an external-tier signing key does not affect internal-tier tokens (or user-tier in the future). Rotation per surface is independent.
- **Per-company isolation.** Every external query filters by `namespace_company_id`. Cross-customer visibility is structurally impossible — there is no code path that returns one customer's content to another.
- **Public key only on the server side.** Customer keeps the private key locally; only the public key is registered.
- **Short JWT lifetime.** Default 1 hour. Customer SDKs should refresh before expiry (recommended at the 50-minute mark).
- **Audit trail.** Every external request carries a JWT linkable back to a specific `external_service_token_id` and `company_id`. Per-customer activity can be reconstructed from logs.

## Related Documents

- [service-tokens.md](./service-tokens.md) — internal-tier service tokens (`web.service_tokens`).
- [api-tokens.md](./api-tokens.md) — user-owned bearer tokens (a different mechanism entirely).
- [`crates/dapple-prompt-external-api/AGENTS.md`](../../../../crates/dapple-prompt-external-api/AGENTS.md) — crate-level invariants and module map for the external API surface.
