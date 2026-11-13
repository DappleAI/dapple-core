# Core Concepts

> **Audience:** Content Authors · SDK Consumers · Frontend Developers · Agents

This chapter covers the core concepts you need to understand when working with the dapple-prompt API.

## Guide-Centric Architecture

The dapple-prompt platform follows a **guide-centric** design: most entities are derived automatically from structured guide directories rather than created individually via API calls.

```
Guide Directory (markdown + TOML + Fluent)
    │
    ▼ ingest
Platform Entities:
    ├── Guide (chapters, blocks, tags)
    ├── Playbook (stages, steps)
    ├── Tasks (nushell scripts)
    ├── Brief Packs (debrief content)
    ├── Inference Profiles (model config)
    ├── Fluent Messages (localized text)
    └── Assets (images, documents)
```

**Guides are the primary authoring path.** Content authors write guide directories with directive blocks (`:::stage`, `:::step`, `:::nushell`, etc.) and the ingest pipeline derives all the entities needed for a working workflow.

**Direct API creation is the advanced path.** You can still create tasks, steps, stages, and playbooks directly via `POST /api/{resource}/`, but this is intended for automation and edge cases where guide authoring doesn't apply.

For guide authoring details, see the [Journey Planner Guide - Guide Directory Structure](../../journey-planner-guide/guide-directory-structure.md). For the ingest API, see [Guide Ingest API](../ingest-api.md).

---

## Execution Model

{{#include ../../_foundation/concepts/core-execution-model.md}}

### Stages, steps, and generate → pick → refine

A **stage** is a static form or pane the user lands on. Advancing from one stage to the next is the real navigation through a journey. A **step** is a dynamic, AJAXy sub-interaction *within* a stage that happens before the user moves on. The canonical shape is **generate → pick → refine**: a task generates candidates, the operator picks one, and a later step refines the pick. Multi-step stages are the norm, not the exception. A single-step stage is just the degenerate case.

As a consumer, you don't author this structure, but you do need to **observe** it and **drive** it. Two endpoints expose runtime state at the two granularities you care about:

- **`GET /api/journeys/{id}/steps`** is the fine-grained "which sub-interaction is awaiting input" signal. Each entry carries the step's `done` flag plus its `exposed_slots` — every value slot the step exposes, each with a `bound` flag and a `producer` discriminator: `"operator"` slots carry an `input` render spec (`control_type`, label/help/placeholder, `choices`) and you POST a binding for them; `"computed"` slots are produced by the server once the step's tasks complete and the step coheres, so you show a waiting state and never bind them. Poll this to discover which step (for example, the "pick" step in a generate → pick → refine flow) is waiting on a binding and which `value_slot_id` to set.
- **`GET /api/journeys/{id}/stages`** is the coarse per-stage roll-up. Each entry carries a `playbook_stage_id` and a `done` flag. A stage is `done` when every step it contains is done.

The typical client loop: poll `/steps`, find the step whose `exposed_slots` has an unbound slot, submit a binding via `POST /api/dispatch-intent-bindings/` for that `value_slot_id`, then poll again. Execution kicks off automatically once every overlay-exposed slot a dispatch needs is bound, so you never have to manually "start" a step.

For how authors decide which slots a step exposes (the overlay system behind these endpoints), see [Value-Slot Overlays](../../journey-planner-guide/value-slot-overlays.md).

---

## Briefs & Packs

{{#include ../../_foundation/concepts/briefs-and-packs.md}}

---

## Versioning

{{#include ../../_foundation/concepts/versioning-model.md:versioned-objects}}

{{#include ../../_foundation/concepts/versioning-model.md:version-lifecycle}}

{{#include ../../_foundation/concepts/versioning-model.md:querying-versions}}
