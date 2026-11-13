# Handoff — Image-Guide Assets on the External Surface (external-api + external-sdk)

> **Audience:** external-api / dapple-sdk (external client) maintainers.
> **TL;DR:** The **internal** SDK needs nothing. The **external** surface cannot
> serve a rendered image at all today, so external customers can't run
> image-generating guides end to end. This is **not blocking** the current
> Wimmelbook guide (it lives in the dapple namespace, internal), but it is real
> work the moment image guides are offered to customers.

## Internal SDK: no work (for the record)

`dapple-sdk`'s internal `DappleClient` already covers everything an
image-generating guide needs:
- `assets().content(id) -> bytes` (`crates/dapple-sdk/src/resource/assets.rs:74`,
  `GET /api/assets/{id}/content`) — fetch the rendered image bytes.
- `journeys()`, `steps()`, `stages()`, `dispatches()`,
  `dispatch_intent_bindings()`, `dispatch_task_attempts()` — the whole runtime
  flow, unchanged.

dapple-ui / dapple-suite consume image guides through this with no SDK change.
The only internal work is UX (see `handoff-dapple-ui-image-generating-guides.md`).

## External surface: the gap

`dapple-prompt-external-api` (`lib.rs::build_router`) nests
`guides`, `journeys`, `playbooks`, `stages`, `steps`, `tasks`, `categories`,
`prompts`, `directives`, `value-slots`, `fluent-messages`, `dispatches`,
`dispatch-intent-bindings`, task-attempts, and briefs — **but no `/api/assets`
and no `/content`.** And `ExternalDappleClient` (`dapple-sdk`) has no `assets()`
accessor (only the internal `DappleClient` does).

Consequence: a customer could build and dispatch an image guide, but the
deliverable is an **asset UUID** with no external endpoint to fetch its bytes.
The hunt-image never reaches the customer.

## What real external work looks like (when image guides go external)

1. **external-api: serve asset content, namespace-gated.** Add
   `GET /api/assets/{id}/content` (and likely `GET /api/assets/{id}` metadata) to
   the external router, served by a new `handler/assets.rs`. It must follow the
   surface's hard invariant: **filter by `caller.company_id`** — a foreign/unknown
   asset returns the standard 404 (no existence leak), exactly like
   `journey_in_namespace` / `guide_in_namespace`. Reuse the internal streaming
   logic (`read_asset_content` / `read_asset_range`, Range support) but behind the
   namespace check. New data-layer helper needed: `asset_in_namespace(asset_id,
   company_id)` (mirrors the existing `*_in_namespace` checks).

2. **Asset namespacing must line up (design wrinkle — verify before building).**
   Runtime-rendered images are persisted by the conduit
   (`persist_image_asset` → `insert_drafts`/`finalize_draft`). Today that asset
   path runs **outside** the per-transaction GUC namespace override (the same V1
   limitation already documented for customer guide-ingest asset uploads in
   `external-api/handler/guide_ingest.rs`), so a customer-rendered image would
   land in the **dapple** namespace. If the external asset endpoint gates by
   `company_id`, the customer would then be blocked from their own render. So
   shipping external image guides requires **either** stamping the caller's
   namespace onto runtime-created image assets **or** a deliberate
   per-company-scoped asset-access rule. Settle this first; it's the crux, not the
   endpoint.

3. **external-sdk: add an external assets client.** A
   `ExternalAssetsClient` with `content(id) -> bytes` on `ExternalDappleClient`,
   mirroring the internal `AssetsClient` but hitting the external base URL with
   the external JWT. Small once the endpoint exists.

4. **Scope tier.** External asset access must respect `scope = 'external'`
   (customer-private), consistent with the rest of the external surface.

## Recommendation

Do **not** build this preemptively. Track it as the explicit prerequisite for
"image-generating guides for customers." When that lands on the roadmap, item (2)
(asset namespacing) is the design decision to make before any endpoint/SDK code.
Until then: internal-only, and the internal SDK is ready.
