# API Patterns & Reference

> **Audience:** Prompt Engineers · Product · Agents

This chapter covers the core API patterns, response formats, query mechanisms, and endpoint reference for the dapple-prompt REST API.

---

## API Basics

| Property | Value |
|----------|-------|
| Base URL | `/api/` |
| Content-Type | `application/json` |
| Authentication | `Authorization: Bearer <JWT>` |
| OpenAPI Spec | `GET /api.json` |
| Documentation | `GET /docs` (ReDoc) |

### Public Endpoints

The following endpoints do not require authentication:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health_check` | GET | Health check |
| `/api.json` | GET | OpenAPI specification |
| `/docs` | GET | ReDoc documentation |
| `/api/get-auth-token` | POST | Obtain JWT from signed auth token |

All other endpoints require a valid JWT in the `Authorization: Bearer <token>` header.

> **Agent Note:** For programmatic access, API tokens (prefixed with `dpl_`) are recommended over JWTs. They are simpler to manage and do not require OAuth flows. See [Authentication](./authentication.md) for token creation.
> - API tokens have a rate limit of 60 req/min (vs 300 for web sessions)
> - Token type is detected by prefix: `dpl_` indicates an API token, no prefix indicates a JWT

> **Typed SDK:** The `dapple-sdk` crate provides typed Rust methods that handle envelope wrapping, query serialization, and error parsing automatically. If you are a Rust consumer, prefer the SDK over raw HTTP. See [The Dapple SDK](../../journey-planner-guide/dapple-sdk.md).

---

## Batch Semantics

All create and update operations use batch semantics. Wrap objects in `{ "objs": [...] }` for POST/PATCH requests.

**Request (POST):**
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

This design enables efficient bulk operations and consistent API patterns.

---

## Response Formats

### Success Responses

**Single object:**
```json
{
  "status": "success",
  "results": 1,
  "obj": { "id": "uuid", ... }
}
```

**Multiple objects:**
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

**Empty result:**
```json
{
  "status": "success",
  "results": 0,
  "objs": []
}
```

### Error Responses

```json
{
  "status": "fail",
  "message": "Detailed error description"
}
```

**Special error formats (for auth-token endpoint):**
```json
{
  "error": "ERROR_CODE",
  "message": "Detailed explanation"
}
```

### HTTP Status Codes

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

---

## Query Patterns

All GET endpoints support query parameters to filter results.

**General pattern:**
```
GET /api/{resource}/?query={query_type}&{params}
```

### Common Query Types

#### By Object IDs

```
GET /api/tasks/?query=by_object_ids&ids[0]=uuid-1&ids[1]=uuid-2
```

Fetch specific objects by their `id`.

#### By Path IDs (Versioned Objects)

```
GET /api/briefs/?query=by_path_ids&path_ids[0]=path-uuid
```

Fetch all versions of a versioned object by its `path_id`.

#### By Parent Relationship

```
GET /api/steps/?query=by_playbook&playbook_id=uuid
```

Fetch resources related to a parent object.

#### By Slugs

```
GET /api/tasks/?query=by_slugs&slugs[0]=send-email&slugs[1]=review-doc
```

Fetch specific objects by their human-readable slug. Available for entities with slug columns: **tasks, steps, stages, playbooks, categories, hats, guides**.

Slugs are auto-generated from names: lowercase, whitespace replaced with hyphens.
- `"Send Email"` → `"send-email"`
- `"Review Document"` → `"review-document"`

**Use when:**
- Looking up entities by name (more efficient than listing + filtering)
- Recovering from 409 conflicts (fetch the existing entity by its known slug)
- Implementing idempotent setup scripts

### SDK Query Handling

When using the `dapple-sdk` crate, query patterns are handled automatically:

- The SDK serializes `GetXSetSchema` enums as query parameters using serde, so you do not need to construct query strings manually.
- `FilterOptions` (limit, page) are passed through as `Option<FilterOptions>` on methods that support pagination.
- The `{ "objs": [...] }` envelope is handled automatically by the client for both requests and responses.

```rust
// SDK handles query serialization and envelope unwrapping
let tasks = client.tasks().list_by_object_ids(
    vec![uuid_1, uuid_2],
    Some(FilterOptions { limit: 10, page: 0 }),
).await?;
// Returns Vec<Task> directly - no envelope parsing needed
```

### Filter Options

Add pagination and limits:

```
GET /api/tasks/?query=by_object_ids&ids[0]=uuid-1&ids[1]=uuid-2&options={"limit":10,"page":0}
```

> **Important:** `query=all` means "no filter" — it does **not** return all results. It applies the default pagination limit of **10**. Always pass `options={"limit":N}` if you need more than 10 results.

**Available options:**
- `limit` - Maximum results per page (default: 10)
- `page` - Page number (0-indexed)

---

## Object Types

### Principal Objects

Standalone entities with independent lifecycles:
- **Task** - Atomic work unit
- **Step** - Collection of tasks
- **Stage** - Collection of steps
- **Playbook** - Complete workflow template
- **Category** - Organization and filtering
- **Hat** - User role definitions
- **Journey** - User's workflow instance (references Playbook, BriefPacks, and InferenceProfiles)
- **InferenceProfile** - Model configuration for prompts (versioned)

**Common fields:**
- `id` - UUID for external references
- `name` - Human-readable identifier
- `description` - Optional detailed description
- `created_at` - Timestamp of creation

### Versioned Objects

Immutable entities with version history:
- **ValueSlot** - Typed value definitions
- **Brief** - Debrief UI container
- **BriefGroup** - Atomic bundle of briefs
- **BriefPack** - Journey-assignable debrief content
- **FluentMessage** - Localized text
- **Prompt** - LLM prompt templates
- **Asset** - Binary files (images, documents, etc.)
- **Directive** - JSONLogic conditional execution rules (retry policies, branching logic)
- **Guide** - Structured documentation with chapters, blocks, and audience tags (versioned)

**Common fields:**
- `id` - UUID for this specific version
- `path_id` - UUID for the version lineage
- `version` - `{ "major": 0, "minor": 0, "patch": 1 }`
- `created_at` - Timestamp of this version's creation

**Versioning behavior:**
- POST creates version 0.0.1
- PATCH creates new version (increments patch by default)
- GET by `path_id` returns all versions
- GET by `id` returns specific version

### Nested Resources

Resources accessed via parent object URL:
- **BriefUnits** - `/api/briefs/{id}/units/`
- **TaskAttempt** - `/api/dispatches/{id}/task_attempt/`
- **TaskAttempt Events** - `/api/dispatches/{id}/task_attempt/{id}/events`

These do not have top-level endpoints. They're always created and queried through their parent.

> **Execution is automatic.** You do **not** drive task execution by POSTing
> TaskAttempts or events. A dispatch auto-spawns its work the moment it is
> `dispatch_ready` (every overlay-exposed slot bound). The TaskAttempt and
> events endpoints are **observability-only and internal-surface-only** — read
> them to watch progress if you want, but they are never required, and they are
> absent on the external surface entirely. To watch progress portably, poll the
> status endpoints (`GET /api/journeys/{id}/steps` and
> `GET /api/journeys/{id}/stages`).

### Associative Tables

Many-to-many relationships between principals. These do NOT have separate API endpoints. They're created atomically with their parent object.

**Examples:**
- `brief_group_briefs` - Briefs in a BriefGroup
- `brief_pack_groups` - BriefGroups in a BriefPack
- `playbook_stages` - Stages in a Playbook (auto-created from submitted stage entries)
- `playbook_step` - Steps in a Playbook
- `playbook_task` - Tasks in a Playbook
- `task_prompts` - Prompts attached to a Task (created atomically with task)
- `journey_brief_packs` - BriefPacks attached to a Journey
- `journey_inference_profiles` - InferenceProfiles attached to a Journey
- `playbook_stage_directives` - Directives attached to a Playbook Stage
- `playbook_step_directives` - Directives attached to a Playbook Step

**Pattern:**
```json
POST /api/brief-groups/
{
  "objs": [{
    "name": "onboarding",
    "brief_ids": ["brief-1", "brief-2"]
  }]
}
```

The `brief_ids` field creates the associative entries atomically.

Playbooks follow the same pattern — stages are submitted inline and auto-created:
```json
POST /api/playbooks/
{
  "objs": [{
    "name": "onboarding-flow",
    "description": "New user onboarding",
    "stages": [
      { "steps": [{ "step_id": "step-1", "params": {}, "tasks": [{ "task_id": "task-1" }] }] },
      { "steps": [{ "step_id": "step-2", "params": {}, "tasks": [{ "task_id": "task-2" }] }] }
    ],
    "categories": []
  }]
}
```

Each entry in `stages` creates one auto-stage and its associated `playbook_stages` row.

### Associative Object Lifecycle

When you create a parent object with nested associative entries, the **server assigns `id` values** to each associative row. These server-assigned IDs are critical for subsequent operations like Dispatch creation.

#### 1. Server Assigns Associative IDs

When you POST a Playbook with inline stages/steps/tasks, the server creates the playbook-stage, stage-step, and step-task associative rows — each with its own `id`. You do **not** supply these IDs; the server generates them.

#### 2. Playbook Response Includes Associative IDs

The Playbook response DTO contains the full nested structure with server-assigned IDs:

```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "playbook-uuid",
    "name": "onboarding-flow",
    "stages": [
      {
        "id": "playbook-stage-uuid",
        "playbook_id": "playbook-uuid",
        "steps": [
          {
            "id": "stage-step-uuid",
            "step": { "id": "step-1" },
            "tasks": [
              {
                "id": "step-task-uuid",
                "task": { "id": "task-1" }
              }
            ]
          }
        ]
      }
    ]
  }]
}
```

In this response:
- `stages[].id` → the associative playbook-stage row ID
- `stages[].steps[].id` → the associative **stage-step** row ID (this is what a dispatch's `stage_step_id` points at)
- `stages[].steps[].step.id` → the principal Step's ID (the one you submitted)
- `stages[].steps[].tasks[].id` → the associative **step-task** row ID (this is what a dispatch's `step_task_id` points at)
- `stages[].steps[].tasks[].task.id` → the principal Task's ID (the one you submitted)

#### 3. Dispatch Creation References Associative IDs

A dispatch create body is a **flat list of `steps[]`** — there is no top-level
`stages` array. Each step carries a `stage_step_id` (the associative
**stage-step** row from the Playbook response) and a list of `tasks[]`, each
carrying a `step_task_id` (the associative **step-task** row). Stages are
**derived server-side** by grouping the submitted steps; you never name them.

> **Critical:** A dispatch's `steps[].stage_step_id` and
> `steps[].tasks[].step_task_id` reference the **associative row `id` values**
> from the Playbook response — **NOT** the principal Step/Task `id` values
> (`step.id` / `task.id`). There is no `playbook_stage_id`, `playbook_step_id`,
> or `playbook_task_id` field in the dispatch body.

**ID mapping from Playbook response → Dispatch create request:**

| Playbook response field | Dispatch create field |
|---|---|
| `stages[].steps[].id` | `steps[].stage_step_id` |
| `stages[].steps[].tasks[].id` | `steps[].tasks[].step_task_id` |

**Example — using Playbook response to create a Dispatch:**

```json
POST /api/dispatches/
{
  "objs": [{
    "journey_id": "journey-uuid",
    "intent": { "source": "web_app" },
    "steps": [
      { "stage_step_id": "stage-step-uuid", "params": {},
        "tasks": [ { "step_task_id": "step-task-uuid" } ] }
    ]
  }]
}
```

Notice the IDs come from `stages[].steps[].id` and `stages[].steps[].tasks[].id`
in the Playbook response — not from the nested `.step.id` or `.task.id` fields.

> **A dispatch is a batch of one-or-more steps.** You never submit a `stages`
> array — stages are derived server-side by grouping the submitted steps. To
> fire a whole stage in one dispatch, include all of that stage's steps in the
> flat `steps[]` list. Once submitted, the dispatch auto-spawns its work as soon
> as it is `dispatch_ready` (every overlay-exposed slot is bound) — there is no
> manual "start execution" call.

---

## Endpoint Reference

### Authentication

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `/api/get-auth-token` | POST | Public | Exchange signed auth token for JWT |
| `/api/api-tokens/` | POST | Required | Create long-lived API token |
| `/api/api-tokens/` | GET | Required | List user's API tokens |
| `/api/api-tokens/` | PATCH | Required | Revoke API token |

See [Authentication](./authentication.md) for details.

### Setup Endpoints (Admin / Configuration)

These endpoints define templates and configuration used by production journeys.

| Endpoint | POST | GET | PATCH | Description |
|----------|------|-----|-------|-------------|
| `/api/tasks/` | ✓ | ✓ | WIP | Workflow task definitions |
| `/api/steps/` | ✓ | ✓ | WIP | Step definitions (groups of tasks) |
| `/api/stages/` | ✓ | ✓ | WIP | Stage definitions (groups of steps) |
| `/api/playbooks/` | ✓ | ✓ | WIP | Workflow templates |
| `/api/categories/` | ✓ | ✓ | WIP | Playbook organization |
| `/api/hats/` | ✓ | ✓ | WIP | User role definitions |
| `/api/value-slots/` | ✓ | ✓ | ✓ | Typed value definitions |
| `/api/value-slot-paths/` | - | ✓ | - | Version history for value slots |
| `/api/briefs/` | ✓ | ✓ | ✓ | Debrief UI containers |
| `/api/brief-paths/` | - | ✓ | - | Version history for briefs |
| `/api/briefs/{id}/units/` | - | ✓ | - | UI elements (read-only; units are created inline via `POST /api/briefs/`) |
| `/api/brief-groups/` | ✓ | ✓ | ✓ | Atomic bundles of briefs |
| `/api/brief-group-paths/` | - | ✓ | - | Version history for brief groups |
| `/api/brief-packs/` | ✓ | ✓ | ✓ | Journey-assignable debrief content |
| `/api/brief-pack-paths/` | - | ✓ | - | Version history for brief packs |
| `/api/fluent-messages/` | ✓ | ✓ | ✓ | Localized text |
| `/api/fluent-message-paths/` | - | ✓ | - | Version history for fluent messages |
| `/api/prompts/` | ✓ | ✓ | WIP | LLM prompt templates |
| `/api/prompt-paths/` | - | ✓ | - | Version history for prompts |
| `/api/assets/` | ✓ | ✓ | ✓ | Binary files (images, documents) |
| `/api/asset-paths/` | - | ✓ | - | Version history for assets |
| `/api/directives/` | ✓ | ✓ | ✓ | JSONLogic conditional execution rules |
| `/api/directive-paths/` | - | ✓ | - | Version history for directives |
| `/api/inference-profiles/` | ✓ | ✓ | WIP | Model configuration for prompts |
| `/api/inference-profile-paths/` | - | ✓ | - | Version history for inference profiles |
| `/api/guides/` | ✓ | ✓ | WIP | Structured documentation (chapters, blocks, tags) |

**Note:** WIP (Work In Progress) indicates the endpoint exists but PATCH functionality is not yet implemented.

### Production Endpoints (Runtime)

These endpoints manage user journeys and workflow execution.

| Endpoint | POST | GET | PATCH | Description |
|----------|------|-----|-------|-------------|
| `/api/journeys/` | ✓ | ✓ | WIP | User workflow sessions |
| `/api/journeys/{id}/briefs` | - | ✓ | - | All briefs for a journey (efficient fetch) |
| `/api/journeys/{id}/steps` | - | ✓ | - | Per-step exposed-slot status: `done` + `exposed_slots[]` (which sub-interaction is awaiting a binding) |
| `/api/journeys/{id}/stages` | - | ✓ | - | Per-stage completion roll-up (`done` per playbook stage) |
| `/api/dispatches/` | ✓ | ✓ | WIP | Workflow execution runs (auto-spawn when `dispatch_ready`) |
| `/api/dispatch-intent-bindings/` | ✓ | ✓ | - | Debrief results (values collected) |
| `/api/dispatches/{id}/task_attempt/` | ✓ | ✓ | - | **Internal-only, optional** — execution is automatic; use the status endpoints above to observe progress. GET reads runtime task phases |
| `/api/dispatches/{id}/task_attempt/{id}/events` | ✓ | ✓ | - | **Internal-only, optional** — per-attempt event log (observability) |

> **Execution is automatic.** Dispatches run themselves once `dispatch_ready`;
> you do not POST task_attempts or events to drive them. Those endpoints are
> observability-only, exist on the **internal surface only**, and are absent on
> the external surface. To watch progress portably, poll
> `GET /api/journeys/{id}/steps` and `GET /api/journeys/{id}/stages`.

### Reference Endpoints (Read-Only)

System-managed reference data. These are read-only — consumers query but never create or modify these resources.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/user-details/` | GET | User profile reference data (email, id) |
| `/api/user-details/{id}` | GET | Single user detail by ID |

**`/api/user-details/`** — Provides user identity lookup for cross-referencing. Queried via `by_object_ids` or `all`. User details are auto-created when a user first authenticates; there is no consumer-facing POST. Cache aggressively — user details change infrequently.

### Utility Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health_check` | GET | Health check (public) |
| `/api.json` | GET | OpenAPI spec (public) |
| `/docs` | GET | ReDoc documentation (public) |
| `/api/deprecations/` | GET | Deprecation notices |

---

## Creating Resources

### Pattern for Principal Objects

**1. Create the resource:**
```json
POST /api/tasks/
{
  "objs": [
    {
      "name": "send-email",
      "description": "Send email to user",
      "function": "email_send",
      "function_input": { "template": "welcome" }
    }
  ]
}
```

**2. Response includes assigned IDs:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [
    {
      "id": "task-uuid",
      "name": "send-email",
      "description": "Send email to user",
      "function": "email_send",
      "function_input": { "template": "welcome" },
      "created_at": "2024-01-22T10:30:00Z"
    }
  ]
}
```

**3. Use `id` to reference in other resources:**
```json
POST /api/steps/
{
  "objs": [{
    "name": "notification-step",
    "tasks": [
      { "task_id": "task-uuid" }
    ]
  }]
}
```

### Pattern for Versioned Objects

**1. Create first version (POST):**
```json
POST /api/value-slots/
{
  "objs": [{
    "name": "user-name",
    "value_kind": "text",
    "title": "User Name",
    "constraints": { "minLength": 2 }
  }]
}
```

**2. Response includes path and version:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "slot-uuid",
    "path_id": "path-uuid",
    "name": "user-name",
    "version": { "major": 0, "minor": 0, "patch": 1 },
    "created_at": "2024-01-22T10:30:00Z"
  }]
}
```

**3. Create new version (PATCH):**
```json
PATCH /api/value-slots/
{
  "objs": [{
    "path_id": "path-uuid",
    "title": "Full Name",
    "constraints": { "minLength": 3 }
  }]
}
```

**4. Response includes incremented version:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "new-slot-uuid",
    "path_id": "path-uuid",
    "version": { "major": 0, "minor": 0, "patch": 2 },
    "title": "Full Name",
    "created_at": "2024-01-22T10:31:00Z"
  }]
}
```

### Pattern for Nested Resources

A brief's units are **created inline with the parent brief** and **read** via the
parent URL — there is no nested `POST`.

**1. Create the parent with its nested units (and value-slot outputs) inline:**
```json
POST /api/briefs/
{
  "objs": [{
    "name": "contact-form",
    "title": "Contact Info",
    "units": [
      { "unit_key": "name-field",
        "subtype": { "kind": "input", "control_type": "text", "label_text": "Your Name" } }
    ],
    "outputs": [
      { "unit_key": "name-field", "value_slot_id": "slot-uuid", "source_kind": "input", "source_path": "/value" }
    ]
  }]
}
```

Units have no `unit_type` or `ordinal` (order follows array position); the
`subtype` discriminator is `kind`. Value-slot wiring lives in the brief-level
`outputs[]`, not inside each unit.

**2. Read the nested units via the parent URL (read-only):**
```
GET /api/briefs/{id}/units/
```

### Pattern for Associative Relationships

**Create parent with references to related objects:**
```json
POST /api/brief-groups/
{
  "objs": [{
    "name": "onboarding-group",
    "title": "Onboarding Forms",
    "brief_ids": ["brief-1-uuid", "brief-2-uuid"]
  }]
}
```

The associative table entries are created atomically. The response includes the resolved references:

```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "group-uuid",
    "name": "onboarding-group",
    "briefs": [
      { "id": "brief-1-uuid", "name": "form-1" },
      { "id": "brief-2-uuid", "name": "form-2" }
    ]
  }]
}
```

### Pattern for Hierarchical Owned Dependents

Some versioned objects include a deep ownership hierarchy created atomically. The Guide entity demonstrates this pattern: Guide → Chapters → Blocks → Tags, all submitted inline in a single POST.

**1. Create a guide with chapters and blocks:**
```json
POST /api/guides/
{
  "objs": [{
    "title": "Journey Builder Guide",
    "description": "How to build and configure journeys",
    "locale": "en",
    "author": "Platform Team",
    "classification": "workflow",
    "source_path": "docs/books/journey-builder-guide",
    "chapters": [
      {
        "title": "Introduction",
        "source_file": "journey-builder-guide/intro.md",
        "blocks": [
          {
            "directive_type": "prose",
            "content": "This guide walks you through building journeys...",
            "tags": ["universal"]
          }
        ]
      },
      {
        "title": "Getting Started",
        "source_file": "journey-builder-guide/getting-started.md",
        "blocks": [
          {
            "directive_type": "prose",
            "content": "Before you begin, ensure you have...",
            "tags": ["agent", "product"]
          }
        ]
      },
      {
        "title": "Setup Details",
        "source_file": "journey-builder-guide/getting-started/setup.md",
        "parent_index": 1,
        "blocks": [
          {
            "directive_type": "prose",
            "name": "Environment Setup",
            "content": "Configure your local environment...",
            "attributes": {},
            "tags": ["agent"]
          }
        ]
      }
    ]
  }]
}
```

**Key fields:**
- `title` — Unique across all guides; auto-generates a slug (e.g., `"journey-builder-guide"`)
- `chapters[].parent_index` — Zero-based index into the `chapters` array. Must reference a chapter that appears *before* this one (topological ordering). Omit for top-level chapters.
- `blocks[].directive_type` — Key from the `rev.guide_block_directive_types` lookup table
- `blocks[].tags` — Keys from the `rev.guide_tags` lookup table (audience targeting)

**2. Response includes the full hierarchy with assigned IDs:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "guide-version-uuid",
    "path_id": "guide-path-uuid",
    "title": "Journey Builder Guide",
    "slug": "journey-builder-guide",
    "version": { "major": 0, "minor": 0, "patch": 1 },
    "locale": "en",
    "chapters": [
      {
        "id": "chapter-1-uuid",
        "title": "Introduction",
        "source_file": "journey-builder-guide/intro.md",
        "parent_chapter_id": null,
        "blocks": [
          {
            "id": "block-1-uuid",
            "directive_type": "prose",
            "content": "This guide walks you through building journeys...",
            "tags": ["universal"]
          }
        ]
      },
      {
        "id": "chapter-2-uuid",
        "title": "Getting Started",
        "parent_chapter_id": null,
        "blocks": [...]
      },
      {
        "id": "chapter-3-uuid",
        "title": "Setup Details",
        "parent_chapter_id": "chapter-2-uuid",
        "blocks": [...]
      }
    ],
    "created_at": "2026-02-13T10:30:00Z"
  }]
}
```

**Note:** `parent_index` in the request becomes `parent_chapter_id` in the response. The server resolves the index to the actual chapter's `id`.

**Deprecation:** Guides support soft deprecation via DELETE:
```
DELETE /api/guides/?level=deprecated
{ "objs": [{ "id": "guide-path-uuid", "reason": "Superseded by v2" }] }
```

The `id` in the DELETE body is the **path** id, not the version id. This deprecates the entire version lineage.

---

## Querying Resources

### By Object IDs

Most efficient when you know exactly which resources you need:

```
GET /api/tasks/?query=by_object_ids&ids[0]=uuid-1&ids[1]=uuid-2&ids[2]=uuid-3
```

**Use when:**
- Fetching specific resources by known IDs
- Following references from parent objects
- Implementing "show details" views

### By Path IDs (Versioned Objects)

Get all versions of a versioned object:

```
GET /api/briefs/?query=by_path_ids&path_ids[0]=path-uuid
```

**Response includes all versions:**
```json
{
  "status": "success",
  "results": 3,
  "objs": [
    {
      "id": "v1-uuid",
      "path_id": "path-uuid",
      "version": { "major": 0, "minor": 0, "patch": 1 },
      ...
    },
    {
      "id": "v2-uuid",
      "path_id": "path-uuid",
      "version": { "major": 0, "minor": 0, "patch": 2 },
      ...
    },
    {
      "id": "v3-uuid",
      "path_id": "path-uuid",
      "version": { "major": 0, "minor": 0, "patch": 3 },
      ...
    }
  ]
}
```

**Use when:**
- Displaying version history
- Allowing users to select specific versions
- Auditing changes over time

### By Parent Relationship

Filter resources by their parent:

```
GET /api/dispatches/?query=by_journey&journey_id=uuid
```

**Use when:**
- Displaying all child resources of a parent
- Implementing "list all X for Y" views
- Filtering by relationship

### Pagination

Control result size and page:

```
GET /api/tasks/?query=by_object_ids&ids[0]=uuid-1&options={"limit":50,"page":1}
```

**Use when:**
- Displaying large result sets
- Implementing infinite scroll or pagination UI
- Reducing response payload size

---

## Data Consistency Patterns

### Ordering Guarantees

The API preserves ordering in batch operations. POST responses match the order of the request:

```json
POST /api/tasks/
{ "objs": [{ "name": "Task A" }, { "name": "Task B" }] }

Response:
{ "objs": [{ "name": "Task A", ... }, { "name": "Task B", ... }] }
```

This ordering is maintained in GET responses when querying by the same IDs:

```
GET /api/tasks/?query=by_object_ids&ids[0]=task-a-uuid&ids[1]=task-b-uuid
```

**Result:** Same order as the `ids` parameter.

### Atomicity

Batch operations are atomic. Either all objects are created, or none are.

```json
POST /api/tasks/
{
  "objs": [
    { "name": "Task 1" },
    { "name": "Task 2" },
    { "name": "Task 1" }  // Duplicate name - conflict
  ]
}
```

**Result:** 409 Conflict. No tasks are created.

### Referential Integrity

Foreign key constraints are enforced. You must create dependencies before referencing them:

```json
POST /api/steps/
{
  "objs": [{
    "name": "My Step",
    "tasks": [{ "task_id": "nonexistent-uuid" }]
  }]
}
```

**Result:** 404 Not Found. The step is not created.

---

## Common Patterns

### Create a Playbook Hierarchy

```
1. POST /api/tasks/       → Create tasks (principal objects)
2. POST /api/steps/       → Create steps (reference task IDs)
3. POST /api/playbooks/   → Create playbook (stages are inlined, reference step + task IDs)
```

See [Composing Steps & Stages](./composing-steps-stages.md) for details.

### Create Debrief Content

```
1. POST /api/value-slots/        → Define value types
2. POST /api/briefs/             → Create briefs with inline units + value-slot outputs
3. POST /api/brief-groups/       → Bundle briefs atomically
4. POST /api/brief-packs/        → Create journey-assignable packs
```

See [Brief Pack Manifest](./brief-pack-manifest.md) for details.

### Setup Documentation Content

Guides are independent of the execution hierarchy. They can be created at any time and are not referenced by Playbooks, Journeys, or Dispatches in this initial release.

```
1. POST /api/guides/    → Create guide with inline chapters, blocks, and tags
2. GET /api/guides/?query=by_slugs&slugs[0]=journey-builder-guide  → Fetch by slug
3. GET /api/guides/?query=by_path_ids&path_ids[0]=path-uuid        → Fetch version history
```

Guides support the journey-centric model as reference material. Because playbooks can be executed partially or out of order, guides provide the structured documentation that users and agents can consult at any point during a journey — not just at the beginning.

### Start a Journey

```
1. POST /api/journeys/               → Create journey: requires playbook_id + guide_id (the exact guide version you navigated to); optional brief_pack_ids and inference_profile_ids
2. GET /api/journeys/{id}/briefs     → Fetch all briefs for debrief UI
3. GET /api/briefs/{id}/units/       → Fetch UI elements
4. Render debrief UI                 → User completes BriefGroups
5. POST /api/dispatch-intent-bindings/ → Submit collected values (binds value slots)
6. POST /api/dispatches/             → Create a dispatch (flat steps[]); it auto-runs once dispatch_ready
7. GET /api/journeys/{id}/steps      → Poll per-step status to watch progress
```

> **`guide_id` is required.** The caller supplies the exact `rev.guides.id`
> version it navigated to (guide browse is canonical-pin-aware); the backend
> validates that the guide exposes `playbook_id` and never resolves "latest" or
> by path. Use `brief_pack_ids` and `inference_profile_ids` (plain plurals) to
> attach debrief content and model configuration.

**Journey Composition:** A Journey is composed of three independent, composable entities:
- **Playbook** - Workflow template defining tasks, steps, and stages
- **BriefPacks** - Debrief content to present to users
- **InferenceProfiles** - Model configuration for prompt execution

This composition allows the same Playbook to run with different models (via different InferenceProfiles) or different user input forms (via different BriefPacks).

See [Runtime Operations](./runtime-operations.md) for the complete flow.

---

## Error Handling

### Validation Errors (400)

```json
{
  "status": "fail",
  "message": "name: field required"
}
```

**Fix:** Check request schema, ensure all required fields are present.

### Unauthorized (401)

```json
{
  "status": "fail",
  "message": "Missing or invalid authorization token"
}
```

**Fix:** Include `Authorization: Bearer <jwt>` header with valid token.

### Forbidden (403)

```json
{
  "error": "SIGNER_NOT_AUTHORIZED",
  "message": "SIGNER_NOT_AUTHORIZED: address 0x... is not in the allowlist"
}
```

**Fix:** Contact admin to add signer to authorized list.

### Not Found (404)

```json
{
  "status": "fail",
  "message": "Resource not found"
}
```

**Fix:** Verify resource exists, check object IDs are correct.

### Conflict (409)

```json
{
  "status": "fail",
  "message": "duplicate key value violates unique constraint"
}
```

**Fix:** Use unique names, avoid duplicate references.

### Internal Server Error (500)

```json
{
  "status": "fail",
  "message": "Internal server error"
}
```

**Fix:** Check server logs, report issue to development team.

---

## Rate Limiting and Performance

### Batch Operations

Prefer batch operations over individual requests:

**Inefficient:**
```javascript
for (const task of tasks) {
  await POST('/api/tasks/', { objs: [task] });
}
```

**Efficient:**
```javascript
await POST('/api/tasks/', { objs: tasks });
```

### Efficient Fetching

Use specialized endpoints when available:

**Inefficient (multiple round-trips):**
```
GET /api/journeys/{id}             → Get journey
GET /api/brief-packs/{id}          → Get brief pack
GET /api/brief-groups/{id}         → Get brief groups
GET /api/briefs/{id}               → Get briefs
GET /api/briefs/{id}/units/        → Get units (per brief)
```

**Efficient (single request):**
```
GET /api/journeys/{id}/briefs      → Get all briefs with context
GET /api/briefs/{id}/units/        → Get units (per brief)
```

---

## Next Steps

- Read [Runtime Operations](./runtime-operations.md) for journey-centric workflows
- See [API Cookbook](./api-cookbook.md) for complete examples
- Check [Authentication](./authentication.md) for token management
