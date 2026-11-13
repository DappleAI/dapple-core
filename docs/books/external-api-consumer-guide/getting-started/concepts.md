# Core Concepts

> **Audience:** Customer Integration Engineers · Backend Developers · Agents

This chapter covers the vocabulary and runtime model you need to consume the external surface. The concepts are the same platform concepts as the internal surface — the differences are in *isolation* and *what you are allowed to do*, not in the shape of a journey.

## Per-Company Isolation (read this first)

Everything you do on the external surface is scoped to your company. This is not a convention you opt into; it is enforced at the query layer for every request.

- **Your identity is your company.** The JWT you exchange carries `sub = <your company UUID>`. Every handler derives `company_id` from that claim — never from a request body. You cannot name another company.
- **Scope is hard-`'external'`.** You see only content whose scope is `'external'` in your namespace. The dapple-internal tiers (`public`, `common`, `internal`) are not visible.
- **A `principal_id` is stamped on what you create.** Token exchange resolves a `principal_id` that the surface writes onto every journey and dispatch you create. It is your audit fingerprint.
- **No existence leak.** A foreign or unknown id returns `404 RESOURCE_NOT_FOUND` — the same body either way. You cannot tell "belongs to someone else" apart from "does not exist".
- **No "list all".** Every list endpoint is company-scoped. There is no platform-wide enumeration.

---

## Execution Model

{{#include ../../_foundation/concepts/core-execution-model.md}}

### Stages, steps, and generate → pick → refine

A **stage** is a static form or pane the operator lands on. Advancing from one stage to the next is the real navigation through a journey. A **step** is a dynamic, AJAXy sub-interaction *within* a stage that happens before the operator moves on. The canonical shape is **generate → pick → refine**: a task generates candidates, the operator picks one, and a later step refines the pick. Multi-step stages are the norm, not the exception. A single-step stage is just the degenerate case.

As a consumer you do not author this structure, but you do **observe** it and **drive** it through two endpoints at two granularities:

- **`GET /api/journeys/{id}/steps`** is the fine-grained "which sub-interaction is awaiting input" signal. Each entry carries the step's `done` flag plus its `exposed_slots` — every value slot the step exposes, each with a `bound` flag telling you whether it has been supplied. Poll this to discover which step (for example, the "pick" step in a generate → pick → refine flow) is waiting on a binding and which `value_slot_id` to set.
- **`GET /api/journeys/{id}/stages`** is the coarse per-stage roll-up. Each entry carries a `playbook_stage_id` and a `done` flag. A stage is `done` when every step it contains is done.

The typical client loop: poll `/steps`, find the step whose `exposed_slots` has an unbound slot, submit a binding via `POST /api/dispatch-intent-bindings` for that `value_slot_id`, then poll again. Execution kicks off automatically once every overlay-exposed slot a dispatch needs is bound — you never have to manually "start" a step.

### Automatic execution

The external surface has **no manual task path**. You do not POST a task attempt, and you do not post task events. Instead:

1. You create a dispatch (a batch of one-or-more steps).
2. You submit the value-slot bindings the dispatch's steps expose.
3. The moment the dispatch is `dispatch_ready` (every exposed slot is bound), the runtime spawns execution automatically.
4. You observe progress through the read-only status endpoints (`/journeys/{id}/steps`, `/journeys/{id}/stages`, `/dispatches/{id}`, `/dispatches/{id}/task_attempts`).

> **Journey completion is a UI-only signal, NOT a gate.** "All steps done" lights up a completion state for the operator, but it never controls whether a dispatch executes. The only gate is per-dispatch readiness (`dispatch_ready`).

---

## Briefs & Packs

{{#include ../../_foundation/concepts/briefs-and-packs.md}}

---

## Versioning and Pinning

A journey is **version-pinned**: it references an exact `guide_id` (a specific guide version) and an exact `playbook_id` (a specific playbook version). You supply both at create time — the backend validates the pair (the guide must expose that playbook version) and returns `400 GUIDE_PLAYBOOK_MISMATCH` on a mismatch. It does **not** resolve "latest" for you; you already hold the exact version you navigated to.

{{#include ../../_foundation/concepts/versioning-model.md:versioned-objects}}

{{#include ../../_foundation/concepts/versioning-model.md:version-lifecycle}}
