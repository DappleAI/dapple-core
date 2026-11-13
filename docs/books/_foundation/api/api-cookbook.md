# API Cookbook

> **Audience:** Prompt Engineers · Product · Agents

This chapter provides complete, copy-paste-ready recipes for common API operations. Each recipe includes the full request/response cycle with real examples.

---

## Recipe Template

Each recipe follows this structure:

1. **Goal:** What to accomplish
2. **Preconditions:** What must exist before starting
3. **Steps:** API requests with expected responses
4. **Verification:** How to confirm success
5. **Troubleshooting:** Common issues and fixes

---

## Authentication Recipes

### Recipe: Obtain JWT with Ethereum-Signed Token

**Goal:** Exchange an Ethereum-signed auth token for a JWT to use with the API.

**Preconditions:**
- You have an Ethereum private key
- Your Ethereum address is in the `AUTH_TOKEN_ALLOWED_SIGNERS` allowlist
- You have a user UUID

**Steps:**

**1. Generate the auth token (Python example):**

```python
from web3 import Web3
from eth_account.messages import encode_defunct
import struct
import time
import base64

def generate_auth_token(private_key: str, valid_for_minutes: int = 5):
    """Generate an Ethereum-signed auth token."""
    w3 = Web3()
    account = w3.eth.account.from_key(private_key)

    # Build the message (64 bytes)
    version = 0x01
    valid_from = int(time.time())
    valid_until = valid_from + (valid_for_minutes * 60)

    message = struct.pack('>B', version)
    message += struct.pack('>Q', valid_from)
    message += struct.pack('>Q', valid_until)
    message += b'\x00' * 47

    # Sign using EIP-191 personal_sign
    signable_message = encode_defunct(message)
    signed = w3.eth.account.sign_message(signable_message, private_key=private_key)

    # Combine signature + message
    r = signed.r.to_bytes(32, 'big')
    s = signed.s.to_bytes(32, 'big')
    v = bytes([signed.v])

    token = r + s + v + message
    return base64.b64encode(token).decode()

# Generate token
private_key = "0x1234567890abcdef..."
auth_token = generate_auth_token(private_key, valid_for_minutes=5)
```

**2. Exchange for JWT:**

```bash
curl -X POST https://api.example.com/api/get-auth-token \
  -H "Content-Type: application/json" \
  -d '{
    "token": "'"$auth_token"'",
    "user_id": "550e8400-e29b-41d4-a716-446655440000"
  }'
```

**Response:**
```json
{
  "status": "ok",
  "jwt": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiI1NTBlODQwMC1lMjliLTQxZDQtYTcxNi00NDY2NTU0NDAwMDAiLCJzZXJ2aWNlX2lkIjoid2ViIiwidG9rZW5fdXNlIjoiZGVsZWdhdGVkIiwiYXV0aF91c2VyX2lkIjpudWxsLCJyb2xlcyI6WyJ1c2VyIl0sImV4cCI6MTcwNTMxMjIwMH0.abc123...",
  "signer": "0x742d35Cc6634C0532925a3b844Bc9e7595f4E3B0",
  "expires_at": "2024-01-15T11:30:00+00:00"
}
```

**3. Use JWT for subsequent requests:**

```bash
export JWT="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."

curl https://api.example.com/api/tasks/ \
  -H "Authorization: Bearer $JWT"
```

**Verification:**
```bash
# Should return tasks, not 401 error
curl https://api.example.com/api/tasks/ \
  -H "Authorization: Bearer $JWT"
```

**Troubleshooting:**

| Error | Fix |
|-------|-----|
| `SIGNER_NOT_AUTHORIZED` | Contact admin to add your address to allowlist |
| `EXPIRED` | Generate new token (reduce clock skew) |
| `INVALID_SIGNATURE` | Check private key, ensure correct signing |

---

### Recipe: Create and Use API Token

**Goal:** Create a long-lived API token for CLI or script use.

**Preconditions:**
- You have a valid web session JWT (logged in via NextAuth)

**Steps:**

**1. Create the API token:**

```bash
curl -X POST https://api.example.com/api/api-tokens/ \
  -H "Authorization: Bearer $WEB_SESSION_JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "CLI Integration",
      "description": "Token for local development",
      "expires_at": "2025-01-22T00:00:00Z"
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "token-uuid",
    "name": "CLI Integration",
    "description": "Token for local development",
    "token": "dpl_abc123def456...",
    "created_at": "2024-01-22T10:30:00Z",
    "expires_at": "2025-01-22T00:00:00Z",
    "revoked_at": null
  }]
}
```

**IMPORTANT:** Save the `token` field immediately. It won't be shown again.

**2. Store token securely:**

```bash
# Option 1: Environment variable
export DAPPLE_API_TOKEN="dpl_abc123def456..."

# Option 2: Password manager
pass insert dapple/api-token

# Option 3: Config file (secure permissions)
echo "dpl_abc123def456..." > ~/.dapple/token
chmod 600 ~/.dapple/token
```

**3. Use the token:**

```bash
curl https://api.example.com/api/tasks/ \
  -H "Authorization: Bearer dpl_abc123def456..."
```

**Verification:**

```bash
# List your tokens
curl https://api.example.com/api/api-tokens/ \
  -H "Authorization: Bearer $WEB_SESSION_JWT"
```

**Troubleshooting:**

| Issue | Fix |
|-------|-----|
| Token lost | Create new token and revoke old one |
| Token expired | Create new token with later expiration |
| 401 Unauthorized | Check token format, ensure not expired |

---

## Setup Recipes

### Recipe: Create a Complete Playbook

**Goal:** Build a playbook from scratch with tasks, steps, and stages.

**Preconditions:**
- Valid JWT token

**Steps:**

**1. Create Tasks:**

```bash
curl -X POST https://api.example.com/api/tasks/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [
      {
        "name": "collect-user-info",
        "description": "Collect basic user information",
        "function": "nushell_script",
        "function_input": {
          "script": "def main [] { { status: \"ok\" } }"
        }
      },
      {
        "name": "validate-info",
        "description": "Validate collected information",
        "function": "nushell_script",
        "function_input": {
          "script": "def main [] { { status: \"ok\" } }"
        }
      }
    ]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 2,
  "objs": [
    {
      "id": "task-1-uuid",
      "name": "collect-user-info",
      "created_at": "2024-01-22T10:30:00Z"
    },
    {
      "id": "task-2-uuid",
      "name": "validate-info",
      "created_at": "2024-01-22T10:30:00Z"
    }
  ]
}
```

**2. Create Steps (reference tasks):**

```bash
curl -X POST https://api.example.com/api/steps/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "user-setup",
      "description": "Complete user setup workflow",
      "tasks": [
        { "task_id": "task-1-uuid" },
        { "task_id": "task-2-uuid" }
      ]
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "step-uuid",
    "name": "user-setup",
    "tasks": [
      { "id": "step-task-1-uuid", "task": { "id": "task-1-uuid" } },
      { "id": "step-task-2-uuid", "task": { "id": "task-2-uuid" } }
    ],
    "created_at": "2024-01-22T10:30:05Z"
  }]
}
```

**3. Create Stage:**

```bash
curl -X POST https://api.example.com/api/stages/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "onboarding",
      "description": "User onboarding stage"
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "stage-uuid",
    "name": "onboarding",
    "created_at": "2024-01-22T10:30:10Z"
  }]
}
```

**4. Create Playbook:**

```bash
curl -X POST https://api.example.com/api/playbooks/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "user-onboarding-playbook",
      "description": "Complete user onboarding workflow",
      "stages": [
        {
          "steps": [
            {
              "step_id": "step-uuid",
              "params": {},
              "tasks": [
                { "task_id": "task-1-uuid" },
                { "task_id": "task-2-uuid" }
              ]
            }
          ]
        }
      ],
      "categories": []
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "playbook-uuid",
    "name": "user-onboarding-playbook",
    "stages": [...],
    "created_at": "2024-01-22T10:30:15Z"
  }]
}
```

**Verification:**

```bash
curl "https://api.example.com/api/playbooks/?query=by_object_ids&ids=[\"playbook-uuid\"]" \
  -H "Authorization: Bearer $JWT"
```

**Troubleshooting:**

| Error | Fix |
|-------|-----|
| 404 on task reference | Create tasks first, save their UUIDs |
| 409 Conflict | Use unique names for each resource |

---

### Recipe: Create Debrief Content (BriefPack)

**Goal:** Create a complete BriefPack with ValueSlots, Briefs, BriefUnits, and BriefGroups.

**Preconditions:**
- Valid JWT token

**Steps:**

**1. Create ValueSlots:**

```bash
curl -X POST https://api.example.com/api/value-slots/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [
      {
        "name": "user-full-name",
        "value_kind": "text",
        "title": "Full Name",
        "description": "User'\''s legal full name",
        "constraints": { "minLength": 2, "maxLength": 100 }
      },
      {
        "name": "user-email",
        "value_kind": "text",
        "title": "Email Address",
        "description": "Professional email contact",
        "constraints": { "pattern": "^[^@]+@[^@]+\\.[^@]+$" }
      }
    ]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 2,
  "objs": [
    {
      "id": "name-slot-uuid",
      "path_id": "name-slot-path-uuid",
      "name": "user-full-name",
      "version": { "major": 0, "minor": 0, "patch": 1 },
      "created_at": "2024-01-22T10:30:00Z"
    },
    {
      "id": "email-slot-uuid",
      "path_id": "email-slot-path-uuid",
      "name": "user-email",
      "version": { "major": 0, "minor": 0, "patch": 1 },
      "created_at": "2024-01-22T10:30:00Z"
    }
  ]
}
```

**2. Create a Brief (with units and outputs):**

Units and their value-slot output mappings are created **inline** with the brief.
There is no `POST /api/briefs/{id}/units/` endpoint — that path is read-only (see
the note after the response).

```bash
curl -X POST https://api.example.com/api/briefs/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "contact-info-form",
      "title": "Contact Information",
      "description": "Please provide your contact details",
      "units": [
        {
          "unit_key": "intro",
          "subtype": {
            "kind": "markdown",
            "markdown_text": "# Welcome!\n\nPlease provide your contact information below."
          }
        },
        {
          "unit_key": "name",
          "subtype": {
            "kind": "input",
            "control_type": "text",
            "label_text": "Full Name",
            "help_text": "Enter your legal name",
            "placeholder_text": "Jane Doe"
          }
        },
        {
          "unit_key": "email",
          "subtype": {
            "kind": "input",
            "control_type": "text",
            "label_text": "Email Address",
            "help_text": "Professional email",
            "placeholder_text": "jane.doe@example.com"
          }
        }
      ],
      "outputs": [
        { "unit_key": "name",  "value_slot_id": "name-slot-uuid",  "source_kind": "input", "source_path": "/value" },
        { "unit_key": "email", "value_slot_id": "email-slot-uuid", "source_kind": "input", "source_path": "/value" }
      ]
    }]
  }'
```

**Key fields:**
- `units[].subtype` is a discriminated union tagged by `kind` (`markdown` | `notice` | `input` | `media`). Markdown text goes in `markdown_text` (or reference a localized Fluent message with `fluent_message_id`). There is no `unit_type` or `ordinal` — unit order follows array position.
- `outputs[]` wires a unit to a value slot at the **brief** level (not inside the unit): `unit_key` ties it to the unit, `value_slot_id` is the slot it fills, `source_kind` is `input` | `files` | `computed`, and `source_path` is the JSON pointer into the submitted payload (e.g. `/value`).

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "brief-uuid",
    "path_id": "brief-path-uuid",
    "name": "contact-info-form",
    "version": { "major": 0, "minor": 0, "patch": 1 },
    "created_at": "2024-01-22T10:30:05Z"
  }]
}
```

> **Fetching units later:** `GET /api/briefs/{id}/units/` returns the brief's
> units as `BriefUnit` objects (`id`, `unit_key`, `layout`, `requires`,
> `extensions`, `subtype`, `created_at`). It is read-only — units are authored
> only via the brief create above.

**3. Create BriefGroup:**

```bash
curl -X POST https://api.example.com/api/brief-groups/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "contact-group",
      "title": "Contact Information",
      "brief_ids": ["brief-uuid"]
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "group-uuid",
    "path_id": "group-path-uuid",
    "name": "contact-group",
    "version": { "major": 0, "minor": 0, "patch": 1 },
    "briefs": [
      { "id": "brief-uuid", "name": "contact-info-form" }
    ],
    "created_at": "2024-01-22T10:30:15Z"
  }]
}
```

**4. Create BriefPack:**

```bash
curl -X POST https://api.example.com/api/brief-packs/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "onboarding-pack",
      "title": "Onboarding Information",
      "groups": [{ "id": "group-uuid" }]
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "pack-uuid",
    "path_id": "pack-path-uuid",
    "name": "onboarding-pack",
    "version": { "major": 0, "minor": 0, "patch": 1 },
    "groups": [
      { "id": "group-uuid", "name": "contact-group" }
    ],
    "created_at": "2024-01-22T10:30:20Z"
  }]
}
```

**Verification:**

```bash
curl "https://api.example.com/api/brief-packs/?query=by_object_ids&ids=[\"pack-uuid\"]" \
  -H "Authorization: Bearer $JWT"
```

---

### Recipe: Create an Inference Profile

**Goal:** Create an InferenceProfile that maps prompts to model configurations. Inference profiles are assigned to Journeys at creation time, making Playbooks model-agnostic.

**Preconditions:**
- Valid JWT token
- Prompt paths exist (created via `/api/prompts/`)

**Steps:**

**1. Create InferenceProfile (without configs):**

```bash
curl -X POST https://api.example.com/api/inference-profiles/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "production-claude",
      "description": "Claude-based model config for production prompts"
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "profile-uuid",
    "path_id": "profile-path-uuid",
    "name": "production-claude",
    "version": { "major": 0, "minor": 0, "patch": 1 },
    "description": "Claude-based model config for production prompts",
    "configs": [],
    "created_at": "2024-01-22T10:30:00Z"
  }]
}
```

**2. Create InferenceProfile with configs (maps prompts to models):**

```bash
curl -X POST https://api.example.com/api/inference-profiles/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "production-multi-model",
      "description": "Multi-model config with fallback priorities",
      "configs": [
        {
          "prompt_path_id": "prompt-path-uuid-1",
          "model_release_id": "model-release-sonnet-uuid",
          "model_variant_id": null,
          "priority": 1
        },
        {
          "prompt_path_id": "prompt-path-uuid-1",
          "model_release_id": "model-release-haiku-uuid",
          "model_variant_id": null,
          "priority": 2
        }
      ]
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "profile-2-uuid",
    "path_id": "profile-2-path-uuid",
    "name": "production-multi-model",
    "version": { "major": 0, "minor": 0, "patch": 1 },
    "description": "Multi-model config with fallback priorities",
    "configs": [
      {
        "prompt_path_id": "prompt-path-uuid-1",
        "model": {
          "type": "release",
          "lineage_key": "claude-3-5",
          "provider_key": "anthropic",
          "tier": "sonnet",
          "release": "20241022"
        },
        "priority": 1
      },
      {
        "prompt_path_id": "prompt-path-uuid-1",
        "model": {
          "type": "release",
          "lineage_key": "claude-3-5",
          "provider_key": "anthropic",
          "tier": "haiku",
          "release": "20241022"
        },
        "priority": 2
      }
    ],
    "created_at": "2024-01-22T10:30:05Z"
  }]
}
```

Each config entry maps a prompt (by `prompt_path_id`) to a model release or variant. The `priority` field controls fallback order — lower numbers are tried first. At runtime, Conduit selects the best available model based on priority, provider availability, and health status.

**3. Fetch profiles:**

```bash
curl "https://api.example.com/api/inference-profiles/?query=by_object_ids&ids[0]=profile-uuid" \
  -H "Authorization: Bearer $JWT"
```

**4. Assign to Journey:**

```bash
curl -X POST https://api.example.com/api/journeys/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "My Journey",
      "guide_id": "guide-uuid",
      "playbook_id": "playbook-uuid",
      "brief_pack_ids": ["pack-uuid"],
      "inference_profile_ids": ["profile-uuid"]
    }]
  }'
```

**Verification:**

```bash
curl "https://api.example.com/api/inference-profiles/?query=by_object_ids&ids[0]=profile-uuid" \
  -H "Authorization: Bearer $JWT"
```

Should return the profile with nested `configs` showing resolved model references.

**Troubleshooting:**

| Error | Fix |
|-------|-----|
| 409 Conflict | Use unique profile names |
| Empty `configs` in response | Configs are optional; add them in the POST request body |

---

## Production Flow Recipes

### Recipe: Complete Journey Flow (Simple One-Shot)

**Goal:** Execute a complete journey from creation to completion in one pass.

**Preconditions:**
- Guide exists (you hold its specific version `id`, obtained from guide browse)
- Playbook exists (and the guide exposes that playbook version)
- BriefPack exists
- InferenceProfile exists (optional - provides model configuration for prompts)
- Valid JWT token

**Steps:**

**1. Create Journey:**

A journey is version-pinned to a specific guide. Both `guide_id` and `playbook_id` are required, and the backend validates that the guide exposes that playbook version (a mismatch returns 400 `GUIDE_PLAYBOOK_MISMATCH`). You already hold the exact `guide_id` from guide browse, so the backend validates rather than resolving "latest."

```bash
curl -X POST https://api.example.com/api/journeys/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "User Onboarding Journey",
      "guide_id": "derived-guide-uuid",
      "playbook_id": "derived-playbook-uuid",
      "brief_pack_ids": ["pack-uuid"],
      "inference_profile_ids": ["profile-uuid"]
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "journey-uuid",
    "name": "User Onboarding Journey",
    "principal_id": "principal-uuid",
    "playbook_id": "derived-playbook-uuid",
    "guide_id": "derived-guide-uuid",
    "brief_packs": [{ "id": "pack-uuid" }],
    "inference_profiles": [{ "id": "profile-uuid" }],
    "created_at": "2024-01-22T10:30:00Z"
  }]
}
```

**Note:** The `inference_profile_ids` field is optional. When provided, it specifies which model configurations to use for prompt execution in this journey. See [Model Taxonomy](../../_foundation/concepts/model-taxonomy.md) for details.

**2. Fetch debrief content:**

```bash
curl "https://api.example.com/api/journeys/journey-uuid/briefs" \
  -H "Authorization: Bearer $JWT"
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "brief-uuid",
    "name": "contact-info-form",
    "context": {
      "brief_pack_id": "pack-uuid",
      "brief_group_id": "group-uuid",
      "brief_group_name": "contact-group",
      "brief_group_title": "Contact Information"
    }
  }]
}
```

**3. Fetch BriefUnits:**

```bash
curl "https://api.example.com/api/briefs/brief-uuid/units/" \
  -H "Authorization: Bearer $JWT"
```

**Response:** (Units as shown in previous recipes)

**4. User completes debrief (collect values in your UI)**

**5. Create Dispatch:**

A dispatch is a flat batch of one or more steps. There is no top-level `stages` array; stages are derived server-side from the steps you include. To run a whole stage, include all of its steps in the `steps` array.

```bash
curl -X POST https://api.example.com/api/dispatches/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "journey_id": "journey-uuid",
      "intent": { "user_goal": "Complete onboarding" },
      "steps": [
        { "stage_step_id": "stage-step-uuid", "params": {},
          "tasks": [ { "step_task_id": "step-task-uuid" } ] }
      ]
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "dispatch-uuid",
    "journey_id": "journey-uuid",
    "created_at": "2024-01-22T10:35:00Z"
  }]
}
```

**6. Store debrief results (submit value-slot bindings):**

```bash
curl -X POST https://api.example.com/api/dispatch-intent-bindings/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [
      {
        "dispatch_id": "dispatch-uuid",
        "value_slot_id": "name-slot-uuid",
        "text_items": [ { "value": "Jane Doe" } ],
        "value_meta": [ { "ui_spec": { "widget": "text_input" }, "producer": "name-input" } ]
      },
      {
        "dispatch_id": "dispatch-uuid",
        "value_slot_id": "email-slot-uuid",
        "text_items": [ { "value": "jane.doe@example.com" } ],
        "value_meta": [ { "ui_spec": { "widget": "text_input" }, "producer": "email-input" } ]
      }
    ]
  }'
```

**7. Execution starts automatically once all required slots are bound.**

You do not POST task attempts or events. Posting the bindings above (or creating the dispatch) auto-spawns execution as soon as the dispatch is `dispatch_ready`, meaning every overlay-exposed slot for the dispatch's steps is bound. Journey *completion* is a UI signal (all steps done, confetti), not an execution gate.

**8. Observe progress via the status endpoints:**

```bash
# Per-step exposed-slot status (which sub-interaction is awaiting a binding)
curl "https://api.example.com/api/journeys/journey-uuid/steps" \
  -H "Authorization: Bearer $JWT"

# Per-stage completion roll-up
curl "https://api.example.com/api/journeys/journey-uuid/stages" \
  -H "Authorization: Bearer $JWT"

# Dispatch status (task phases)
curl "https://api.example.com/api/dispatches/dispatch-uuid" \
  -H "Authorization: Bearer $JWT"
```

**Verification:**

`GET /api/dispatches/dispatch-uuid` should show its tasks reaching `status: "ok"`, and `GET /api/journeys/journey-uuid/stages` should report the stage as `done`.

---

### Recipe: Iterative Journey with Retry

**Goal:** Handle a task failure and recover by supplying the missing information. You never drive execution by hand. You observe failure through the status endpoints, fill the gap with another binding, and the dispatch re-readies and re-runs itself.

**Preconditions:**
- Active journey
- A dispatch whose task can fail due to missing data

**Steps:**

**1-6:** (Same as complete journey flow above: create journey, fetch debrief content, create dispatch, submit the initial bindings)

**7. Observe the failure.**

A task failure surfaces through the status endpoints, not a manual event. Poll the dispatch and the per-step status:

```bash
# A failed task shows up here as status: "failed"
curl "https://api.example.com/api/dispatches/dispatch-uuid" \
  -H "Authorization: Bearer $JWT"

# An unbound exposed slot shows up here (the step's bound: false slot)
curl "https://api.example.com/api/journeys/journey-uuid/steps" \
  -H "Authorization: Bearer $JWT"
```

The `/steps` response tells you which step is awaiting input and which `value_slot_id` is still unbound (for example, a phone-number slot the operator skipped).

**8. Present the matching BriefGroup for the missing value and collect it from the user.**

**9. Submit the additional binding:**

```bash
curl -X POST https://api.example.com/api/dispatch-intent-bindings/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [
      {
        "dispatch_id": "dispatch-uuid",
        "value_slot_id": "phone-slot-uuid",
        "text_items": [ { "value": "+1-555-123-4567" } ],
        "value_meta": [ { "ui_spec": { "widget": "text_input" }, "producer": "phone-input" } ]
      }
    ]
  }'
```

**10. The dispatch re-readies and re-executes automatically.**

Submitting the binding flips the dispatch's gate: once every overlay-exposed slot for its steps is bound, it becomes `dispatch_ready` again and execution re-spawns on its own. Bindings are latest-wins across dispatches, so the re-run picks up the new value. There is no manual "retry" call.

**Verification:**

```bash
curl "https://api.example.com/api/dispatches/dispatch-uuid" \
  -H "Authorization: Bearer $JWT"
```

The re-run should reach `status: "ok"`, and `GET /api/journeys/journey-uuid/stages` should report the stage as `done`.

---

## Versioning Recipes

### Recipe: Update a Versioned Object

**Goal:** Create a new version of a ValueSlot with updated constraints.

**Preconditions:**
- ValueSlot already exists

**Steps:**

**1. Get current version:**

```bash
curl "https://api.example.com/api/value-slots/?query=by_path_ids&path_ids=[\"slot-path-uuid\"]" \
  -H "Authorization: Bearer $JWT"
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "slot-v1-uuid",
    "path_id": "slot-path-uuid",
    "version": { "major": 0, "minor": 0, "patch": 1 },
    "name": "user-age",
    "constraints": { "min": 0, "max": 120 }
  }]
}
```

**2. Create new version (PATCH):**

```bash
curl -X PATCH https://api.example.com/api/value-slots/ \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "id": "slot-path-uuid",
      "title": "Age",
      "constraints": { "min": 0, "max": 150 }
    }]
  }'
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [{
    "id": "slot-v2-uuid",
    "path_id": "slot-path-uuid",
    "version": { "major": 0, "minor": 0, "patch": 2 },
    "name": "user-age",
    "constraints": { "min": 0, "max": 150 },
    "created_at": "2024-01-22T10:40:00Z"
  }]
}
```

**Verification:**

```bash
# Get all versions
curl "https://api.example.com/api/value-slots/?query=by_path_ids&path_ids=[\"slot-path-uuid\"]" \
  -H "Authorization: Bearer $JWT"
```

Should return both v1 and v2.

---

## Troubleshooting Common Scenarios

### Scenario: "Why can't I find my BriefGroups?"

**Problem:** Trying to fetch BriefGroups but getting 404 errors.

**Solution:** BriefGroups are accessed via their parent BriefPack:

```bash
# Don't do this (no such endpoint for filtering)
curl "https://api.example.com/api/brief-groups/?query=by_journey&journey_id=..." \
  -H "Authorization: Bearer $JWT"

# Do this instead
curl "https://api.example.com/api/journeys/journey-uuid/briefs" \
  -H "Authorization: Bearer $JWT"
# Then use context.brief_group_id to group briefs
```

---

### Scenario: "Dispatch fails with missing values"

**Problem:** Task execution fails because required ValueSlots are missing.

**Solution:**

1. Observe the failure via `GET /api/dispatches/{id}` (task `status: "failed"`) or `GET /api/journeys/{id}/steps` (an unbound exposed slot)
2. Identify which BriefGroup contains inputs for the missing slots
3. Present that BriefGroup to the user
4. POST new DispatchIntentBindings with the values
5. The dispatch re-readies and re-executes automatically once every required slot is bound. There is no manual retry call.

See "Recipe: Iterative Journey with Retry" above for the complete example.

---

### Scenario: "Can't create duplicate binding"

**Problem:** Getting 409 Conflict when creating DispatchIntentBinding.

**Solution:** Each `(dispatch_id, value_slot_id)` pair must be unique. Either:

- Check if binding already exists (GET bindings for dispatch)
- Use a different dispatch for the new value
- Update the existing binding (if PATCH is implemented)

---

## SDK Recipes

The Dapple SDK provides typed Rust methods for all API operations. These recipes parallel the HTTP examples above.

### Create a Journey (SDK)

```rust
use dapple_sdk::DappleClient;
use dapple_sdk::types::CreateJourneySchema;

let client = DappleClient::with_api_token("https://api.example.com", "dpl_token");

let journeys = client.journeys().create(vec![
    CreateJourneySchema {
        name: "My Journey".into(),
        guide_id,
        playbook_id,
        brief_pack_ids: vec![pack_id],
        inference_profile_ids: vec![profile_id],
    },
]).await?;
```

### Ingest a Guide (SDK)

```rust
use dapple_sdk::types::IngestParams;
use std::path::Path;

// Ingest from directory (requires "tarball" feature)
let response = client.guides().ingest_from_dir(
    &Path::new("./my-guide"),
    IngestParams { dry_run: false, source_path: Some("guides/my-guide".into()) },
).await?;

// Ingest from pre-built tarball
let bytes = std::fs::read("guide.tar.gz")?;
let response = client.guides().ingest(
    bytes,
    IngestParams { dry_run: true, source_path: None },
).await?;
```

### List Guides by Slug (SDK)

```rust
let guides = client.guides().list_by_slugs(
    vec!["build-resume".into()],
    None,
).await?;
```

### Render a Guide as PDF (SDK)

```rust
let pdf_bytes = client.guides().render_pdf(guide_id, "consumer").await?;
std::fs::write("guide.pdf", &pdf_bytes)?;
```

### Guide Favorites (SDK)

```rust
// Add to favorites
client.guides().favorite(guide_id).await?;

// Check if favorited
let check = client.guides().is_favorite(guide_id).await?;

// List all favorites
let favorites = client.guides().favorites().await?;
```

---

## Next Steps

- Read [Runtime Operations](./runtime-operations.md) for detailed flow explanations
- See [API Patterns & Reference](./api-patterns.md) for endpoint documentation
- Check [Authentication](./authentication.md) for token management strategies
- Review [Testing & Debugging](./testing-debugging.md) for troubleshooting workflows
