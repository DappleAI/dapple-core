# Introduction

> **Audience:** Customer Integration Engineers · Backend Developers · Agents

Welcome to the dapple-prompt **external** API. This guide shows a customer company how to authenticate, list and ingest guides, run journeys, and observe execution through the public surface at `external-api.dapple.x`.

> **Are you a dapple-internal service?** Use the internal surface (`api.dapple.x`) instead — see the [API Consumer Guide](../../api-consumer-guide/getting-started/intro.md). The two surfaces are cryptographically isolated; a token from one cannot be used on the other.

## What is the External Surface?

dapple-prompt is a workflow orchestration platform for LLM-powered applications. The **external surface** is the public, customer-facing slice of its REST API. It lets a third party run dapple workflows against its own content, fully isolated from every other customer.

| Property | Value |
|----------|-------|
| Base URL | `https://external-api.dapple.x/api/` |
| Listener port | `8081` |
| Content-Type | `application/json` |
| Authentication | `Authorization: Bearer <jwt>` (from token exchange) |
| Token table | `rev.external_service_tokens` |
| JWT issuer (`iss`) | `external-api.dapple.x` |
| JWT use (`token_use`) | `external_service` |

### How it is isolated

- **Cryptographic isolation.** The external surface signs JWTs with its own key. A JWT minted on the internal surface fails verification here, and vice versa.
- **Per-company data isolation.** Your JWT's `sub` claim is your company UUID. Every query is filtered by that `company_id` and restricted to `scope = 'external'` content. Another company's data is structurally invisible.
- **No existence leak.** A foreign or unknown id returns the same `404 RESOURCE_NOT_FOUND` whether it belongs to another company or does not exist. You cannot probe for other companies' resources.

## The V1 Endpoint Set

The external surface is deliberately narrow. The whole V1 surface:

| Endpoint | Purpose |
|---|---|
| `POST /api/get-service-token` | Exchange an Ed25519-signed token for a JWT |
| `GET /api/guides` | List your company's guides |
| `POST /api/guides/ingest` | Upload a tarballed guide |
| `PATCH /api/guides/{path_id}/ingest` | Re-ingest / upsert an existing guide |
| `POST /api/journeys` | Create a journey on your playbook |
| `GET /api/journeys` | List your journeys |
| `GET /api/journeys/{id}` | Fetch one journey |
| `GET /api/journeys/{id}/briefs` | Per-journey brief list |
| `GET /api/journeys/{id}/stages` | Per-stage completion |
| `GET /api/journeys/{id}/steps` | Per-step exposed-slot status |
| `POST /api/dispatches` | Create a dispatch (auto-executes when ready) |
| `GET /api/dispatches/{id}` | Poll dispatch status |
| `GET /api/dispatches/{id}/briefs` | Per-dispatch brief state |
| `GET /api/dispatches/{id}/task_attempts` | Per-task runtime phase (read-only) |
| `POST /api/dispatch-intent-bindings` | Submit value-slot bindings |

See the [Endpoint Reference](../endpoint-reference.md) for response field summaries.

## Quick Start

### 1. Exchange a Token for a JWT

You hold an Ed25519 private key registered with dapple. Sign a token locally and exchange it:

```bash
curl -X POST https://external-api.dapple.x/api/get-service-token \
  -H "Content-Type: application/json" \
  -d '{
    "token": "base64-encoded-128-byte-signed-token",
    "service_id": "your_company_prod_client"
  }'
```

The response is a JWT signed with the external surface's key. Use it as `Authorization: Bearer <jwt>` on every subsequent request. See [Authentication](./authentication.md) for the full flow.

### 2. List Your Guides

```bash
curl https://external-api.dapple.x/api/guides \
  -H "Authorization: Bearer <jwt>"
```

This returns only your company's guides (`scope = 'external'`). There is no "list all" variant.

### 3. Start a Journey

You already hold the exact `guide_id` (guide version) and `playbook_id` (playbook version) you navigated to. Pass both:

```bash
curl -X POST https://external-api.dapple.x/api/journeys \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "My First Journey",
      "guide_id": "guide-version-uuid",
      "playbook_id": "playbook-version-uuid",
      "brief_pack_ids": ["briefpack-uuid"],
      "inference_profile_ids": ["profile-uuid"]
    }]
  }'
```

The backend validates that the guide exposes that playbook version. A mismatch returns `400 GUIDE_PLAYBOOK_MISMATCH`; a foreign guide returns `404`.

## Next Steps

- [Authentication](./authentication.md) — the Ed25519 → JWT exchange in detail
- [Core Concepts](./concepts.md) — the journey vocabulary and step model
- [Endpoint Reference](../endpoint-reference.md) — the full V1 surface
- [Customer Runtime Flow](../runtime-operations.md) — run a journey end to end
