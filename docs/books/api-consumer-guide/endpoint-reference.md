# Endpoint Reference

> **Audience:** API Consumers · Integration Developers

Complete reference for all API endpoints.

> **Note:** Most entities in the Setup Endpoints section below are created automatically during [guide ingest](./ingest-api.md). Direct creation via POST is an advanced path for cases where guide authoring doesn't apply.

## API Basics

| Property | Value |
|----------|-------|
| Base URL | `/api/` |
| Content-Type | `application/json` |
| Authentication | `Authorization: Bearer <token>` |
| OpenAPI Spec | `GET /api.json` |
| Documentation | `GET /docs` (ReDoc) |

## Public Endpoints

These endpoints do not require authentication:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health_check` | GET | Health check |
| `/api.json` | GET | OpenAPI specification |
| `/docs` | GET | ReDoc documentation |
| `/api/get-auth-token` | POST | Obtain JWT from signed auth token |

## Authentication Endpoints

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `/api/get-auth-token` | POST | Public | Exchange signed auth token for JWT |
| `/api/api-tokens/` | POST | Required | Create long-lived API token |
| `/api/api-tokens/` | GET | Required | List user's API tokens |
| `/api/api-tokens/` | PATCH | Required | Revoke API token |

## Guide Ingest Endpoints

These endpoints handle guide ingestion. (Shared-content ingest is deferred to V2+ — see the struck-through row below.)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/guides/ingest` | POST | Ingest a new guide from tarball (multipart). See [Guide Ingest Pipeline](./ingest-api.md). |
| `/api/guides/{path_id}/ingest` | PATCH | Ingest a new version of an existing guide |
| `/api/guides/search` | GET | Natural-language search over guide content (pgvector). Query params: `q` (required), `tags`, `locale`, `limit`, `guide_ids`, `threshold`. Embeddings are generated asynchronously after ingest, so brand-new guides may briefly return no hits. Returns 503 if the conduit's embedding service is not configured. |
| `/api/guide-cards/` | GET | List guide cards (filterable by category) |
| `/api/guides/favorites` | GET | List the user's favorite guides → `{ objs: [{ guide: { id, title }, created_at }] }` |
| `/api/guides/{id}/favorite` | POST | Add a guide to favorites (no body; `{id}` is the guide path id). Idempotent → `{ guide: { id, title }, created_at }` |
| `/api/guides/{id}/favorite` | DELETE | Remove a guide from favorites |
| `/api/guides/{id}/favorite` | GET | Check if a guide is favorited → `{ is_favorited }` |
| `/api/guides/{id}/render` | GET | Render guide as PDF (query: `?profile=consumer`) |
| ~~`/api/shared-content/ingest`~~ | ~~POST~~ | **Removed (V2+ deferral).** See [`shared-content.md`](./shared-content.md). |

## Setup Endpoints (Admin / Configuration)

These endpoints define templates and configuration used by production journeys. Most are populated automatically during guide ingest -- direct POST is an advanced path.

### Workflow Resources

| Endpoint | POST | GET | PATCH | Typical Source | Description |
|----------|------|-----|-------|---------------|-------------|
| `/api/tasks/` | Y | Y | WIP | Guide-derived | Workflow task definitions |
| `/api/steps/` | Y | Y | WIP | Guide-derived | Step definitions (groups of tasks) |
| `/api/stages/` | Y | Y | WIP | Guide-derived | Stage definitions (groups of steps) |
| `/api/playbooks/` | Y | Y | WIP | Guide-derived | Workflow templates |
| `/api/categories/` | Y | Y | WIP | Manual | Playbook organization |
| `/api/hats/` | Y | Y | WIP | Manual | User role definitions |

### Versioned Resources

| Endpoint | POST | GET | PATCH | Description |
|----------|------|-----|-------|-------------|
| `/api/value-slots/` | Y | Y | Y | Typed value definitions |
| `/api/value-slot-paths/` | - | Y | - | Version history for value slots |
| `/api/briefs/` | Y | Y | Y | Debrief UI containers |
| `/api/brief-paths/` | - | Y | - | Version history for briefs |
| `/api/brief-groups/` | Y | Y | Y | Atomic bundles of briefs |
| `/api/brief-group-paths/` | - | Y | - | Version history for brief groups |
| `/api/brief-packs/` | Y | Y | Y | Journey-assignable debrief content |
| `/api/brief-pack-paths/` | - | Y | - | Version history for brief packs |
| `/api/fluent-messages/` | Y | Y | Y | Localized text |
| `/api/fluent-message-paths/` | - | Y | - | Version history for fluent messages |
| `/api/prompts/` | Y | Y | WIP | LLM prompt templates |
| `/api/prompt-paths/` | - | Y | - | Version history for prompts |
| `/api/assets/` | Y | Y | Y | Binary files (images, documents) |
| `/api/asset-paths/` | - | Y | - | Version history for assets |
| `/api/directives/` | Y | Y | Y | JSONLogic conditional execution rules |
| `/api/directive-paths/` | - | Y | - | Version history for directives |
| `/api/inference-profiles/` | Y | Y | WIP | Model configuration for prompts |
| `/api/inference-profile-paths/` | - | Y | - | Version history for inference profiles |
| `/api/guides/` | Y | Y | WIP | Structured documentation (chapters, blocks, tags). **No separate `-paths/` endpoint** -- use `by_path_ids` query variant. Supports DELETE for deprecation. See [Guide Ingest API](./ingest-api.md). |

### Nested Resources

| Endpoint | POST | GET | PATCH | Description |
|----------|------|-----|-------|-------------|
| `/api/briefs/{id}/units/` | - | Y | - | UI elements (read-only; units are created inline via `POST /api/briefs/`) |

**Note:** WIP indicates the endpoint exists but PATCH functionality is not yet implemented.

## Production Endpoints (Runtime)

These endpoints manage user journeys and workflow execution.

### Journey Management

| Endpoint | POST | GET | PATCH | Description |
|----------|------|-----|-------|-------------|
| `/api/journeys/` | Y | Y | WIP | User workflow sessions. **POST requires `guide_id` and `playbook_id`** (both specific version ids; the backend validates the pair → 400 `GUIDE_PLAYBOOK_MISMATCH`). |
| `/api/journeys/{id}/briefs` | - | Y | - | All briefs for a journey (efficient fetch) |
| `/api/journeys/{id}/steps` | - | Y | - | Per-step exposed-slot status. Returns `{objs:[{stage_step_id, playbook_stage_id, stage_name, stage_description, stage_kind?, done, exposed_slots:[{value_slot_id, name, value_kind, bound, producer, input?}]}]}`. Each slot is self-describing: `producer` is `"operator"` (render the `input` spec — `control_type`, label/help/placeholder, `choices` — and POST a binding) or `"computed"` (server-produced; show a waiting state, no `input`). `value_slot_id` is populated for both. `stage_name`/`stage_description`/`stage_kind`/`stage_icon` are the owning stage's authored identity, repeated identically across a stage's rows (fold first-seen-wins); `stage_kind` is a closed vocabulary (`intake`, `fetch`, `analyze`, `decide`, `review`, `compare`, `assemble`, `deliver`), absent when unauthored — treat an unknown or absent value as your neutral default, never an error. `stage_icon` is an ordered array of author-set icon names from an **open** vocabulary owned by the drawing client — up to five, absent when unauthored (absent and `[]` mean the same thing). If you draw icons, take one and **degrade to your `stage_kind` glyph on any name you do not recognize**: an unknown name is the expected state, not skew. If you do not draw icons, ignore the field. Fine-grained "which step is awaiting input" signal. |
| `/api/journeys/{id}/stages` | - | Y | - | Per-stage completion roll-up. Returns `{objs:[{playbook_stage_id, name, description, kind?, icon?, done}]}`. A stage is `done` when every step it contains is done. `name`/`description`/`kind`/`icon` are the stage's authored identity (same vocabularies and absent-tolerance as `/steps` — `kind` closed, `icon` an open ordered array you degrade from). |

### Workflow Execution

| Endpoint | POST | GET | PATCH | Description |
|----------|------|-----|-------|-------------|
| `/api/dispatches/` | Y | Y | WIP | Workflow execution runs. A dispatch is a batch of one-or-more steps; execution starts **automatically** once the dispatch is `dispatch_ready` (all overlay-exposed slots bound). |
| `/api/dispatch-intent-bindings/` | Y | Y | - | Debrief results (values collected). Submitting a binding can flip a dispatch to `dispatch_ready` and trigger its execution. |
| `/api/dispatches/{id}/task_attempt/` | Y | Y | - | **Internal-only, optional** — execution is automatic; observe via the journey status endpoints (`/steps`, `/stages`) instead. Manual task-attempt tracking (singular path). |
| `/api/dispatches/{id}/task_attempt/{id}/events` | Y | - | - | **Internal-only, optional** — execution is automatic; observe via the journey status endpoints instead. Manual task status updates. |

## Reference Endpoints (Read-Only)

System-managed reference data. Read-only — no POST/PATCH/DELETE.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/user-details/` | GET | User profile reference data (email, id) |
| `/api/user-details/{id}` | GET | Single user detail by object ID |

## Utility Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health_check` | GET | Health check (public) |
| `/api.json` | GET | OpenAPI spec (public) |
| `/docs` | GET | ReDoc documentation (public) |
| `/api/deprecations/` | GET | Deprecation notices |

## Query Patterns

All GET endpoints support query parameters to filter results.

### General Pattern

```
GET /api/{resource}/?query={query_type}&{params}
```

### By Object IDs

```
GET /api/tasks/?query=by_object_ids&ids[0]=uuid-1&ids[1]=uuid-2
```

Fetch specific objects by their `id`.

### By Path IDs (Versioned Objects)

```
GET /api/briefs/?query=by_path_ids&path_ids[0]=path-uuid
```

Fetch all versions of a versioned object by its `path_id`.

### By Parent Relationship

```
GET /api/steps/?query=by_playbook&playbook_id=uuid
```

Fetch resources related to a parent object.

### By Slugs

```
GET /api/tasks/?query=by_slugs&slugs[0]=send-email
```

Fetch objects by human-readable slug. Available for: **tasks, steps, stages, playbooks, categories, hats, guides**.

Slugs are auto-generated from names: lowercase, whitespace replaced with hyphens (e.g. `"Send Email"` → `"send-email"`).

### Filter Options

Add pagination and limits:

```
GET /api/tasks/?query=by_object_ids&ids[0]=uuid-1&ids[1]=uuid-2&options={"limit":10,"page":0}
```

> **Important:** `query=all` means "no filter" — it does **not** return all results. It applies the default pagination limit of **10**. Always pass `options={"limit":N}` if you need more than 10 results.

**Available options:**
- `limit` - Maximum results per page (default: 10)
- `page` - Page number (0-indexed)

## Response Formats

### Success Response (Single)

```json
{
  "status": "success",
  "results": 1,
  "obj": { "id": "uuid", ... }
}
```

### Success Response (Multiple)

```json
{
  "status": "success",
  "results": 3,
  "objs": [
    { "id": "uuid-1", ... },
    { "id": "uuid-2", ... },
    { "id": "uuid-3", ... }
  ]
}
```

### Error Response

```json
{
  "status": "fail",
  "message": "Detailed error description"
}
```

## Batch Semantics

All create and update operations use batch semantics:

**Request:**
```json
{
  "objs": [
    { "name": "Task 1", ... },
    { "name": "Task 2", ... }
  ]
}
```

**Response:**
```json
{
  "status": "success",
  "results": 2,
  "objs": [
    { "id": "uuid-1", "name": "Task 1", ... },
    { "id": "uuid-2", "name": "Task 2", ... }
  ]
}
```

## HTTP Status Codes

| Code | Meaning | When It Happens |
|------|---------|-----------------|
| 200 | OK | Successful GET request |
| 201 | Created | Successful POST request |
| 400 | Bad Request | Validation error, malformed request |
| 401 | Unauthorized | Missing or invalid JWT |
| 403 | Forbidden | Valid JWT but insufficient permissions |
| 404 | Not Found | Resource doesn't exist |
| 409 | Conflict | Duplicate key constraint violation |
| 500 | Internal Server Error | Unexpected server error |
| 503 | Service Unavailable | Missing server configuration |

See [Error Handling](./error-handling.md) for detailed error resolution.
