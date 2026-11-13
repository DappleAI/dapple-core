# Endpoint Reference

> **Audience:** Customer Integration Engineers · Backend Developers

The complete V1 external surface. This is the whole API a customer company can call — there are no hidden admin or configuration endpoints behind it.

## API Basics

| Property | Value |
|----------|-------|
| Base URL | `https://external-api.dapple.x/api/` |
| Listener port | `8081` |
| Content-Type | `application/json` |
| Authentication | `Authorization: Bearer <jwt>` (from `get-service-token`) |

Every authenticated request is scoped to your company. There is no "list all" variant of any endpoint, the visible scope is hard-`'external'`, and a foreign or unknown id returns `404 RESOURCE_NOT_FOUND` with no existence leak.

## The V1 Surface

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/get-service-token` | POST | Exchange an Ed25519-signed token for a JWT. The only unauthenticated endpoint. |
| `/api/guides` | GET | List your company's guides (`company_id` + `scope = 'external'`). |
| `/api/guides/ingest` | POST | Upload a tarballed guide (multipart, field name `file`). See [Guide Ingest](./guide-ingest.md). |
| `/api/guides/{path_id}/ingest` | PATCH | Re-ingest / upsert an existing guide. See [Guide Ingest](./guide-ingest.md). |
| `/api/journeys` | POST | Create a journey on your playbook. Requires `guide_id` + `playbook_id`. |
| `/api/journeys` | GET | List your journeys (pagination via `options`). |
| `/api/journeys/{id}` | GET | Fetch one journey. |
| `/api/journeys/{id}/briefs` | GET | All briefs for a journey. |
| `/api/journeys/{id}/stages` | GET | Per-stage completion roll-up. |
| `/api/journeys/{id}/steps` | GET | Per-step exposed-slot status. |
| `/api/dispatches` | POST | Create a dispatch on your journey. Auto-executes when `dispatch_ready`. |
| `/api/dispatches/{id}` | GET | Poll dispatch status. |
| `/api/dispatches/{id}/briefs` | GET | Per-dispatch brief state. |
| `/api/dispatches/{id}/task_attempts` | GET | Per-task runtime phase. **Read-only.** |
| `/api/dispatch-intent-bindings` | POST | Submit value-slot bindings. Triggers execution of each now-ready dispatch. |

> **No manual task path.** There is no POST to create a task attempt and no events write path. The `task_attempts` endpoint is strictly read-only — execution is automatic.

## Endpoint Details

### `POST /api/get-service-token`

Exchange a signed token for a JWT. See [Authentication](./getting-started/authentication.md).

**Request:**
```json
{ "token": "base64-encoded-128-byte-signed-token", "service_id": "your_company_prod_client" }
```

Returns a JWT signed with the external surface's key. `401` if the signature is invalid, the token is revoked, or the company is revoked. `503` if the surface's signing key is not configured.

### `GET /api/guides`

Lists your company's guides. Scoped to `company_id` and `scope = 'external'`. There is no unscoped variant.

### `POST /api/guides/ingest` and `PATCH /api/guides/{path_id}/ingest`

Upload or upsert a guide tarball. Full details in [Guide Ingest](./guide-ingest.md). A foreign or unknown `path_id` on PATCH returns `404 RESOURCE_NOT_FOUND`.

### `POST /api/journeys`

Create one or more journeys.

**Request:**
```json
{
  "objs": [
    {
      "name": "Resume Builder Session",
      "guide_id": "guide-version-uuid",
      "playbook_id": "playbook-version-uuid",
      "brief_pack_ids": ["briefpack-uuid"],
      "inference_profile_ids": ["profile-uuid"]
    }
  ]
}
```

| Field | Required | Notes |
|-------|----------|-------|
| `name` | yes | Display name for the session |
| `guide_id` | yes | The exact *guide version* this journey is pinned to |
| `playbook_id` | yes | The exact *playbook version* to run |
| `brief_pack_ids` | no | Debrief content to assign; auto-derived from the guide when empty |
| `inference_profile_ids` | no | Model configurations to use |

The backend validates that the guide exposes that playbook version. A mismatch returns `400 GUIDE_PLAYBOOK_MISMATCH`. A guide outside your namespace returns `404` (no leak). A matching-but-non-canonical pair is allowed (experiments, or staying on an older version deliberately).

**Response (Journey DTO):**
```json
{
  "status": "success",
  "results": 1,
  "objs": [
    {
      "id": "journey-uuid",
      "name": "Resume Builder Session",
      "principal_id": "principal-uuid",
      "playbook_id": "playbook-version-uuid",
      "guide_id": "guide-version-uuid",
      "brief_packs": [ { "id": "briefpack-uuid" } ],
      "inference_profiles": [ { "id": "profile-uuid" } ],
      "created_at": "2024-01-22T10:30:00Z"
    }
  ]
}
```

### `GET /api/journeys` and `GET /api/journeys/{id}`

List your journeys (with pagination via `options`) or fetch one. A foreign or unknown id returns the same `404 RESOURCE_NOT_FOUND` body as a non-existent id.

### `GET /api/journeys/{id}/briefs`

Returns all briefs for the journey, each with a `context` object carrying `brief_pack_id`, `brief_group_id`, and the group's name/title — group by `brief_group_id` to respect BriefGroup atomicity.

### `GET /api/journeys/{id}/steps`

The fine-grained "which step awaits which slot" signal. Each exposed slot is
**self-describing** — you can drive a whole generate→pick→refine wizard from this
endpoint alone, without a second join to `/brief-form`.

```json
{
  "status": "success",
  "objs": [
    {
      "stage_step_id": "stage-step-uuid",
      "playbook_stage_id": "stage-uuid",
      "stage_name": "Pull the Record",
      "stage_description": "Name the fund you want to examine.",
      "stage_kind": "intake",
      "stage_icon": ["radar", "scan-search", "id-card"],
      "done": false,
      "exposed_slots": [
        {
          "value_slot_id": "cuisine-slot-uuid",
          "name": "cuisine",
          "value_kind": "text",
          "bound": false,
          "producer": "operator",
          "input": {
            "control_type": "select",
            "label_text": "Cuisine",
            "choices": [
              { "value": "italian", "label": "Italian" },
              { "value": "thai", "label": "Thai" }
            ]
          }
        },
        {
          "value_slot_id": "synthesis-slot-uuid",
          "name": "synthesis",
          "value_kind": "json",
          "bound": false,
          "producer": "computed"
        }
      ]
    }
  ]
}
```

**`producer` is the field you branch on:**

- `"operator"` — *you* provide this value. Render the control from the `input`
  block (`control_type`, `label_text`, `help_text`, `placeholder_text`,
  `choices`) and POST a binding to `/api/dispatch-intent-bindings` with this
  `value_slot_id`. A `select`/`radio`/`checkbox` carries its `choices` inline —
  render the real control, never a raw text/JSON box.
- `"computed"` — the **server** produces this value after the producing step's
  task succeeds. There is no `input` block; show a waiting/progress state and
  **do not** try to bind it. Poll `bound` (and `/api/dispatches/{id}/task_attempts`
  → `computed_outputs`) to know when it lands and whether production failed.

`value_slot_id` is populated for **both** producers (the journey-pinned version).
Use `producer` as the discriminator — do **not** infer "computed" from a missing
id.

**Stage identity fields:** `stage_name` (the authored stage title),
`stage_description` (its resolved description), and the optional `stage_kind`
and `stage_icon` repeat identically on every row of a stage — fold
first-seen-wins. `stage_kind` is a closed, author-set vocabulary describing what
kind of pane the stage is: `intake`, `fetch`, `analyze`, `decide`, `review`,
`compare`, `assemble`, `deliver`. It is absent when the guide authored no kind;
treat an unknown or absent value as your neutral default — never an error, never
a blocker.

`stage_icon` is an **ordered array** of author-set icon names (up to five),
distinguishing sibling stages that share a kind. Its vocabulary is **open** and
belongs to whichever client draws it, so unlike `stage_kind` you should expect
names you do not recognize — that is the normal state, not skew. If you draw
icons, take one and fall back to your `stage_kind` glyph on anything unfamiliar;
if you do not, ignore the field entirely. Absent and `[]` mean the same thing.

### `GET /api/journeys/{id}/stages`

The coarse per-stage roll-up. A stage is `done` when all of its steps are.
Each entry carries the same stage identity as `/steps` (`name`, `description`,
optional `kind` and `icon`), with the same vocabularies and the same
absent-tolerance.

```json
{
  "status": "success",
  "objs": [
    {
      "playbook_stage_id": "stage-uuid",
      "name": "Pull the Record",
      "description": "Name the fund you want to examine.",
      "kind": "intake",
      "icon": ["radar", "scan-search", "id-card"],
      "done": false
    }
  ]
}
```

### `POST /api/dispatches`

Create a dispatch — a batch of one-or-more steps. There is no top-level `stages` array; submit a flat `steps[]` and the server groups them under their parent stage.

**Request:**
```json
{
  "objs": [
    {
      "journey_id": "journey-uuid",
      "intent": { "user_goal": "Create professional resume" },
      "steps": [
        {
          "stage_step_id": "stage-step-uuid",
          "params": {},
          "tasks": [ { "step_task_id": "step-task-uuid" } ]
        }
      ]
    }
  ]
}
```

To fire a whole stage in one dispatch, include all of that stage's steps in `steps[]`. The dispatch executes automatically once every value slot its steps expose is bound.

### `GET /api/dispatches/{id}`

Polls dispatch status. The response echoes the dispatch with the server-derived nesting (`stages` → `steps` → `tasks`), each task carrying a `status`.

### `GET /api/dispatches/{id}/briefs`

Per-dispatch brief state.

### `GET /api/dispatches/{id}/task_attempts`

Per-task runtime phase (Running / Failed / Ok). **Read-only.** This surface has no task-attempt write path.

Each attempt carries `status`, `context_data`, `error_detail`, the author-emitted
`signals`, and `computed_outputs` — per-output results for the computed
`:::brief-output`s the step produces:

```json
{
  "status": "success",
  "objs": [
    {
      "id": "attempt-uuid",
      "status": "ok",
      "signals": { "items_deduped": 3 },
      "computed_outputs": [
        { "value_slot_id": "synthesis-slot-uuid", "name": "synthesis", "outcome": "produced" }
      ],
      "error_detail": null
    }
  ]
}
```

`outcome` is one of `produced` (a binding was written — the `computed` slot on
`/steps` will now be `bound`), `source_path_missing` (the task ran but its payload
didn't contain the value — no binding written), `type_mismatch`, or
`unsupported_kind`. Use this to explain a stuck `computed` slot to your operator
instead of leaving the chapter in an unexplained pending state.

> **Authoring note for external-scope tasks.** The external surface drops the internal `context_metadata` and exposes only `context_data`, the author-emitted `signals` (projected from `context_metadata.signals`), and `computed_outputs` (runtime production results). `error_detail` is lifted from the failure event. Guide authors writing external-scope tasks are responsible for keeping internal-only data (file paths, internal table names, stack traces) out of `context_data` and author `signals` — the wire surface does not redact those.

### `POST /api/dispatch-intent-bindings`

Submit the values collected during the debrief. Each binding targets one `(dispatch_id, value_slot_id)` pair, which must be unique.

**Request:**
```json
{
  "objs": [
    {
      "dispatch_id": "dispatch-uuid",
      "value_slot_id": "name-slot-uuid",
      "text_items": [ { "value": "Jane Doe" } ],
      "value_meta": [
        { "ui_spec": { "widget": "text_input", "label": "Your Full Name" }, "producer": "name-input" }
      ]
    }
  ]
}
```

| Item array | Use for |
|------------|---------|
| `text_items` | freeform text — `{ "value": "…" }` |
| `int_items` | integers — `{ "value": 5 }` |
| `number_items` | floats — `{ "value": 99.99 }` |
| `bool_items` | flags — `{ "value": true }` |
| `json_items` | structured data — `{ "value": { … } }` |
| `asset_items` | uploaded files — `{ "asset_id": "asset-uuid" }` |

Items carry **no `ordinal`**. Omit the arrays you do not use. Submitting bindings that complete a dispatch's exposed slots triggers execution automatically.

## Response Formats

### Success (Single)

```json
{ "status": "success", "results": 1, "obj": { "id": "uuid", "...": "..." } }
```

### Success (Multiple)

```json
{ "status": "success", "results": 2, "objs": [ { "id": "uuid-1" }, { "id": "uuid-2" } ] }
```

### Error

```json
{ "status": "fail", "message": "Detailed error description" }
```

## HTTP Status Codes

| Code | Meaning | When it happens |
|------|---------|-----------------|
| 200 | OK | Successful GET; a content-hash-matching PATCH ingest (`outcome: "refreshed"`) |
| 201 | Created | Successful POST; a new-version PATCH ingest (`outcome: "new_version"`) |
| 400 | Bad Request | Validation error; `GUIDE_PLAYBOOK_MISMATCH` |
| 401 | Unauthorized | Missing/invalid/expired JWT; revoked token or company |
| 403 | Forbidden | Valid JWT presented to a disallowed operation |
| 404 | Not Found | `RESOURCE_NOT_FOUND` — also returned for foreign ids (no existence leak) |
| 500 | Internal Server Error | Unexpected server error |
| 503 | Service Unavailable | The external signing key is not configured |

See [Error Handling](./error-handling.md) for resolution detail and the no-leak rationale.
