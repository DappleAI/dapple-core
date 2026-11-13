# Agent Reading Guide — External API Consumer Guide

## What this book is for

This book documents how a **customer company** (a third party) consumes the dapple-prompt **external** REST surface served on `external-api.dapple.x` by the `dapple-prompt-external-api` crate. It covers token exchange, guide listing and ingest, journeys, dispatches, value-slot bindings, and runtime status polling — all from the perspective of a customer integrating from outside dapple.

Read this if you are calling `external-api.dapple.x/api/...` as a customer. This is **not** the book for dapple-internal services.

> **Surface scope:** this book covers the **external/customer** surface only. dapple-owned internal services (dapple-ui, conduit, flow) use the **internal** surface (`api.dapple.x`, served by `dapple-prompt-api`) documented in [`../api-consumer-guide/`](../api-consumer-guide/AGENTS.md). The two surfaces share no JWT material: a token minted on one cannot be verified on the other (different signing keys). They also expose different endpoint sets — the external surface is deliberately narrower.

## Read this first if…

- …you need to authenticate a client → [`getting-started/authentication.md`](./getting-started/authentication.md) (Ed25519 signed token → JWT exchange).
- …you want the vocabulary (journey, stage, step, dispatch, generate → pick → refine) → [`getting-started/concepts.md`](./getting-started/concepts.md).
- …you need the full V1 endpoint list → [`endpoint-reference.md`](./endpoint-reference.md).
- …you are uploading or re-ingesting a guide tarball → [`guide-ingest.md`](./guide-ingest.md).
- …you are running a journey end to end → [`runtime-operations.md`](./runtime-operations.md).
- …a request is failing and you need to interpret the response → [`error-handling.md`](./error-handling.md).

## Surface posture you must already understand

These properties are non-negotiable invariants of the external surface. If any of them surprise you, read [`getting-started/concepts.md`](./getting-started/concepts.md) and [`getting-started/authentication.md`](./getting-started/authentication.md) before continuing.

- **Cryptographic isolation.** The external surface (`iss = external-api.dapple.x`, `token_use = external_service`) signs JWTs with its own key. A JWT from the internal surface cannot be verified here, and vice versa — verification fails at signature check before any claim is read.
- **Per-company data isolation.** Every authenticated request resolves to a `company_id` from the JWT's `sub` claim. Every query filters by that `company_id`, and the visible scope is hard-`'external'`. There is no code path that returns another company's data.
- **No existence leak.** A foreign or unknown id returns `404 RESOURCE_NOT_FOUND` — the same body whether the id belongs to another company or does not exist at all. You cannot probe for the existence of other companies' resources.
- **No "list all" endpoints.** Every list endpoint is scoped to your company. The surface never enumerates the whole platform.
- **Automatic execution.** You never POST a task attempt or drive tasks by hand. Creating a dispatch, or submitting the bindings it needs, triggers execution automatically once the dispatch is `dispatch_ready`. The `task_attempts` endpoint is **read-only** here.
- **Journey completion is a UI signal, not a gate.** The only execution gate is per-dispatch readiness (`dispatch_ready`). "All steps done" is decoration for the operator.

## Concepts you must already understand

- **Journey-centric vocabulary.** Users experience *Playbook → Journey → Debrief → Dispatch*. The API uses this vocabulary in URLs and payloads.
- **Stages are panes; steps are the interactions on them.** A stage is a static form/pane the operator advances between; steps are the AJAXy sub-interactions on a pane (generate → pick → refine). Multi-step stages are the norm. Poll `/journeys/{id}/steps` to see which step awaits which value slot.
- **A dispatch is a batch of one-or-more steps.** Its wire shape is a flat `steps[]` (each with `stage_step_id` and `tasks[].step_task_id`) — there is no top-level `stages` array on the request. To fire a whole stage in one dispatch, include all of that stage's steps.
- **Versioned objects are version-pinned.** A journey pins an exact `guide_id` (guide version) and `playbook_id` (playbook version). You supply both at create time; the backend validates the pair, it does not resolve "latest".
- **Field naming is plain.** `id`, `playbook_id`, `journey_id`, `guide_id`, `value_slot_id`, `dispatch_id`, `brief_pack_ids`, `inference_profile_ids`. There is no `object_id` field name on this surface.

## Quick navigation

| Topic | File | Key information |
|-------|------|-----------------|
| Authentication | [`getting-started/authentication.md`](./getting-started/authentication.md) | Ed25519 → JWT exchange, claims, expiry, onboarding |
| Core concepts | [`getting-started/concepts.md`](./getting-started/concepts.md) | Journey vocabulary, generate → pick → refine, isolation |
| Endpoint reference | [`endpoint-reference.md`](./endpoint-reference.md) | Full V1 table, scoping, 404 semantics, read-only task_attempts |
| Guide ingest | [`guide-ingest.md`](./guide-ingest.md) | POST + PATCH upsert, multipart `file`, namespace gating |
| Runtime flow | [`runtime-operations.md`](./runtime-operations.md) | Create journey → bind → auto-execute → poll status |
| Error handling | [`error-handling.md`](./error-handling.md) | 401/403/404 no-leak, 400 GUIDE_PLAYBOOK_MISMATCH, 503 |

## Cross-book pointers

| You want to… | Go to |
|--------------|-------|
| Consume the **internal** surface (`api.dapple.x`) | [`../api-consumer-guide/AGENTS.md`](../api-consumer-guide/AGENTS.md) |
| Author a Guide (directives, Fluent files, guide.toml) | [`../journey-planner-guide/AGENTS.md`](../journey-planner-guide/AGENTS.md) |
| Understand the surface internally (handlers, isolation invariants) | [`../../../crates/dapple-prompt-external-api/AGENTS.md`](../../../crates/dapple-prompt-external-api/AGENTS.md) |
| Learn the external auth model in depth | [`../_foundation/authentication/external-service-tokens.md`](../_foundation/authentication/external-service-tokens.md) |
| Look up platform terminology | [`../_foundation/glossary.md`](../_foundation/glossary.md) |
