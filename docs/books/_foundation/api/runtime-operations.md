# Runtime Operations

> **Audience:** Prompt Engineers · Product · Agents

This chapter covers journey-centric runtime operations for consuming the dapple-prompt API in production. Topics include starting journeys, fetching debrief content, collecting user input, executing workflows, and handling iterative flows.

---

## The Journey-Centric Model

Before diving into API calls, understand how users experience the system. This mental model is essential for building correct integrations.

### What Users Experience

Users interact with **Journeys**, not internal data structures. They don't see "Briefs" or "BriefPacks" - they experience a workflow:

```
1. Pick a Playbook     → User selects a workflow (like picking a recipe)
        ↓
2. Journey Created     → System creates Journey with assigned BriefPack(s)
        ↓
3. Debrief             → User completes an interview to gather information
        ↓
4. Dispatch            → System executes workflow steps
        ↓
5. Review Results      → User sees results, may need to provide more info
        ↓
6. Iterate             → Loop back to debrief/dispatch as needed
        ↓
7. Complete            → User enjoys results
```

**Key insight:** This is an iterative loop, not a linear one-shot process. Users may cycle through debrief → dispatch → review multiple times.

### Terminology

| Internal Term | What Users Experience | API Consumers Should Know |
|---------------|----------------------|---------------------------|
| Playbook | Workflow the user picks | Template for the entire process |
| Journey | User's session with a workflow | Central runtime object - start here |
| BriefPack | (Hidden from users) | Assigned to Journey, contains debrief content |
| BriefGroup | (Hidden from users) | Atomic unit - users must complete entire groups |
| Brief | (Hidden from users) | Container for UI elements |
| BriefUnit | (Hidden from users) | Individual content/input elements |
| InferenceProfile | (Hidden from users) | Assigned to Journey, configures which models to use |
| Debrief | The interview experience | Render BriefUnits as the user interface |
| Dispatch | Background execution | Workflow run triggered by debrief data |

### Journey Composition

A Journey is composed of three independent, composable entities:

```
Journey = Playbook + BriefPacks + InferenceProfiles
```

| Component | Purpose | Enables |
|-----------|---------|---------|
| **Playbook** | What tasks to run | Workflow logic reuse |
| **BriefPacks** | What forms users fill out | Content/UX variations |
| **InferenceProfiles** | Which models to use | Model A/B testing, provider flexibility |

This composition allows the same Playbook to run with different models (by swapping InferenceProfiles) or different user input experiences (by swapping BriefPacks).

### The Golden Rule: BriefGroup Atomicity

**BriefGroups must be completed atomically.** Product teams bundle related questions into BriefGroups to ensure they are always asked together. The UI must:
- Present all BriefUnits from a BriefGroup together
- Not allow partial BriefGroup submission
- Treat the group as a single "form" or "step"

---

> **SDK alternative:** All HTTP operations shown in this chapter have typed SDK equivalents. For example, `POST /api/journeys/` becomes `client.journeys().create(...)`. See [The Dapple SDK](../../journey-planner-guide/dapple-sdk.md) for the full client reference.

## Starting a Journey

> **Agent Note:** When creating journeys programmatically:
> - API tokens (prefixed with `dpl_`) are recommended for authentication. See [Authentication](./authentication.md).
> - The journey `id` is the primary reference for all subsequent operations.
> - Store both the `journey_id` and the assigned `brief_pack_ids` for the session.

When a user picks a playbook, create a Journey with assigned BriefPacks and InferenceProfiles.

### Step 1: Create the Journey

```json
POST /api/journeys/
Authorization: Bearer <jwt>
Content-Type: application/json

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

**Response:**
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
      "brief_packs": [
        { "id": "briefpack-uuid" }
      ],
      "inference_profiles": [
        { "id": "profile-uuid" }
      ],
      "created_at": "2024-01-22T10:30:00Z"
    }
  ]
}
```

**Key fields:**
- `guide_id` - **Required.** The specific *guide version* this journey is pinned to. You already hold this from guide browse (which is canonical-pin-aware).
- `playbook_id` - **Required.** The specific *playbook version* to run. The backend validates the pair (the guide must expose that playbook version) and returns 400 `GUIDE_PLAYBOOK_MISMATCH` on a mismatch. A matching-but-non-canonical pair is allowed (experiments or a consumer deliberately on an older version).
- `brief_pack_ids` - Which debrief content to assign (optional; auto-derived from the guide when empty, multiple for A/B testing).
- `inference_profile_ids` - Which model configurations to use for prompts (optional, enables model selection at runtime).
- `predecessor_journey_id` - Optional. For chained journeys; must share the predecessor's `guide_id`.
- `fork_stage_id` - Optional; requires `predecessor_journey_id`. A `playbook_stage` id of the *predecessor's* playbook naming a stage-level fork point: the new journey inherits only the predecessor's bindings produced by stages strictly *before* that stage ("fork at stage N" = redo N). Earlier panes arrive done; the fork stage onward starts clean. Omit it for whole-journey chaining (the predecessor's entire binding pool carries forward).

### Step 2: Store the Journey ID

Save `journey.id` for all subsequent operations. This is the central identifier for the user's session.

---

## The Debrief

After creating a journey, you need to render the debrief experience. This involves traversing from Journey to BriefUnits.

### Efficient Brief Retrieval (Recommended)

Use the convenience endpoint to get all briefs for a journey in a single request:

```
GET /api/journeys/journey-uuid/briefs
Authorization: Bearer <jwt>
```

**Response:**
```json
{
  "status": "success",
  "results": 2,
  "objs": [
    {
      "id": "brief-1-uuid",
      "path_id": "brief-1-path-uuid",
      "name": "personal-info",
      "content_hash": "…",
      "title": "Personal Information",
      "description": "Tell us about yourself",
      "created_at": "2024-01-15T10:30:00Z",
      "context": {
        "brief_pack_id": "pack-uuid",
        "brief_group_id": "group-1-uuid",
        "brief_group_name": "basic-info",
        "brief_group_title": "Basic Information"
      }
    },
    {
      "id": "brief-2-uuid",
      "path_id": "brief-2-path-uuid",
      "name": "work-history",
      "content_hash": "…",
      "title": "Work History",
      "description": "Your professional experience",
      "created_at": "2024-01-15T10:30:00Z",
      "context": {
        "brief_pack_id": "pack-uuid",
        "brief_group_id": "group-1-uuid",
        "brief_group_name": "basic-info",
        "brief_group_title": "Basic Information"
      }
    }
  ]
}
```

The `context` field enables you to:
- **Group briefs** by `brief_group_id` for atomic completion (BriefGroup rule)
- **Display group titles** using `brief_group_title`

### Fetch BriefUnits

For each brief, fetch its UI elements:

```
GET /api/briefs/brief-1-uuid/units/
Authorization: Bearer <jwt>
```

**Response:**
```json
{
  "status": "success",
  "results": 3,
  "objs": [
    {
      "id": "unit-1-uuid",
      "unit_key": "intro-text",
      "layout": {},
      "requires": {},
      "extensions": {},
      "subtype": {
        "kind": "markdown",
        "markdown_text": "# Welcome!\n\nPlease tell us about yourself."
      },
      "created_at": "2024-01-15T10:30:00Z"
    },
    {
      "id": "unit-2-uuid",
      "unit_key": "name-input",
      "layout": {},
      "requires": {},
      "extensions": {},
      "subtype": {
        "kind": "input",
        "control_type": "text",
        "label_text": "Your Full Name",
        "help_text": "Enter your legal name as it should appear on the resume",
        "placeholder_text": "Jane Doe",
        "control_spec": {},
        "choices": []
      },
      "created_at": "2024-01-15T10:30:00Z"
    },
    {
      "id": "unit-3-uuid",
      "unit_key": "email-input",
      "layout": {},
      "requires": {},
      "extensions": {},
      "subtype": {
        "kind": "input",
        "control_type": "text",
        "label_text": "Email Address",
        "help_text": "Professional email for contact",
        "placeholder_text": "jane.doe@example.com",
        "control_spec": {},
        "choices": []
      },
      "created_at": "2024-01-15T10:30:00Z"
    }
  ]
}
```

> **Unit → value-slot mapping.** A `BriefUnit` response carries no `outputs` —
> the unit→slot wiring is defined once at brief-create time via the brief's
> `outputs[]` (`{ unit_key, value_slot_id, source_kind, source_path }`). At
> runtime you don't need it: `GET /api/journeys/{id}/steps` returns each step's
> `exposed_slots[].value_slot_id` — the slot to bind for that sub-interaction.

### Render the Debrief UI

With the BriefUnits, render your debrief interface:

| Unit Type | How to Render |
|-----------|---------------|
| `markdown` | Display as formatted content (use markdown renderer) |
| `input` | Render as form control based on `control_type` |
| `output` | Display computed/retrieved values |
| `notice` | Show alerts or important messages |
| `media` | Display images, videos, or files |

**Example UI pseudocode:**
```javascript
// Group briefs by BriefGroup for atomic handling
const briefsByGroup = groupBy(briefs, b => b.context.brief_group_id);

// Render each group as a separate form section/screen
for (const [groupId, groupBriefs] of Object.entries(briefsByGroup)) {
  const groupTitle = groupBriefs[0].context.brief_group_title;

  renderFormSection({
    title: groupTitle,
    briefs: groupBriefs.map(brief => ({
      title: brief.title,
      description: brief.description,
      units: fetchUnits(brief.id) // Fetch units for this brief
    }))
  });
}
```

> **Form section/screen ≠ playbook step.** A debrief form-screen (a BriefGroup) is not the same as a playbook **step** — a step is a generate→pick→refine sub-interaction within a stage, surfaced via `GET /api/journeys/{id}/steps`.

**Remember the atomicity rule:** Present all BriefUnits from a single BriefGroup together. Don't allow users to skip between groups.

---

## Collecting Debrief Results

When users complete a BriefGroup, create DispatchIntentBindings to store their input.

### Step 1: Create a Dispatch

A Dispatch is the runtime execution of part of a playbook. **A dispatch is a batch of one-or-more steps.**

```json
POST /api/dispatches/
Authorization: Bearer <jwt>
Content-Type: application/json

{
  "objs": [
    {
      "journey_id": "journey-uuid",
      "intent": { "user_goal": "Create professional resume" },
      "steps": [
        {
          "stage_step_id": "stage-step-uuid",
          "params": {},
          "tasks": [
            { "step_task_id": "step-task-uuid" }
          ]
        }
      ]
    }
  ]
}
```

> **A dispatch is a batch of one-or-more steps.** There is no top-level `stages` array — submit steps; the server groups them under their parent stage. To run a whole stage in one dispatch, include all of that stage's steps in `steps[]`.

**Key fields:**
- `journey_id` - Links to the user's journey
- `intent` - JSON object for user context (freeform, used by tasks)
- `steps` - The flat list of steps to execute. Stage grouping is derived server-side.

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [
    {
      "id": "dispatch-uuid",
      "journey_id": "journey-uuid",
      "intent": { "user_goal": "Create professional resume" },
      "stages": [ /* derived stages → steps → tasks */ ],
      "created_at": "2024-01-22T10:35:00Z"
    }
  ]
}
```

The response echoes the dispatch with the server-derived `stages` nesting (each stage holds its steps, each step holds its tasks).

### Step 2: Create Intent Bindings

DispatchIntentBindings store the values collected during the debrief.

**Item types:**

| Item Type | Use Case | Example |
|-----------|----------|---------|
| `text_items` | Names, descriptions, freeform text | "Jane Doe", "Senior Engineer" |
| `int_items` | Counts, ages, quantities | 5, 28, 100 |
| `number_items` | Prices, percentages, measurements | 99.99, 85.5, 3.14 |
| `bool_items` | Flags, toggles, yes/no | true, false |
| `json_items` | Complex structured data | `{ "value": {"skills": ["Python", "Rust"]} }` |
| `asset_items` | References to uploaded files | `{ "asset_id": "asset-uuid" }` |

Each item in `text_items`/`int_items`/`number_items`/`bool_items`/`json_items` is `{ "value": … }`; each `asset_items` entry is `{ "asset_id": "…" }`. Omit empty arrays.

**Request:**
```json
POST /api/dispatch-intent-bindings/
Authorization: Bearer <jwt>
Content-Type: application/json

{
  "objs": [
    {
      "dispatch_id": "dispatch-uuid",
      "value_slot_id": "name-slot-uuid",
      "text_items": [ { "value": "Jane Doe" } ],
      "value_meta": [
        {
          "ui_spec": { "widget": "text_input", "label": "Your Full Name" },
          "producer": "name-input"
        }
      ]
    },
    {
      "dispatch_id": "dispatch-uuid",
      "value_slot_id": "email-slot-uuid",
      "text_items": [ { "value": "jane.doe@example.com" } ],
      "value_meta": [
        {
          "ui_spec": { "widget": "text_input", "label": "Email Address" },
          "producer": "email-input"
        }
      ]
    }
  ]
}
```

**Key fields:**
- `value_slot_id` - The slot defining the value type (from Setup)
- `{type}_items` - Use the appropriate type for your slot's `value_kind` (omit the arrays you don't use)
- `value_meta` - Tracks provenance - which UI element produced this value

**Constraint:** Each `(dispatch_id, value_slot_id)` pair must be unique.

> **Posting bindings auto-triggers execution.** When the bindings you submit make a dispatch `dispatch_ready` (every value slot the dispatch's steps *expose* is now bound), the runtime spawns execution automatically. You do not POST a task attempt or drive tasks by hand — you observe progress via the status endpoints below.

**Response:**
```json
{
  "status": "success",
  "results": 2,
  "objs": [
    {
      "id": "binding-1-uuid",
      "dispatch_id": "dispatch-uuid",
      "value_slot_id": "name-slot-uuid",
      "text_items": [{ "value": "Jane Doe" }],
      ...
    },
    {
      "id": "binding-2-uuid",
      "dispatch_id": "dispatch-uuid",
      "value_slot_id": "email-slot-uuid",
      "text_items": [{ "value": "jane.doe@example.com" }],
      ...
    }
  ]
}
```

---

## Executing the Workflow (automatic)

You do not drive task execution. The runtime spawns it for you.

When a dispatch becomes `dispatch_ready` — every value slot the dispatch's steps *expose* (their overlays) is bound — execution starts automatically. Posting `dispatch-intent-bindings` (or creating the dispatch when its slots are already satisfied) is what flips that gate and kicks off the run.

> **Journey completion is a UI-only signal, NOT a gate.** "All steps done" lights up a completion state (confetti) for the user, but it never controls whether a dispatch executes. The only gate is per-dispatch readiness (`dispatch_ready`).

Consumers do not POST task attempts or post events. The task-attempt/events write path that earlier versions of this doc described is gone — instead, you **observe** progress through the read-only status endpoints below.

### Tracking progress

Poll these endpoints to see what the runtime is doing and what (if anything) is still waiting on input:

**Per-step exposed-slot status** — the fine-grained "which step is awaiting which slot" signal. This is the runtime expression of generate → pick → refine:

```
GET /api/journeys/journey-uuid/steps
Authorization: Bearer <jwt>
```

```json
{
  "status": "success",
  "objs": [
    {
      "stage_step_id": "stage-step-uuid",
      "playbook_stage_id": "stage-uuid",
      "done": false,
      "exposed_slots": [
        { "value_slot_id": "name-slot-uuid", "name": "full-name", "value_kind": "text", "bound": true },
        { "value_slot_id": "summary-slot-uuid", "name": "summary", "value_kind": "text", "bound": false }
      ]
    }
  ]
}
```

**Per-stage roll-up** — a stage is `done` when all of its steps are:

```
GET /api/journeys/journey-uuid/stages
Authorization: Bearer <jwt>
```

```json
{
  "status": "success",
  "objs": [
    { "playbook_stage_id": "stage-uuid", "done": false }
  ]
}
```

**Dispatch status** — the dispatch with its server-derived nesting (`stages` → `steps` → `tasks`); each task carries a `status`:

```
GET /api/dispatches/dispatch-uuid
Authorization: Bearer <jwt>
```

---

## Review & Iterate

After dispatch execution, users review results and may need to provide more information.

### Check Dispatch Status

```
GET /api/dispatches/dispatch-uuid
Authorization: Bearer <jwt>
```

**Response includes all nested stages, steps, and tasks with their current status:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [
    {
      "id": "dispatch-uuid",
      "journey_id": "journey-uuid",
      "intent": { "user_goal": "Create professional resume" },
      "stages": [
        {
          "id": "dispatch-stage-uuid",
          "steps": [
            {
              "id": "dispatch-step-uuid",
              "tasks": [
                {
                  "id": "dispatch-task-uuid",
                  "status": "ok"
                }
              ]
            }
          ]
        }
      ],
      "created_at": "2024-01-22T10:35:00Z"
    }
  ]
}
```

Each task's `status` reflects its current runtime phase: the non-terminal `queued` and `running`, and the terminal `ok`, `failed` (retryable via re-dispatch), `canceled` (deliberately stopped), and `unsafe` (a task vetoed the step via `signals.unsafe = true` — do not refire). A step's derived verdict is worst-wins across its task attempts (any `unsafe` → unsafe; else any `failed` → failed; else `ok`), and because a step's tasks fail together the step stops at the first failed task. There is no `latest_attempt` field — read `status` directly.

### Handle Incomplete Results

If a step is still waiting on input (its `exposed_slots` show `bound: false` in `GET /api/journeys/{id}/steps`), the user needs to supply the missing value:

1. **Identify which ValueSlots are unbound** - the `exposed_slots` array on `/journeys/{id}/steps` tells you exactly which slot each step is waiting on.
2. **Present the relevant BriefGroups again** - same journey, render the briefs that produce those slots.
3. **POST the missing DispatchIntentBindings** - adding the binding flips the dispatch back to `dispatch_ready`, which re-spawns execution automatically. Latest-binding-wins: a later binding for the same slot supersedes the earlier one across dispatches.

### Continue or Complete

Based on results:
- **More debrief needed** - Loop back to present additional BriefGroups
- **Partial dispatch** - Execute additional steps with new bindings
- **Complete** - All steps successful, journey is done

---

## How Many Steps to Batch Per Dispatch

A dispatch is a batch of one-or-more steps. Your only real decision is **how many steps you put in a single dispatch's `steps[]`** — and a dispatch only runs once every value slot its steps *expose* is bound. So "how much to debrief before dispatching" is really "how many steps to batch, and whether you've collected the bindings those steps need."

### Step-Driven (Recommended)

Batch one step (or the smallest useful set) per dispatch, collect just its exposed slots, repeat:

```
1. Present the BriefGroup(s) that produce the next step's exposed slots
2. User completes them
3. Create a dispatch whose steps[] holds just that step
4. POST the bindings for that step's exposed slots → dispatch becomes ready → auto-executes
5. Poll /journeys/{id}/steps to see it flip to done
6. Repeat for the next step
```

**Advantages:**
- Better UX (user sees progress immediately)
- Allows early validation and feedback
- Reduces wasted work if user abandons midway

### Whole-Stage / Full Debrief

Batch a whole stage's steps (or every step) into one dispatch's `steps[]`:

```
1. Present all the BriefGroups for the stage (or the whole journey)
2. User completes them
3. Create one dispatch whose steps[] holds all of that stage's steps
4. POST all the bindings those steps expose → dispatch becomes ready → auto-executes
5. Poll /journeys/{id}/stages for the per-stage roll-up
```

**Advantages:**
- Simpler implementation
- User has complete picture before committing
- Fewer API round-trips

### User Choice

Let users decide how much to complete before proceeding — batch as many steps as they've supplied bindings for:

```
1. Present all BriefGroups
2. User completes some of them
3. User clicks "Continue" or "Run"
4. Create a dispatch whose steps[] holds the steps whose exposed slots are now satisfiable
5. POST the bindings → ready steps auto-execute
6. Present results and the remaining BriefGroups
7. Repeat
```

**Advantages:**
- Maximum flexibility
- User controls pace
- Supports exploratory workflows

The API supports all patterns. Choose based on your use case and UX goals.

---

## Manual Traversal (Alternative)

If you need more control than the convenience endpoint provides, you can manually traverse the hierarchy:

```
Journey.brief_packs → BriefPack.groups → BriefGroup.briefs → Brief.units → BriefUnit
```

### Step 1: Fetch BriefPacks for the Journey

```
GET /api/brief-packs/?query=by_object_ids&ids=["briefpack-uuid"]
Authorization: Bearer <jwt>
```

**Response:**
```json
{
  "status": "success",
  "results": 1,
  "objs": [
    {
      "id": "briefpack-uuid",
      "path_id": "pack-path-uuid",
      "name": "resume-pack",
      "version": { "major": 0, "minor": 0, "patch": 1 },
      "groups": [
        { "id": "group-1-uuid" },
        { "id": "group-2-uuid" }
      ]
    }
  ]
}
```

### Step 2: Fetch BriefGroups

```
GET /api/brief-groups/?query=by_object_ids&ids=["group-1-uuid","group-2-uuid"]
Authorization: Bearer <jwt>
```

**Response includes nested briefs:**
```json
{
  "status": "success",
  "results": 2,
  "objs": [
    {
      "id": "group-1-uuid",
      "path_id": "group-1-path-uuid",
      "name": "basic-info",
      "title": "Basic Information",
      "version": { "major": 0, "minor": 0, "patch": 1 },
      "briefs": [
        { "id": "brief-1-uuid", "name": "personal-info" },
        { "id": "brief-2-uuid", "name": "work-history" }
      ]
    },
    {
      "id": "group-2-uuid",
      "name": "education",
      "title": "Education History",
      "briefs": [
        { "id": "brief-3-uuid", "name": "degrees" }
      ]
    }
  ]
}
```

### Step 3: Fetch BriefUnits

For each brief, fetch units:

```
GET /api/briefs/brief-1-uuid/units/
Authorization: Bearer <jwt>
```

(Response same as shown in "Fetch BriefUnits" section above)

---

## Common Patterns

### Pattern 1: Simple One-Shot Journey

```
1. POST /api/journeys/                    → Create journey (guide_id + playbook_id)
2. GET /api/journeys/{id}/briefs          → Fetch all briefs
3. Render all BriefGroups as a single form
4. User completes entire form
5. POST /api/dispatches/                  → Batch all steps into one dispatch
6. POST /api/dispatch-intent-bindings/    → Store all debrief results (auto-executes when ready)
7. GET /api/journeys/{id}/steps + /stages → Poll progress
8. GET /api/dispatches/{id}               → Read results
9. Display results to user
```

### Pattern 2: Step-by-Step Journey

```
1. POST /api/journeys/                    → Create journey (guide_id + playbook_id)
2. GET /api/journeys/{id}/briefs          → Fetch all briefs
3. Group briefs by BriefGroup
4. FOR EACH BriefGroup:
   a. Render BriefGroup as a form section/screen
   b. User completes group
   c. POST /api/dispatches/               → Dispatch the next step
   d. POST /api/dispatch-intent-bindings/ → Store group's results (auto-executes when ready)
   e. GET /api/journeys/{id}/steps        → Poll until the step is done
   f. Display immediate results
5. All steps done → Journey shows complete (UI signal)
```

### Pattern 3: Iterative with Missing Input

```
1. POST /api/journeys/                    → Create journey (guide_id + playbook_id)
2. GET /api/journeys/{id}/briefs          → Fetch briefs
3. Render first BriefGroup
4. User completes group
5. POST /api/dispatches/                  → Dispatch the step
6. POST /api/dispatch-intent-bindings/    → Store results (auto-executes when ready)
7. GET /api/journeys/{id}/steps           → Step shows an exposed_slot with bound: false
8. Identify the unbound value_slot_id
9. Present the BriefGroup that produces that slot
10. User completes the missing group
11. POST /api/dispatch-intent-bindings/   → Add the missing binding → dispatch re-readies → re-executes
12. GET /api/journeys/{id}/steps + /dispatches/{id} → Step flips to done; read results
13. Display results
```

---

## Best Practices

### Do's

- **Group briefs by BriefGroup** - Always render atomically
- **Use the convenience endpoint** - `/api/journeys/{id}/briefs` is more efficient
- **Store journey ID persistently** - Needed for all subsequent operations
- **Poll the status endpoints** - `/journeys/{id}/steps` and `/journeys/{id}/stages` tell you what the runtime is doing and which slots are still unbound
- **Handle missing input gracefully** - Read `exposed_slots[].bound` to find unbound ValueSlots, then collect them
- **Support iteration** - Allow users to add bindings; the dispatch re-readies and re-executes (latest-binding-wins)

### Don'ts

- **Don't allow partial BriefGroup completion** - This violates the atomicity rule
- **Don't expose "Brief" terminology to users** - Use "form", "section", or "interview"
- **Don't drive task execution by hand** - There is no task-attempt/events write path; execution auto-spawns when a dispatch is `dispatch_ready`
- **Don't treat journey completion as a gate** - It is a UI-only signal; the only execution gate is per-dispatch readiness
- **Don't create multiple journeys for one user session** - Reuse the same journey
- **Don't ignore `context.brief_group_id`** - Essential for grouping
- **Don't assume linear flow** - Support review → re-debrief → re-dispatch loops

---

## Troubleshooting

### "BriefGroup not found"
- **Cause:** Trying to fetch a BriefGroup that doesn't exist
- **Fix:** Ensure you're using `brief_group_id` from the briefs response, not inventing IDs

### "Duplicate binding"
- **Cause:** Trying to create multiple bindings for same `(dispatch_id, value_slot_id)` pair
- **Fix:** Check if binding already exists, or use a different dispatch for new values (latest-binding-wins resolves across dispatches)

### "Dispatch never executes"
- **Cause:** The dispatch isn't `dispatch_ready` — one or more value slots its steps expose are still unbound
- **Fix:** Read `GET /api/journeys/{id}/steps`, find the `exposed_slots` with `bound: false`, and POST the missing bindings. Execution auto-spawns once every exposed slot is bound.

### `GUIDE_PLAYBOOK_MISMATCH` (400)
- **Cause:** The `guide_id` and `playbook_id` you passed are inconsistent — the guide version does not expose that playbook version
- **Fix:** Pass the exact pair you got from guide browse. A matching-but-non-canonical pair is fine; an inconsistent pair is rejected.

### "Journey has no BriefPacks"
- **Cause:** Created journey without assigning `brief_pack_ids` and the guide had none to auto-derive
- **Fix:** Include at least one BriefPack when creating the journey, or ensure the pinned guide exposes one

---

## Guide Operations (SDK)

If you are a Rust consumer, the `dapple-sdk` crate provides typed methods for guide operations alongside the HTTP endpoints described above.

### List Guides

```rust
use dapple_sdk::types::FilterOptions;

// List all guides (with pagination)
let guides = client.guides().list(Some(FilterOptions { limit: 20, page: 0 })).await?;
```

### Fetch a Guide by Slug

```rust
let guides = client.guides().list_by_slugs(
    vec!["journey-builder-guide".into()],
    None,
).await?;
```

### Fetch Guide Cards

```rust
let cards = client.guides().cards(Some(FilterOptions { limit: 50, page: 0 })).await?;
```

See [The Dapple SDK](../../journey-planner-guide/dapple-sdk.md) for the full client reference including journey creation, dispatch management, and brief retrieval.

---

## Next Steps

- Read [API Cookbook](./api-cookbook.md) for complete code examples
- See [API Patterns & Reference](./api-patterns.md) for endpoint details
- Check [Authentication](./authentication.md) for token management
- Review [Testing & Debugging](./testing-debugging.md) for troubleshooting workflows
