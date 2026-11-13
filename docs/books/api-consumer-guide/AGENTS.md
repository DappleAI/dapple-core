# Agent Reading Guide — API Consumer Guide

## What this book is for

This book documents how to **consume** the dapple-prompt REST API from outside the platform: typed Rust SDK, raw HTTP, authentication, batch semantics, ingest, and runtime journey/debrief/dispatch flows. It assumes the platform is already running somewhere; it does **not** explain how to author Guides (see `journey-planner-guide/`) or how the API layer is built internally (see `journey-builder-guide/`).

Read this if you are calling `/api/...` from a client. Skip it if you are extending the API itself.

> **Surface scope:** this book covers the **internal** surface (`api.dapple.x`, served by the `dapple-prompt-api` crate). Customer-tier (third-party) consumers authenticate to a separate **external** surface (`external-api.dapple.x`, served by `dapple-prompt-external-api`) with its own service-token table, its own signing key, and a deliberately narrower endpoint surface. See `crates/dapple-prompt-external-api/AGENTS.md` and `docs/books/_foundation/authentication/external-service-tokens.md`. The two surfaces share no JWT material — a token from one cannot be used on the other.

## Read this first if…

- …you need to authenticate a client → `getting-started/authentication.md` (token types: `dpl_` PAT, `dps_` service JWT, web session JWT).
- …you are uploading a Guide tarball → `ingest-api.md`.
- …you want the typed Rust client → `sdk-overview.md`.
- …you want a raw HTTP example → `api-cookbook.md`.
- …you need to start a Journey, run a Debrief, or fire a Dispatch → `runtime-operations.md`.
- …a request is failing and you need to interpret the response → `error-handling.md`.
- …you are searching Guides by natural-language query → `endpoint-reference.md` (look for `GET /api/guides/search`).

## Concepts you must already understand

Before reading any chapter, hold these in your head. The terse versions live in `docs/books/README.md` ("Start here, agent"); the canonical versions are in `CLAUDE.md` and the per-crate `AGENTS.md` files. Most relevant for API consumers:

- **Journey-centric vocabulary.** Users see *Playbook → Journey → Debrief → Dispatch*. The API uses this vocabulary in URLs and payloads. The legacy term *Brief* is still in some internal places, but treat *Debrief* as the user-facing name.
- **Batch semantics.** Every create/update endpoint takes `{ "objs": [...] }` and returns `{ "status", "results", "objs": [...] }`. Even when you have one object.
- **Token detection by prefix.** `dpl_` → opaque PAT; `dps_` → service JWT; no prefix → NextAuth web session JWT.
- **Versioned objects are content-addressable.** When you re-ingest a Guide whose hash matches the latest version, the API returns the existing version — there is no duplicate. Plan your "did anything change?" logic around this.
- **`query=all` returns 10 by default.** Pass `options={"limit":N}` for more. Prefer `by_slugs` for name lookups.
- **Embedding generation is async.** `POST /api/guides/ingest` returns before embeddings are written. `GET /api/guides/search` may briefly return zero hits for a brand-new guide while the conduit catches up.

If any of those bullets surprise you, read `_foundation/glossary.md` and `_foundation/authentication/overview.md` before continuing.

## Quick navigation

| Topic | File | Key information |
|-------|------|-----------------|
| SDK overview | `sdk-overview.md` | Typed Rust client, resource clients, authentication strategies |
| Guide ingest API | `ingest-api.md` | SDK and HTTP ingest, `dry_run`, versioning, error responses |
| Guide cards & favorites | `guide-cards-and-favorites.md` | Cards, favorites, PDF rendering, render profiles |
| Shared content (deferred) | `shared-content.md` | **Removed (V2+ deferral)** — `/api/shared-content/ingest` and its SDK client no longer exist |
| Authentication methods | `getting-started/authentication.md` | Token types, creation, usage |
| API patterns | `api-patterns.md` | Batch semantics, query variants, response formats |
| Endpoint reference | `endpoint-reference.md` | Full endpoint table including `/api/guides/search` |
| Directives & flow control | `directives.md` | JSONLogic actions, attaching directives to playbook stages/steps |
| Journey runtime | `runtime-operations.md` | Create journey, debrief, dispatch lifecycle |
| Code examples | `api-cookbook.md` | Copy-paste recipes for common flows |
| Error handling | `error-handling.md` | Status codes, common failure modes |

### SDK quick reference (`client.guides()`)

| Method | Purpose |
|--------|---------|
| `ingest_from_dir(path, params)` | Build tarball from a directory and ingest |
| `ingest(bytes, params)` | Ingest a pre-built tarball |
| `ingest_version(path_id, bytes, params)` | New version of an existing guide |
| `cards(options)` / `cards_by_category(cat, options)` | List guide cards |
| `favorite(id)` / `unfavorite(id)` / `is_favorite(id)` / `favorites()` | Favorites |
| `render_pdf(id, profile)` | Render guide as PDF using a render profile |
| `get(id)` / `list(options)` / `list_by_slugs(slugs, options)` | Read |
| `deprecate_one(id, level, reason)` | Soft delete |

### Directive action values

| Action | Behavior |
|--------|----------|
| `"proceed"` or `true` | Continue to next stage/step |
| `"skip"` | Skip this element |
| `"fail"` or `false` | Halt workflow |
| `"retry"` | Wait `retry_delay_ms`, try again (up to `max_attempts`) |
| `"loop"` | Try again immediately (up to `max_attempts`) |
| `"{uuid}"` | Branch to target stage/step |

The `run` primitive (`{ "run": [] }`) executes the attached stage/step at most once per evaluation; access results via `{ "var": "run.status" }`, `{ "var": "run.error_code" }`, `{ "var": "run.data.field" }`. Full JSONLogic syntax lives in `../journey-planner-guide/directives.md`.

### Common query patterns

```
GET /api/{resource}/?query=by_object_ids&ids=["uuid1","uuid2"]
GET /api/{resource}/?query=by_path_ids&path_ids=["path-uuid"]
GET /api/{resource}/?query=by_{parent}&{parent}_id=uuid
GET /api/{resource}/?query=by_slugs&slugs[0]=slug-1&slugs[1]=slug-2
GET /api/{resource}/?query=all&options={"limit":50,"offset":0}
```

### Response shape

```json
{
  "status": "success",
  "results": 2,
  "objs": [{ "id": "...", ... }]
}
```

## Cross-book pointers

| You want to… | Go to |
|--------------|-------|
| Author a Guide (write directives, Fluent files, guide.toml) | [`../journey-planner-guide/AGENTS.md`](../journey-planner-guide/AGENTS.md) |
| Understand the API internally (handler patterns, sql-composer, schema design) | [`../journey-builder-guide/AGENTS.md`](../journey-builder-guide/AGENTS.md) |
| Learn the auth model in depth | [`../_foundation/authentication/`](../_foundation/authentication/) |
| Look up platform terminology | [`../_foundation/glossary.md`](../_foundation/glossary.md) |
| Implement billing/audit features | [`../journey-auditor-guide/AGENTS.md`](../journey-auditor-guide/AGENTS.md) (most of it is still WIP) |
