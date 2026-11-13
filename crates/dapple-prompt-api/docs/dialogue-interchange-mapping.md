# Dialogue Interchange to Backend Mapping Guide

This guide maps the Form DSL Renderer's Dialogue Interchange protocol to the dapple-prompt-api backend.

**Table of Contents**
- [Architecture Comparison](#architecture-comparison)
- [Concept Mapping](#concept-mapping)
- [Block Type Mapping](#block-type-mapping)
- [Endpoint Mapping](#endpoint-mapping)
- [What Changed](#what-changed)
- [Spec Changes Needed](#spec-changes-needed)
- [Not Yet Covered](#not-yet-covered)

---

## Architecture Comparison

### Spec Structure (Dialogue Interchange)
```
Envelope (turn)
  └── Sections (slides/pages)
        └── Blocks (content: markdown, form, notice, asset, chat, custom)

ClientReply
  └── responses[] (form data, custom payloads)
```

### Backend Structure (dapple-prompt-api)
```
Brief (versioned document)
  └── BriefUnits (atomic UI elements: markdown, notice, input, media, output)

BriefGroups → BriefPacks (organizational hierarchy)

Dispatch (runtime execution)
  └── DispatchIntentBindings (typed value bindings per ValueSlot)
```

### Key Architectural Difference

The spec uses a **composite block model** where a `FormBlock` contains a complete FormDSL schema. The backend uses a **decomposed model** where each form field is a separate `BriefUnit` with individual output mappings to `ValueSlot`s.

---

## Concept Mapping

| Spec Concept | Backend Equivalent | Notes |
|--------------|-------------------|-------|
| `Envelope` | `Brief` | Versioned document with units |
| `Envelope.id` | `Brief.object_id` | UUID identifier |
| `Envelope.createdAt` | `Brief.created_at` | ISO timestamp |
| `Envelope.role` | N/A | No role concept - briefs are templates |
| `Envelope.context.sessionId` | Journey + Dispatch | Session is modeled via Journey/Dispatch |
| `Envelope.context.threadId` | `Journey.object_id` | Thread = Journey |
| `Envelope.context.parentId` | Dispatch lineage | Via dispatch relationships |
| `Envelope.sections[]` | `BriefGroup` | Groups organize multiple briefs |
| `Section.blocks[]` | `BriefUnit[]` | Units are nested under briefs |
| `Section.layout` | `BriefUnit.layout` | Layout hints on each unit |
| `Block.requires` | `BriefUnit.requires` | Conditional visibility |
| `Block.extensions` | `BriefUnit.extensions` | Extension data |
| `ClientReply` | `DispatchIntentBinding[]` | One binding per ValueSlot |
| `ClientReply.context.journeyId` | `Dispatch.journey_id` | Links to journey |
| `ClientReply.idempotencyKey` | Internal | Handled by API |
| `branchingPolicy` | Re-dispatch | Steps/stages can be re-dispatched |

---

## Block Type Mapping

### MarkdownBlock → BriefUnit (markdown)

**Spec:**
```json
{
  "type": "markdown",
  "markdown": "# Hello",
  "format": "gfm",
  "formatOptions": {
    "deliverable": {
      "editButton": { "label": "Edit", "target": "..." },
      "downloadButton": [{ "label": "PDF", "target": "..." }]
    }
  }
}
```

**Backend:**
```json
{
  "unit_type": "markdown",
  "unit_key": "intro",
  "ordinal": 0,
  "subtype": {
    "kind": "markdown",
    "markdown_text": "# Hello",
    "format": "gfm",
    "deliverable_spec": {
      "editButton": { "label": "Edit", "target": "..." },
      "downloadButton": [{ "label": "PDF", "target": "..." }]
    }
  }
}
```

**Status:** Direct mapping. `deliverable_spec` captures the deliverable options.

---

### FormBlock → Multiple BriefUnits (input)

**Spec:**
```json
{
  "type": "form",
  "schema": {
    "fields": [
      { "key": "name", "type": "text", "label": "Name" },
      { "key": "age", "type": "number", "label": "Age" }
    ],
    "actions": { "submit": { "target": "/api/..." } }
  }
}
```

**Backend:** Forms are decomposed into individual units:

```json
// First, create ValueSlots for each field
POST /api/value-slots/
{
  "objs": [
    { "name": "name", "value_kind": "text", "title": "Name" },
    { "name": "age", "value_kind": "int", "title": "Age" }
  ]
}

// Then create BriefUnits referencing them
POST /api/briefs/{brief_id}/units/
{
  "objs": [
    {
      "unit_type": "input",
      "unit_key": "name",
      "ordinal": 0,
      "outputs": [{
        "value_slot_object_id": "slot-name-uuid",
        "source_kind": "input",
        "source_path": "/value"
      }],
      "subtype": {
        "kind": "input",
        "control_type": "text",
        "label_text": "Name"
      }
    },
    {
      "unit_type": "input",
      "unit_key": "age",
      "ordinal": 1,
      "outputs": [{
        "value_slot_object_id": "slot-age-uuid",
        "source_kind": "input",
        "source_path": "/value"
      }],
      "subtype": {
        "kind": "input",
        "control_type": "number",
        "label_text": "Age"
      }
    }
  ]
}
```

**Status:** Structural change. FormDSL must be decomposed during setup. Submit actions are handled via Dispatch flow.

---

### NoticeBlock → BriefUnit (notice)

**Spec:**
```json
{
  "type": "notice",
  "level": "warning",
  "title": "Warning",
  "detail": "Please review carefully"
}
```

**Backend:**
```json
{
  "unit_type": "notice",
  "unit_key": "warning-1",
  "ordinal": 0,
  "subtype": {
    "kind": "notice",
    "level": "warning",
    "title_text": "Warning",
    "detail_text": "Please review carefully"
  }
}
```

**Status:** Direct mapping. Also supports `fluent_message_object_id` for i18n.

---

### AssetBlock → BriefUnit (media)

**Spec:**
```json
{
  "type": "asset",
  "assets": [
    {
      "id": "img-1",
      "kind": "image",
      "url": "https://...",
      "alt": "Description"
    }
  ]
}
```

**Backend:**
```json
{
  "unit_type": "media",
  "unit_key": "gallery-1",
  "ordinal": 0,
  "subtype": {
    "kind": "media",
    "items": [{
      "ordinal": 0,
      "media_kind": "image",
      "source": { "type": "url", "url": "https://..." },
      "alt_text_fluent_message_object_id": "msg-uuid"
    }]
  }
}
```

**Mapping:**
| Spec Asset.kind | Backend media_kind |
|-----------------|-------------------|
| `image` | `image` |
| `video` | `video` |
| `audio` | `audio` |
| `file` | `document` or `file` |

**Status:** Mapping exists. Backend uses `source.type` discriminator (`asset` or `url`).

---

### ChatBlock → Work in Progress

**Spec:**
```json
{
  "type": "chat",
  "targetBlockId": "deliverable-1",
  "placeholder": "Ask a question..."
}
```

**Backend:** Work in progress. Will enable inline chat for modifying deliverable content.

---

### CustomBlock → Removed

The `CustomBlock` type is being removed from the spec. Use `extensions` on existing block types if custom metadata is needed.

---

## Endpoint Mapping

### Journey Start

**Spec:**
```
POST /api/journey/start
Body: { envelope, title?, notes?, branchingPolicy? }
Returns: { envelope, ... }
```

**Backend:** Split into two operations:

```
1. POST /api/journeys/
   Body: { "objs": [{ "name": "...", "playbook_object_id": "..." }] }

2. POST /api/dispatches/
   Body: { "objs": [{ "journey_id": "...", "intent": {...}, "stages": [...] }] }
```

The backend requires a pre-configured Playbook. The spec's inline envelope must be translated to pre-existing Briefs.

---

### Dialogue Reply (Form Submission)

**Spec:**
```
POST /api/journey/:id/dialogue/reply
Body: ClientReply { replyTo, responses: [{ type: "form", blockId, data }] }
Returns: next envelope
```

**Backend:**
```
POST /api/dispatch-intent-bindings/
Body: {
  "objs": [{
    "dispatch_object_id": "...",
    "value_slot_object_id": "...",
    "text_items": [{ "ordinal": 0, "value": "John" }],
    "int_items": [{ "ordinal": 0, "value": 28 }],
    ...
  }]
}
```

**Key difference:** The backend binds values per `ValueSlot` with typed arrays (`text_items`, `int_items`, etc.), not as a nested form data object.

**Translation required:** ClientReply.responses[].data must be flattened into individual bindings per field.

---

### Form Data Persistence

**Spec:**
```
GET /api/journey/:id/dialogue/forms/:envelopeId
POST /api/journey/:id/dialogue/forms/:envelopeId
DELETE /api/journey/:id/dialogue/forms/:envelopeId
```

**Backend:** Form data lives in `DispatchIntentBinding`. Query by dispatch:
```
GET /api/dispatch-intent-bindings/?query=by_dispatch&dispatch_id=...
```

---

### History

**Spec:**
```
GET /api/journey/:id/history
Returns: list of turns with envelopes
```

**Backend:**
```
GET /api/dispatches/?query=by_journey&journey_id=...  (if implemented)
```

The backend doesn't have a direct "history" endpoint - dispatches track execution state.

---

### Branching (Re-dispatch)

**Spec:**
```
POST /api/journey/:id/branch
Body: { parentId }
```

**Backend:** Branching is achieved via re-dispatching steps or stages within a Journey:

```
POST /api/dispatches/
Body: {
  "objs": [{
    "journey_id": "existing-journey-uuid",
    "stages": [/* subset of stages to re-run */],
    "steps": [/* subset of steps to re-run */]
  }]
}
```

The Journey can have multiple Dispatches, each representing a branch or re-execution from a prior point.

---

## What Changed

### Structural Changes

1. **Forms decomposed**: FormDSL schemas are broken into individual BriefUnits + ValueSlots at setup time, not embedded in envelopes.

2. **Typed value bindings**: ClientReply form data is stored as typed arrays (`text_items`, `int_items`, `number_items`, etc.) in DispatchIntentBinding, not as nested JSON.

3. **Setup vs Runtime split**: The spec mixes template definition with runtime. The backend separates:
   - **Setup (Admin)**: Brief → BriefUnits, ValueSlots, Playbooks
   - **Runtime**: Journey → Dispatch → DispatchIntentBindings

4. **No inline envelopes**: The spec allows creating envelopes on-the-fly. The backend requires pre-configured Briefs.

### Field Mappings

| Spec Field | Backend Field |
|------------|---------------|
| `Envelope.id` | `Brief.object_id` |
| `Block.id` | `BriefUnit.unit_key` |
| `Block.layout.colSpan` | `BriefUnit.layout.colSpan` |
| `Block.groupId` | `BriefUnit.layout.groupId` (in JSON) |
| `FormBlock.submit.targetUrl` | Handled via Dispatch flow |

---

## Spec Changes Needed

### 1. Add ValueSlot References

BriefUnits need `outputs[]` mapping to ValueSlots. The spec's FormDSL field keys should map to ValueSlot references:

```json
// Spec addition: outputs array on form fields
{
  "key": "name",
  "type": "text",
  "outputs": [{
    "valueSlotId": "uuid",
    "sourcePath": "/value"
  }]
}
```

### 2. Separate Setup from Runtime

Consider splitting the spec:
- **Template spec**: Defines briefs/units/value-slots (setup)
- **Dialogue spec**: Runtime envelope delivery using template references

### 3. Typed Value Submission

ClientReply should submit typed values, not nested objects:

```json
// Instead of:
{ "data": { "name": "John", "age": 28 } }

// Consider:
{
  "bindings": [
    { "valueSlotId": "name-slot", "textValue": "John" },
    { "valueSlotId": "age-slot", "intValue": 28 }
  ]
}
```

### 4. Add `path_object_id` for Versioning

Envelopes should expose version lineage:
```json
{
  "id": "version-uuid",
  "pathId": "lineage-uuid",
  "version": { "major": 0, "minor": 0, "patch": 1 }
}
```

### 5. Remove CustomBlock

The `CustomBlock` type should be removed from the spec. Use `extensions` on existing block types if custom data is needed.

---

## Work in Progress

### ChatBlock

**Status:** Work in progress.

The chat block for inline conversation within a deliverable is being developed. This will allow users to ask questions or request changes to content blocks.

---

## Clarifications

### Branching

The spec's branching concept maps to the backend's **re-dispatch capability**. Journeys can:
- Re-dispatch individual steps
- Re-dispatch entire stages
- Continue from any point in the workflow

This is handled via the Dispatch → TaskAttempt flow, not a separate "branch" endpoint.

---

### Deliverable Metadata

The spec's `formatOptions.deliverable` metadata (edit buttons, download buttons) is stored in `BriefUnit.subtype.deliverable_spec`.

The `DispatchIntentBinding.value_meta` field captures provenance and UI spec for product reporting:

```json
{
  "value_meta": [{
    "ordinal": 0,
    "ui_spec": { "widget": "text_input", "label": "Name" },
    "producer": "brief-unit-name-input"
  }]
}
```

This is used internally for analytics and reporting, not for client rendering.

---

### Internal Concerns (Not Part of Client API)

The following are handled internally by the API and don't require client implementation:

- **Session guardrails** (max branches, turns, age limits)
- **Idempotency** (deduplication of requests)
- **Flow ordering** (handling out-of-order replies)

---

## Summary

| Feature | Status |
|---------|--------|
| Markdown blocks | Implemented |
| Notice blocks | Implemented |
| Media/Asset blocks | Implemented |
| Form inputs (decomposed) | Implemented |
| Value bindings (typed) | Implemented |
| Journey + Dispatch flow | Implemented |
| Versioned briefs | Implemented |
| Branching (re-dispatch) | Implemented |
| Deliverable metadata | Implemented (via value_meta) |
| ChatBlock | Work in progress |
| CustomBlock | Removed from spec |
| Session guardrails | Internal |
| Idempotency | Internal |
| Flow ordering | Internal |
