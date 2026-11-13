# External API — Operator-Runner Endpoint & SDK Extensions

**Status:** Draft
**Created:** 2026-05-15
**Last Updated:** 2026-05-15
**Target Version:** 0.x (additive — no schema changes, no breaking changes)
**Dependencies:**
- [`00-architecture.md`](./00-architecture.md) — locks the surface posture, namespace-gating model, and cryptographic-isolation guarantees this plan operates within.
- `crates/dapple-prompt-external-api/AGENTS.md` — V1 Endpoint Surface, Mandatory Invariants, Anti-patterns.
- `crates/dapple-sdk/src/resource/external_journeys.rs` — sets the "absent by construction; added per-method as deliberate decisions" precedent for narrow external SDK clients.

## Decisions (resolved)

The questions raised during drafting have been answered. Each is recorded below with the chosen answer plus a one-line rationale; the rest of the plan reflects these choices throughout.

1. **Singular vs plural sub-resource path for task attempts → plural.**
   The external endpoint ships as `GET /api/dispatches/{id}/task_attempts`. The internal singular form (`/dispatch/{id}/task_attempt/`) is untouched. The two surfaces already diverge in envelope wrappers and error shapes; the path mismatch is acceptable and the plural form matches the response envelope key.

2. **`error_detail` content on the external surface → pass through verbatim, with an authoring contract.**
   `DispatchTaskAttempt.error_detail` is surfaced unchanged. Guide authors writing external-scope tasks are responsible for ensuring `metadata.signals.error` is safe to display to a third-party operator. This authoring contract is documented as an external-surface invariant in the AGENTS.md update list (§8.1).

3. **Auto-spawn execution from `POST /api/dispatch-intent-bindings` → no separate kick endpoint.**
   The existing handler re-checks brief state and spawns in-process execution for any dispatch that is now fully bound. The bindings POST *is* the advance action. No additional endpoint is required.

4. **External task_attempts DTO shape → pass through unchanged.**
   No narrow projection. The endpoint returns the internal `DispatchTaskAttempt` DTO (re-exported as `dapple_data::model::dispatch_task_attempts::DispatchTaskAttempt`, which itself re-exports `dapple_types::model::dispatch_task_attempts::DispatchTaskAttempt`). The full `context_data` / `context_metadata` blobs are surfaced verbatim — same authoring contract as `error_detail`: tasks written for external scope must not put internal-only data in their payloads. This drops one planned new file (the `ExternalDispatchTaskAttempt` projection) and one `From` impl.

5. **`GET /api/journeys/{id}` and `GET /api/journeys` (list) on the external surface → in scope for V1.**
   Both are designed in this plan (§4.7, §4.8). Rationale: an operator runner may need to resync after process restart (single-journey GET) and a runner-side "your open dispatches" view benefits from journey listing. Both endpoints filter by `caller.company_id` via the existing helpers (single uses `journey_in_namespace`; list uses a new `select_journeys_by_owner_company_id` SQL — see §4.8.3 and the implementation flag in §10).

---

## 1. Executive Summary

The external API surface needs three additional read endpoints to drive an interactive dispatch runner from a customer-side service whose operators have no dapple identity:

1. `GET /api/dispatches/{id}/task_attempts` — per-task runtime phase (Running / Failed / Ok) for runner phase derivation. The immediate consumer (acme's operator suite) cannot render a useful runner UI without it.
2. `GET /api/journeys/{id}` — single-journey fetch, namespace-gated. Needed when a runner process restarts and must resync against a known journey UUID.
3. `GET /api/journeys` — list the caller's journeys, namespace-gated. Needed to surface "your open dapple dispatches" cross-customer views and to support runner-side discovery beyond the single journey just created.

All three reuse the existing namespace-gating pattern (`journey_in_namespace` / `dispatch_in_namespace` + `caller.company_id` filter). The new `task_attempts` and single-journey endpoints reuse existing `dapple-data` query functions — no new SQL. The list-journeys endpoint requires one new SQL file (`select_journeys_by_owner_company_id`) and one new public query function — small, mirroring the existing `select_journeys_by_user` shape with the join target swapped from `user_details` to `companies`.

Responses pass through the internal `DispatchTaskAttempt` and `Journey` DTOs unchanged. No narrow external projections; per the resolved decisions, the authoring contract carries the safety guarantee for content fields (`error_detail`, `context_data`, `context_metadata`).

The plan also adds SDK methods that wrap these endpoints (plus the three existing endpoints not yet covered: `POST /api/journeys`, `POST/GET /api/dispatches`, `POST /api/dispatch-intent-bindings`) in a per-method-justified shape consistent with the existing narrow-by-construction precedent set by `ExternalJourneysClient`.

Audit conclusion: every other endpoint a runner needs is already present. Playbooks are discoverable via `Guide.playbooks` on `GET /api/guides` (no separate `/api/playbooks` recommended for V1); per-journey briefs and per-dispatch briefs are already exposed; single-dispatch get is already exposed; the bindings POST auto-spawns execution. No further endpoints are recommended for V1.

## 2. Context and Background

**The consumer.** acme's operator suite authenticates each operator to `acme-api` (acme-side identity). Operators have **no dapple-side user record**, so the only auth surface that can serve them is `external-api.dapple.x` — every request the operator makes is proxied through `acme-api`, signed with acme's external service token, and gated by `caller.company_id = acme's UUID`.

**What "interactive dispatch" means here.** An operator runs through a playbook step-by-step. Each step:
1. Operator picks a playbook, `POST /api/journeys` creates the journey.
2. Operator opens the first step, `POST /api/dispatches` creates a dispatch for it.
3. Renderer pulls `GET /api/dispatches/{id}` (status), `GET /api/dispatches/{id}/briefs` (form schema + binding state), and the new `GET /api/dispatches/{id}/task_attempts` (per-task runtime phase: Running / Failed / Ok).
4. Operator submits inputs via `POST /api/dispatch-intent-bindings`. The handler auto-detects "all inputs bound" and spawns execution.
5. Renderer polls back through (3) until terminal phase.

Resync scenarios — process restart, a runner UI reopening — additionally need:
- `GET /api/journeys/{id}` to confirm a known journey is still the caller's.
- `GET /api/journeys` to list all of the caller's journeys when no specific UUID is held.

**Why this matters.** Without `task_attempts`, the runner has only dispatch-level status (the dispatch as a whole) and brief-level state (per-slot binding). It cannot tell "task 2 of 3 in this step is currently running" or "task 1 failed with message X" — both are needed to render a useful runner UI. Without the journey GET/list endpoints, runners cannot recover cleanly from process restart and cannot surface "your open dispatches" views without inventing a non-dapple parallel store.

**Surface posture (unchanged).** Every endpoint added here continues to enforce the locked invariants:
- JWT signed with `DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY` only (cryptographic isolation).
- Every SQL query filters by `caller.company_id` (namespace isolation).
- Cross-namespace existence does not leak: foreign-UUID and nonexistent-UUID both return 404 `RESOURCE_NOT_FOUND`.
- No internal-token override escape hatches.

## 3. Audit — does the runner have what it needs?

For each capability the consumer's report requires, the table below records: status, where it lives, and rationale for any "deliberately out of scope" calls.

| Capability | Status | Where | Notes |
|---|---|---|---|
| Create journey from playbook | ✅ covered | `POST /api/journeys` (handler at `crates/dapple-prompt-external-api/src/handler/journeys.rs:36`) | Namespace-validates `playbook_id`, `brief_pack_ids`, `inference_profile_ids`. SDK gap: `ExternalJourneysClient::create` is not implemented — added in §5. |
| Kick a dispatch | ✅ covered | `POST /api/dispatches` (handler at `handler/dispatches.rs:38`) | Namespace-validates every `journey_id` in the batch. Auto-spawns execution if all briefs are bound. SDK gap: no `ExternalDispatchesClient` — added in §5. |
| Poll dispatch status | ✅ covered | `GET /api/dispatches/{id}` (handler at `handler/dispatches.rs:196`) | Returns the full nested `Dispatch` DTO (stages/steps/tasks). SDK gap: see §5. |
| Render brief forms / detect AwaitingInput | ✅ covered | `GET /api/dispatches/{id}/briefs` (handler at `handler/dispatches.rs:222`) | Returns `DispatchBriefsResponse` with per-unit binding state. SDK gap: see §5. |
| Per-task phase derivation (Running / Failed / Ok) | ❌ **gap** — covered by this plan | Internal-only at `dapple-prompt-api::handler::dispatch_task_attempts::task_attempt_index_handler` | This plan adds `GET /api/dispatches/{id}/task_attempts` (§4.1–§4.6). |
| Submit operator's input → advance | ✅ covered | `POST /api/dispatch-intent-bindings` (handler at `handler/dispatch_intent_bindings.rs:35`) | Auto-spawns execution when all slots are bound. SDK gap: no `ExternalDispatchIntentBindingsClient` — added in §5. |
| Per-journey briefs | ✅ covered | `GET /api/journeys/{id}/briefs` (handler at `handler/journey_briefs.rs:19`) | Already mirrored from internal. |
| Discover playbooks for the operator's namespace | ✅ covered (indirect) | `GET /api/guides` returns `Guide.playbooks: Vec<GuidePlaybookEnrichedModel>` per guide | The runner walks the customer's guides and surfaces playbooks from them. **No separate `GET /api/playbooks` endpoint is recommended for V1** — adding one would replicate path-namespace gating in a third place and create a discovery surface that disagrees with guides (a playbook can be active without being surfaced via guides if the customer authored it that way; but in practice all external playbooks come from guide ingest). If a runner later needs orphan-playbook discovery, that's a follow-up. |
| Retrieve playbook structure (stages/steps) for preview | ⚠️ **partial / out of scope** | The `Guide.playbooks: Vec<GuidePlaybookEnrichedModel>` already includes enriched stage/step structure. **No separate `GET /api/playbooks/{id}` endpoint is recommended for V1.** The runner-side preview can render from the guide payload. If runtime preview without the parent guide is needed later, add it as a deliberate decision. |
| Get a single journey | ❌ **gap** — covered by this plan, see §4.7 | Internal `GET /api/journeys/{id}` exists; this plan adds the external counterpart. | Rationale: an operator-runner process may need to resync against a known journey UUID after restart. The endpoint is namespace-gated via `journey_in_namespace`. |
| List operator's own journeys | ❌ **gap** — covered by this plan, see §4.8 | No external counterpart. | Rationale: a runner UI may eventually surface "your open dispatches" cross-customer, which benefits from journey listing. Requires one new SQL file (`select_journeys_by_owner_company_id.sql`) and one new public query function — see §4.8.3. |
| Anything else `dapple-studio` uses from internal `DappleClient` | reviewed | `dapple-studio` is internal-only; operators access dapple through acme-api → external-api proxying, not directly. The internal-only surface (`tasks()`, `steps()`, `stages()`, etc.) is structural-isolation guarded against being mounted on `ExternalDappleClient`. **No additional endpoints needed.** |

**Audit verdict:** three endpoints are structural gaps (`task_attempts`, single-journey GET, list-journeys GET). Four SDK clients/methods are missing (the three new endpoints plus three existing endpoints not yet wrapped in the SDK).

## 4. Endpoint designs

### 4.1 Route — `GET /api/dispatches/{id}/task_attempts`

- Path: `GET /api/dispatches/{id}/task_attempts` (plural — see Decisions §1).
- Mounted by `route::dispatches::router()` (extend the existing builder).
- New file: `crates/dapple-prompt-external-api/src/handler/dispatch_task_attempts.rs`.
- Modified file: `crates/dapple-prompt-external-api/src/route/dispatches.rs` (no new route file — `task_attempts` is a sub-path of dispatches, exactly as on the internal side).

### 4.2 Handler signature — `task_attempts`

```rust
// crates/dapple-prompt-external-api/src/handler/dispatch_task_attempts.rs

use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::query::dispatch_task_attempts::select_task_attempts_by_dispatch_id;
use dapple_data::query::dispatches::dispatch_in_namespace;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn dispatch_task_attempts_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(dispatch_id): Path<Uuid>,
) -> impl IntoApiResponse {
    // Namespace gate. Identical 404 for foreign-namespace and unknown-id
    // — no cross-namespace existence leak.
    match dispatch_in_namespace(&state.db, dispatch_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found("dispatch", dispatch_id),
        Err(e) => return db_error("dispatch_in_namespace", e),
    }

    match select_task_attempts_by_dispatch_id(&state.db, dispatch_id).await {
        Ok(attempts) => {
            // Pass-through: internal DispatchTaskAttempt DTO is the wire
            // shape on the external surface (Decisions §4). The
            // `error_detail` / `context_data` / `context_metadata` fields
            // carry the same authoring contract as the rest of the surface
            // — external-scope tasks must not place internal-only data in
            // their payloads.
            (
                StatusCode::OK,
                Json(json!({
                    "status": "ok",
                    "results": attempts.len(),
                    "objs": attempts,
                })),
            )
                .into_response()
        }
        Err(e) => db_error("select_task_attempts_by_dispatch_id", e),
    }
}

// not_found / db_error helpers mirror handler/dispatches.rs:249-281.
```

### 4.3 SQL — reuses `select_task_attempts_by_dispatch_id`

The existing query at `crates/dapple-data/assets/.sql/dispatches/dispatch_task_attempts/select_by_dispatch_object_id.sql` already does the right thing: it walks `dispatch_task_attempts → dispatch_steps → dispatch_stages → dispatches` and filters by `dispatches.id = $1`. The namespace gate is applied separately via `dispatch_in_namespace` (which checks `dispatches.owner_company_id = $caller.company_id`).

**No new SQL files. No new `dapple-data` query functions for this endpoint.**

**Why two queries instead of one merged query?** Same pattern as every other external handler in this crate (see `dispatches.rs:200-218` for `GET /api/dispatches/{id}`). The cost of the extra round-trip is negligible (single-row `EXISTS`), and the pattern keeps namespace gating uniform and auditable across the handler surface.

### 4.4 Response shape — `task_attempts`

Pass-through. The endpoint returns the internal `DispatchTaskAttempt` DTO unchanged. No new schema file in `dapple-prompt-external-api/src/schema/`.

The DTO is defined in `crates/dapple-types/src/model/dispatch_task_attempts.rs` (re-exported from `dapple_data::model::dispatch_task_attempts::DispatchTaskAttempt` and through `dapple_sdk::types::*`).

```rust
// Existing — no plan changes:
// crates/dapple-types/src/model/dispatch_task_attempts.rs
pub struct DispatchTaskAttempt {
    pub id: Uuid,
    pub dispatch_step_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub context_data: serde_json::Value,
    pub context_metadata: serde_json::Value,
    pub error_detail: Option<String>,
    pub created_at: DateTime,
}
```

The authoring contract for content fields (`error_detail`, `context_data`, `context_metadata`) is documented as an external-surface invariant — see §8.1.

### 4.5 Route registration — `task_attempts`

```rust
// crates/dapple-prompt-external-api/src/route/dispatches.rs

use crate::ExternalAppState;
use crate::handler::dispatch_task_attempts::dispatch_task_attempts_index_handler;
use crate::handler::dispatches::{
    dispatches_briefs_get_handler, dispatches_get_handler, dispatches_post_handler,
};
use aide::axum::ApiRouter;
use aide::axum::routing::{get, post};
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", post(dispatches_post_handler))
        .api_route("/{id}", get(dispatches_get_handler))
        .api_route("/{id}/briefs", get(dispatches_briefs_get_handler))
        .api_route("/{id}/task_attempts", get(dispatch_task_attempts_index_handler))
}
```

Register the new handler module in `crates/dapple-prompt-external-api/src/handler.rs`.

### 4.6 Error states — `task_attempts`

| Status | When | Body |
|---|---|---|
| 200 | Dispatch exists in caller's namespace; query succeeds (returns empty `objs` if no attempts) | `{ status: "ok", results: N, objs: [...] }` |
| 401 | Missing or malformed `Authorization` header | Handled by `ExternalCaller` extractor before reaching this handler |
| 404 | Dispatch does not exist OR exists but in another namespace | `{ error: "RESOURCE_NOT_FOUND", message: "dispatch {uuid} not found in caller's namespace" }` |
| 500 | DB error (connection failure, query error, etc.) | `{ error: "REQUEST_FAILED", message: "..." }` (logged at `error` level) |

**Cross-namespace existence does not leak.** A dispatch UUID that belongs to a different company returns the exact same 404 body as a randomly generated UUID — no timing side-channel, no body-shape side-channel. This is verified explicitly in the test plan (§6).

### 4.7 Route + handler — `GET /api/journeys/{id}`

- Path: `GET /api/journeys/{id}`.
- Mounted by extending `route::journeys::router()`.
- Modified file: `crates/dapple-prompt-external-api/src/handler/journeys.rs` (add new handler alongside the existing POST/PATCH/DELETE).
- Modified file: `crates/dapple-prompt-external-api/src/route/journeys.rs` (add `.api_route("/{id}", get(...))`).

The handler mirrors the existing external `journeys_patch_handler` namespace-gating pattern: gate first via `journey_in_namespace`, then read via the existing `select_journey_by_universal_id` (the viewer's preferences overlay using the caller's `principal_id` — same shape used by the PATCH/DELETE handlers, lines 282 and 323).

```rust
// crates/dapple-prompt-external-api/src/handler/journeys.rs — addition

/// GET /api/journeys/{id} — fetch a single journey in the caller's
/// namespace. Returns the same Journey DTO the PATCH/DELETE handlers
/// already return (overlaid with the caller's per-principal name/hidden
/// preferences). Namespace-gated via `journey_in_namespace`; a foreign
/// or unknown UUID both return the same 404 RESOURCE_NOT_FOUND body.
pub async fn journeys_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    if !match journey_in_namespace(&state.db, id, caller.company_id).await {
        Ok(b) => b,
        Err(e) => return db_error("journey_in_namespace", e),
    } {
        return not_found("journey", id);
    }

    match select_journey_by_universal_id(&state.db, caller.principal_id, id).await {
        Ok(journey) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "obj": journey,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_journey_by_universal_id", e),
    }
}
```

**Response shape:** the internal `Journey` DTO unchanged. The DTO has no internal-only fields — `id`, `name`, `principal_id`, `user_id` (None for external service principals), `playbook_id`, `brief_packs`, `inference_profiles`, `created_at`, `hidden_at`. All are safe for customer consumption.

**No new SQL. No new `dapple-data` query functions.** Reuses `journey_in_namespace` (already at `dapple_data::query::journeys::journey_in_namespace`) and `select_journey_by_universal_id` (already at `dapple_data::query::journeys::select_journey_by_universal_id`).

**Error states (same as `task_attempts`):** 200, 401 (extractor), 404 (foreign or unknown), 500 (DB). Foreign-namespace and unknown-UUID return identical 404 bodies — verified in tests (§6.13).

### 4.8 Route + handler — `GET /api/journeys`

- Path: `GET /api/journeys`.
- Mounted by extending `route::journeys::router()`.
- Modified files: `handler/journeys.rs` (add handler), `route/journeys.rs` (add `.api_route("/", get(...))`), `schema/journeys.rs` (add query schema), plus one new SQL file and one new query function in `dapple-data` (§4.8.3).

#### 4.8.1 Handler

```rust
// crates/dapple-prompt-external-api/src/handler/journeys.rs — addition

/// GET /api/journeys — list journeys in the caller's namespace.
/// Filters by `owner_company_id = caller.company_id` at the SQL site;
/// the caller's principal_id supplies the viewer-preferences overlay
/// (so journeys the caller has soft-hidden are filtered out).
///
/// Pagination via FilterOptions (`limit`, `page`) mirrors the internal
/// `GET /api/journeys` model. No `query=...` variants — the external
/// list is unambiguously "the caller's journeys".
pub async fn journeys_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Query(query): Query<ListExternalJourneysQuery>,
) -> impl IntoApiResponse {
    let filter_ops = query.options.unwrap_or_default();
    match select_journeys_by_owner_company_id(
        &state.db,
        caller.principal_id,
        caller.company_id,
        filter_ops,
    )
    .await
    {
        Ok(journeys) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": journeys.len(),
                "objs": journeys,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_journeys_by_owner_company_id", e),
    }
}
```

#### 4.8.2 Query schema

```rust
// crates/dapple-prompt-external-api/src/schema/journeys.rs — addition

use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Query parameters for `GET /api/journeys` on the external surface.
/// Pagination only — no `query=...` variants. The caller's company is
/// implicit (taken from the JWT), so the only knob is FilterOptions
/// (`limit`, `page`) to mirror the internal pagination model.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct ListExternalJourneysQuery {
    pub options: Option<FilterOptions>,
}
```

This is a fresh schema type local to the external crate. **Do not add a `ByOwnerCompanyId` variant to the internal `GetJourneySetSchema`** — the external list endpoint is its own handler with its own SQL; surfaces stay separate.

#### 4.8.3 New SQL — `select_journeys_by_owner_company_id`

**This is the only new SQL the plan introduces.** It mirrors the existing `select_journeys_by_user.sql` structure with the join target swapped from `rev.user_details` to `rev.companies`. The viewer-preferences overlay is preserved so the caller's `caller.principal_id` continues to govern hidden-state and name overrides.

```sql
-- File: crates/dapple-data/assets/.sql/journeys/select_journeys_by_owner_company_id.sql
-- (Author this via the .sqlc workflow — see Implementation Guidance §7.)
--
-- List journeys belonging to one company (the caller's namespace),
-- overlaid with the caller's per-principal preferences. The
-- `viewer_principal_id` is the caller's external_service_principal;
-- hidden_at filtering is the same as `select_journeys_by_user`.
--
-- Positional binds (alphabetical for sql-composer):
--   $1 = limit
--   $2 = offset
--   $3 = owner_company_id
--   $4 = viewer_principal_id
WITH args AS (
    SELECT
        $3::UUID AS owner_company_id,
        $4::UUID AS viewer_principal_id
),
viewer_prefs AS (
    SELECT DISTINCT ON (pjp.journey_id)
        pjp.journey_id,
        pjp.name AS name_override,
        pjp.hidden_at
    FROM rev.principal_journey_preferences pjp, args
    WHERE pjp.principal_id = args.viewer_principal_id
    ORDER BY pjp.journey_id, pjp.id DESC
),
limited AS (
    SELECT objs.*
    FROM rev.journeys objs, args
    LEFT JOIN viewer_prefs vp ON vp.journey_id = objs.id
    WHERE objs.owner_company_id = args.owner_company_id
      AND vp.hidden_at IS NULL
    ORDER BY objs.id DESC
    LIMIT $1 OFFSET $2
)
SELECT
    limited.id,
    limited.playbook_id,
    limited.principal_id,
    u.object_id AS "user_id?",
    COALESCE(vp.name_override, limited.name) AS "name!",
    limited.created_at,
    vp.hidden_at AS "hidden_at?",
    jbp.id AS "journey_brief_pack_id?",
    bp.id AS "brief_pack_id?",
    jip.id AS "journey_inference_profile_id?",
    ip.id AS "inference_profile_id?"
FROM limited
JOIN rev.playbooks p ON limited.playbook_id = p.id
LEFT JOIN viewer_prefs vp ON vp.journey_id = limited.id
LEFT JOIN rev.user_details u ON u.user_id = limited.user_id
LEFT JOIN rev.journey_brief_packs jbp ON jbp.journey_id = limited.id
LEFT JOIN rev.brief_packs bp ON bp.id = jbp.brief_pack_id
LEFT JOIN rev.journey_inference_profiles jip ON jip.journey_id = limited.id
LEFT JOIN rev.inference_profiles ip ON ip.id = jip.inference_profile_id
ORDER BY limited.id DESC, jbp.id, jip.id;
```

**sql-composer note:** the executor agent should author the `.sqlc` source first under `crates/dapple-data/assets/sqlc/journeys/select_journeys_by_owner_company_id.sqlc`, then run `cargo sqlc compose` to generate the `.sql` shown above, then `cargo sqlx prepare --workspace`. Binding order is alphabetical: `limit`, `offset`, `owner_company_id`, `viewer_principal_id` → `$1`, `$2`, `$3`, `$4`. Use `:bind(name)::TYPE` for each parameter. **No `:compose(...)` is needed** — this is a single self-contained SELECT, not a versioned-object SELECT that needs the shared body/page slot pattern (the journeys directory does not currently use slot composition; mirror the existing `select_journeys_by_user.sqlc` if one exists, or otherwise author it as a flat `.sqlc`).

The new public query function:

```rust
// crates/dapple-data/src/query/journeys.rs — addition

/// List the caller's journeys (filtered by `owner_company_id`), overlaid
/// with the caller's per-principal preferences. Used by the external
/// surface's `GET /api/journeys` handler — see plan
/// `docs/plans/external-api/01-operator-runner-endpoints.md`.
pub async fn select_journeys_by_owner_company_id(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    owner_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Journey>, DataError> {
    let models = sqlx::query_file_as!(
        FlatJourneyModel,
        "assets/.sql/journeys/select_journeys_by_owner_company_id.sql",
        filter_ops.sql_values().0,   // $1 = limit
        filter_ops.sql_values().1,   // $2 = offset
        owner_company_id,            // $3
        viewer_principal_id,         // $4
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatJourneyModel::fold_models(models)?;
    Ok(models.into_iter().map(Into::into).collect())
}
```

#### 4.8.4 Response shape — list-journeys

Pass-through `Vec<Journey>`. The external list endpoint returns the same `Journey` DTO as the single-get endpoint and the existing POST/PATCH/DELETE handlers. **No brief_packs / inference_profiles enrichment beyond what's already in the `Journey` DTO** — the internal `GET /api/journeys` does not enrich beyond that, and the external list mirrors the simplest shape that satisfies the runner.

#### 4.8.5 Pagination

Pagination via `FilterOptions` (`limit`, `page`) — matches the internal pagination model. Default `limit` is whatever `FilterOptions::default()` resolves to (see `dapple-types`). The `objs` array length tells the caller how many rows were returned; there is no separate `total_count` — consistent with the internal `index_response_handler` shape.

#### 4.8.6 Error states — list-journeys

| Status | When | Body |
|---|---|---|
| 200 | Always — empty list returns `{ status: "ok", results: 0, objs: [] }` | |
| 401 | Missing or malformed `Authorization` header | Extractor |
| 500 | DB error | `{ error: "REQUEST_FAILED", ... }` |

There is no 404 case — listing one's own (possibly empty) namespace always succeeds.

## 5. SDK additions

The narrow-by-construction precedent (see `crates/dapple-sdk/src/resource/external_journeys.rs:14`) requires each new method to be a deliberate decision, not a blanket mirror of the internal SDK. Below: each method to add, the endpoint it wraps, and a one-line justification.

### 5.1 `ExternalJourneysClient` additions

Existing methods (keep unchanged): `update(id, name)`, `hide(id)`.

| Method | Endpoint | Justification |
|---|---|---|
| `create(objs: Vec<CreateExternalJourneySchema>) -> Vec<Journey>` | `POST /api/journeys` | The endpoint already exists and is the **first step** of any dispatch runner flow. Without it, callers must hand-roll the HTTP request. Same wire shape as internal `JourneysClient::create`. |
| `get(id: Uuid) -> Journey` | `GET /api/journeys/{id}` (new — §4.7) | Required for post-restart resync against a known journey UUID. |
| `list(options: Option<FilterOptions>) -> Vec<Journey>` | `GET /api/journeys` (new — §4.8) | Required for runner-side discovery of the caller's journeys. |

**Do not add:** `briefs`, `favorites`, `recent`, `list_by_user`, `list_by_playbook`. Briefs are already covered via the per-journey briefs endpoint (`GET /api/journeys/{id}/briefs`) but that endpoint has no SDK method *yet* — adding it is straightforward but out of scope for this plan unless explicitly needed; flag in §10. Favorites/recent are user-scoped and customers have no user identities here. `list_by_user` is meaningless on the external surface (no user identities); `list_by_playbook` is not requested by the consumer.

```rust
// Additions to crates/dapple-sdk/src/resource/external_journeys.rs

use crate::types::{CreateExternalJourneySchema, FilterOptions, Journey};
use uuid::Uuid;

impl<'a> ExternalJourneysClient<'a> {
    /// POST /api/journeys — create one or more journeys in the caller's
    /// namespace. Each `playbook_id` must belong to the caller's company;
    /// the server returns 404 RESOURCE_NOT_FOUND for foreign or unknown
    /// playbook UUIDs. The created journey carries `owner_company_id =
    /// caller.company_id` and `principal_id = caller.external_service_principal`.
    pub async fn create(
        &self,
        objs: Vec<CreateExternalJourneySchema>,
    ) -> Result<Vec<Journey>, SdkError> {
        #[derive(serde::Serialize)]
        struct Body { objs: Vec<CreateExternalJourneySchema> }
        self.client.post_many("api/journeys", &Body { objs }).await
    }

    /// GET /api/journeys/{id} — fetch a single journey in the caller's
    /// namespace. Returns 404 RESOURCE_NOT_FOUND for foreign or unknown
    /// UUIDs (identical body — no existence leak).
    pub async fn get(&self, id: Uuid) -> Result<Journey, SdkError> {
        self.client.get_one(&format!("api/journeys/{id}")).await
    }

    /// GET /api/journeys — list the caller's journeys, paginated via
    /// FilterOptions (limit/page). Server filters by
    /// `owner_company_id = caller.company_id`.
    pub async fn list(
        &self,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Journey>, SdkError> {
        #[derive(serde::Serialize, Default)]
        struct Query { options: Option<FilterOptions> }
        self.client.get_many("api/journeys", &Query { options }).await
    }
}
```

### 5.2 New `ExternalDispatchesClient`

New file: `crates/dapple-sdk/src/resource/external_dispatches.rs`. Accessor: `ExternalDappleClient::dispatches()`.

| Method | Endpoint | Justification |
|---|---|---|
| `create(objs: Vec<CreateDispatchSchema>) -> Vec<Dispatch>` | `POST /api/dispatches` | Required to kick a dispatch. Same wire shape as internal `DispatchesClient::create`. Auto-spawn-on-fully-bound semantics are server-side. |
| `get(id: Uuid) -> Dispatch` | `GET /api/dispatches/{id}` | Required to render the runner panel (stages/steps/tasks). |
| `briefs(id: Uuid) -> DispatchBriefsResponse` | `GET /api/dispatches/{id}/briefs` | Required to render BriefRenderer and detect AwaitingInput. |
| `task_attempts(id: Uuid) -> Vec<DispatchTaskAttempt>` | `GET /api/dispatches/{id}/task_attempts` (new — §4.1) | Required for per-task phase derivation. Returns the internal `DispatchTaskAttempt` DTO unchanged (Decisions §4). |

**Do not add (compared to internal `DispatchesClient`):**

- `list`, `list_by_ids`, `list_by_journey`, `list_by_user`, `list_by_playbook` — operator dispatch-listing is out of scope. The runner discovers dispatches via the journey it just created or listed, not via cross-journey enumeration.
- `task_attempt(dispatch_id, attempt_id)` — single attempt get. The phase derivation needs the list, not a per-id get; if a runner later wants per-attempt detail, add deliberately.
- `task_attempt_events`, `create_task_attempt`, `create_task_attempt_event` — these are internal write paths used by the workflow engine. **Customers do not create attempts or events**; the dispatch_runner does, server-side.

```rust
// crates/dapple-sdk/src/resource/external_dispatches.rs

//! External-surface dispatches client (`external` feature gated).
//!
//! Narrow customer-facing slice of [`crate::resource::dispatches::DispatchesClient`].
//! Operator-runner methods only — list-by-* variants and write-side task-
//! attempt methods are absent by construction; the workflow engine creates
//! attempts and events server-side, and the operator-runner reads them.

use crate::types::{
    CreateDispatchSchema, Dispatch, DispatchBriefsResponse, DispatchTaskAttempt,
};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

pub struct ExternalDispatchesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ExternalDispatchesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// POST /api/dispatches — create one or more dispatches. The server
    /// namespace-validates every `journey_id` and auto-spawns in-process
    /// execution for any dispatch whose briefs are all already bound.
    pub async fn create(&self, objs: Vec<CreateDispatchSchema>) -> Result<Vec<Dispatch>, SdkError> {
        #[derive(serde::Serialize)]
        struct Body { objs: Vec<CreateDispatchSchema> }
        self.client.post_many("api/dispatches", &Body { objs }).await
    }

    /// GET /api/dispatches/{id} — poll dispatch status. Returns the
    /// stages/steps/tasks nested structure used by the runner panel.
    pub async fn get(&self, id: Uuid) -> Result<Dispatch, SdkError> {
        self.client.get_one(&format!("api/dispatches/{id}")).await
    }

    /// GET /api/dispatches/{id}/briefs — per-dispatch brief state. Used by
    /// BriefRenderer to render forms and detect AwaitingInput.
    pub async fn briefs(&self, id: Uuid) -> Result<DispatchBriefsResponse, SdkError> {
        self.client
            .get_one(&format!("api/dispatches/{id}/briefs"))
            .await
    }

    /// GET /api/dispatches/{id}/task_attempts — per-task runtime phase for
    /// runner phase derivation (Running / Failed / Ok). Returns the
    /// internal `DispatchTaskAttempt` DTO unchanged — see plan
    /// `docs/plans/external-api/01-operator-runner-endpoints.md` Decisions §4.
    pub async fn task_attempts(
        &self,
        id: Uuid,
    ) -> Result<Vec<DispatchTaskAttempt>, SdkError> {
        self.client
            .get_many(&format!("api/dispatches/{id}/task_attempts"), &())
            .await
    }
}
```

### 5.3 New `ExternalDispatchIntentBindingsClient`

New file: `crates/dapple-sdk/src/resource/external_dispatch_intent_bindings.rs`. Accessor: `ExternalDappleClient::dispatch_intent_bindings()`.

| Method | Endpoint | Justification |
|---|---|---|
| `create(objs: Vec<CreateDispatchIntentBindingSchema>) -> Vec<DispatchIntentBinding>` | `POST /api/dispatch-intent-bindings` | Required to submit operator inputs and advance the dispatch. Server auto-spawns execution when all slots are bound. |

**Do not add:** `list`, `list_by_dispatch_ids`. Bindings are not externally listable — they're write-only from the operator's perspective, and the renderer reads them via the brief/dispatch endpoints.

```rust
// crates/dapple-sdk/src/resource/external_dispatch_intent_bindings.rs

//! External-surface dispatch-intent-bindings client (`external` feature gated).
//!
//! Single-method write client. Bindings are not externally listable — the
//! renderer reads binding state through the brief endpoints.

use crate::types::{CreateDispatchIntentBindingSchema, DispatchIntentBinding};
use crate::{ClientCore, SdkError};

pub struct ExternalDispatchIntentBindingsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ExternalDispatchIntentBindingsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// POST /api/dispatch-intent-bindings — submit operator inputs for
    /// value-slots on a dispatch. The server namespace-validates every
    /// `dispatch_id` and auto-spawns execution for any dispatch whose briefs
    /// are now fully bound.
    pub async fn create(
        &self,
        objs: Vec<CreateDispatchIntentBindingSchema>,
    ) -> Result<Vec<DispatchIntentBinding>, SdkError> {
        #[derive(serde::Serialize)]
        struct Body { objs: Vec<CreateDispatchIntentBindingSchema> }
        self.client
            .post_many("api/dispatch-intent-bindings", &Body { objs })
            .await
    }
}
```

### 5.4 `ExternalDappleClient` accessor additions

```rust
// crates/dapple-sdk/src/lib.rs additions inside #[cfg(feature = "external")] impl ExternalDappleClient { }

/// Access the customer's dispatches — operator-runner methods (create,
/// get, briefs, task_attempts).
pub fn dispatches(&self) -> resource::external_dispatches::ExternalDispatchesClient<'_> {
    resource::external_dispatches::ExternalDispatchesClient::new(&self.core)
}

/// Access dispatch intent bindings — write-only from the customer's
/// perspective.
pub fn dispatch_intent_bindings(
    &self,
) -> resource::external_dispatch_intent_bindings::ExternalDispatchIntentBindingsClient<'_> {
    resource::external_dispatch_intent_bindings::ExternalDispatchIntentBindingsClient::new(&self.core)
}
```

Register both modules in `crates/dapple-sdk/src/resource/mod.rs` under the appropriate `#[cfg(feature = "external")]` gate (mirroring `external_guides` / `external_journeys`).

### 5.5 Types — what needs to be in `crates/dapple-types`

- `DispatchTaskAttempt` — **already exists** at `dapple_types::model::dispatch_task_attempts::DispatchTaskAttempt`. Already re-exported through `dapple_sdk::types::*` via the wildcard at `crates/dapple-sdk/src/types.rs:20`. No new types needed.
- `Dispatch`, `DispatchBriefsResponse`, `CreateDispatchSchema`, `CreateDispatchIntentBindingSchema`, `DispatchIntentBinding`, `CreateExternalJourneySchema`, `Journey`, `FilterOptions` — already exist; already re-exported through `dapple_sdk::types::*`. Confirm during implementation that each is reachable for the `external` feature; no new types expected.

## 6. Lifecycle test coverage

All new tests go in `crates/dapple-prompt-external-api/tests/external_lifecycle.rs`. Each `#[sqlx::test(migrations = "./../dapple-data/migrations")]` gets a fresh DB and (where needed) `#[serial_test::serial(external_signing_key)]` for env-var safety.

### 6.1 Happy path — `task_attempts` endpoint

```
test name: get_dispatch_task_attempts_happy_path
setup:
  - setup_customer(alpha)
  - seed_external_playbook(alpha) → playbook_id
  - seed_external_journey(alpha, playbook_id) → journey_id
  - seed_external_dispatch(alpha, journey_id) → dispatch_id
  - Insert one rev.dispatch_task_attempts row via direct SQL (mirror
    seed_external_dispatch's shape — no need to drive the full executor)
exchange alpha's token → JWT
GET /api/dispatches/{dispatch_id}/task_attempts with alpha's JWT
assert: 200; body.objs.len() == 1; body.objs[0].id == seeded attempt id;
        body.objs[0] deserializes successfully as the full internal
        DispatchTaskAttempt (with context_data / context_metadata
        present — pass-through DTO per Decisions §4);
        body.objs[0].error_detail follows Decisions §2 (verbatim).
```

### 6.2 Cross-namespace negative — `task_attempts` endpoint

```
test name: get_dispatch_task_attempts_foreign_dispatch_404
setup:
  - alpha: setup_customer + seed_external_playbook + seed_external_journey +
    seed_external_dispatch
  - beta: onboard_with_playbook
GET /api/dispatches/{alpha_dispatch_id}/task_attempts with beta's JWT
assert: 404; body.error == "RESOURCE_NOT_FOUND";
        body.message format matches handler::dispatches::not_found
        (string contains "not found in caller's namespace").
```

### 6.3 Empty-state — `task_attempts` endpoint

```
test name: get_dispatch_task_attempts_empty
setup:
  - alpha: setup_customer + seed_external_playbook + seed_external_journey +
    seed_external_dispatch — but DO NOT seed any task attempts
GET /api/dispatches/{dispatch_id}/task_attempts
assert: 200; body.results == 0; body.objs == [].
```

### 6.4 Nonexistent dispatch — `task_attempts` endpoint

```
test name: get_dispatch_task_attempts_nonexistent_404
setup:
  - alpha onboarded
GET /api/dispatches/{Uuid::new_v4()}/task_attempts with alpha's JWT
assert: 404; body.error == "RESOURCE_NOT_FOUND".
```

This catches the "unknown dispatch UUID returns same 404 body as foreign dispatch" guarantee.

### 6.5 Malformed dispatch UUID — `task_attempts` endpoint

```
test name: get_dispatch_task_attempts_malformed_uuid_400
setup:
  - alpha onboarded
GET /api/dispatches/not-a-uuid/task_attempts with alpha's JWT
assert: 400 (axum extractor rejection on Path<Uuid>).
```

Matches the existing `get_dispatch_malformed_uuid_400` pattern.

### 6.6 SDK round-trip — `ExternalDispatchesClient::task_attempts`

Add to the existing `happy_path_full_dispatch_execution` scenario (currently at line 1222):

```
extend happy_path_full_dispatch_execution:
  After the dispatch completes:
  - Build an ExternalDappleClient with alpha's signing key + base_url
  - Call client.dispatches().task_attempts(dispatch_id).await
  - assert: len > 0; statuses match the executor's terminal phases;
            response deserializes as Vec<DispatchTaskAttempt> with all
            fields populated (pass-through DTO).
```

### 6.7 SDK round-trip — `ExternalJourneysClient::create`

```
test name: external_sdk_create_journey
setup:
  - alpha onboarded with playbook
build ExternalDappleClient::external_at(base_url, service_id, signing_key)
call client.journeys().create(vec![CreateExternalJourneySchema {
    name: "test", playbook_id, brief_pack_ids: vec![], inference_profile_ids: vec![]
}]).await
assert: returns Vec<Journey> of length 1; journeys[0].id is a UUIDv7;
        DB read confirms owner_company_id = alpha.company_id
```

### 6.8 SDK round-trip — `ExternalDispatchesClient::create` + `get` + `briefs`

```
test name: external_sdk_full_dispatch_round_trip
setup:
  - alpha onboarded with playbook
  - SDK-create journey (reusing 6.7)
client.dispatches().create(...).await  -> Vec<Dispatch>
client.dispatches().get(dispatch_id).await  -> Dispatch
client.dispatches().briefs(dispatch_id).await  -> DispatchBriefsResponse
assert: each call succeeds; UUIDs are consistent across responses.
```

### 6.9 SDK round-trip — `ExternalDispatchIntentBindingsClient::create`

```
test name: external_sdk_create_bindings
setup:
  - alpha onboarded; dispatch created via SDK with at least one input-bearing
    brief unit (reuse seed helpers if direct DB seed is simpler)
client.dispatch_intent_bindings().create(vec![CreateDispatchIntentBindingSchema {
    dispatch_id, value_slot_id, ...
}]).await
assert: returns Vec<DispatchIntentBinding> length 1; binding is persisted in DB.
```

### 6.10 Cross-namespace SDK negative

```
test name: external_sdk_cross_namespace_blocked
setup:
  - alpha: onboard + seed_external_playbook + seed_external_journey +
    seed_external_dispatch
  - beta: onboard
build ExternalDappleClient with beta's signing key
call client.dispatches().task_attempts(alpha_dispatch_id).await
assert: returns SdkError::Api { status: 404, message contains "RESOURCE_NOT_FOUND" }
```

This proves the namespace isolation propagates to the SDK error type. One such test for `task_attempts` is sufficient — the wire-level isolation is exhaustively tested by existing per-endpoint cross-namespace tests (lines 909, 937, 966, 1001).

### 6.11 Happy path — `GET /api/journeys/{id}`

```
test name: get_journey_happy_path
setup:
  - alpha: setup_customer + seed_external_playbook + seed_external_journey
    → journey_id
exchange alpha's token → JWT
GET /api/journeys/{journey_id} with alpha's JWT
assert: 200; body.obj.id == journey_id; body.obj.playbook_id matches seed;
        body.obj deserializes as full Journey DTO.
```

### 6.12 Cross-namespace — `GET /api/journeys/{id}`

```
test name: get_journey_foreign_namespace_404
setup:
  - alpha: seed_external_journey → alpha_journey_id
  - beta: onboard
GET /api/journeys/{alpha_journey_id} with beta's JWT
assert: 404; body.error == "RESOURCE_NOT_FOUND";
        body.message contains "journey {uuid} not found in caller's namespace".
```

### 6.13 Nonexistent journey — `GET /api/journeys/{id}`

```
test name: get_journey_nonexistent_404
setup:
  - alpha onboarded
GET /api/journeys/{Uuid::new_v4()} with alpha's JWT
assert: 404; body matches 6.12 exactly (identical body for foreign and unknown).
```

### 6.14 Empty list — `GET /api/journeys`

```
test name: list_journeys_empty
setup:
  - alpha onboarded; no journeys
GET /api/journeys with alpha's JWT
assert: 200; body.results == 0; body.objs == [].
```

### 6.15 Non-empty list — `GET /api/journeys`

```
test name: list_journeys_non_empty
setup:
  - alpha: seed three external journeys (distinct names/playbooks)
GET /api/journeys with alpha's JWT
assert: 200; body.results == 3; body.objs.len() == 3;
        every obj.id is one of the seeded UUIDs;
        order is created_at DESC (newest first) — matches SQL `ORDER BY objs.id DESC`.
```

### 6.16 Cross-namespace isolation — `GET /api/journeys`

```
test name: list_journeys_cross_namespace_isolated
setup:
  - alpha: seed two journeys
  - beta: seed one journey
GET /api/journeys with beta's JWT
assert: 200; body.results == 1; body.objs[0].id matches beta's seed only.
GET /api/journeys with alpha's JWT
assert: 200; body.results == 2; both UUIDs are alpha's, neither is beta's.
```

### 6.17 SDK round-trip — `ExternalJourneysClient::get` + `list`

```
test name: external_sdk_get_and_list_journeys
setup:
  - alpha onboarded with playbook
  - SDK-create three journeys via client.journeys().create(...)
let one = client.journeys().get(journeys[0].id).await
assert: one.id == journeys[0].id
let all = client.journeys().list(None).await
assert: all.len() == 3; UUIDs match
```

## 7. Mandatory invariants — no new ones

Every endpoint added here is governed by the **existing** invariants in `crates/dapple-prompt-external-api/AGENTS.md`:

- ✅ Every external query takes `caller.company_id` as a binding (via `dispatch_in_namespace`, `journey_in_namespace`, or the new `select_journeys_by_owner_company_id` SQL).
- ✅ Scope filter is hard-coded `'external'` at the SQL site (inherited via the existing dispatch / journey namespace checks; `task_attempts` is downstream of a dispatch row whose `owner_company_id` is `'external'`-only by ingest construction).
- ✅ JWTs signed with the external surface's key only.
- ✅ No delegation; no `auth_user_id` parameter.
- ✅ `ExternalCaller` extractor only accepts external-iss external-token_use JWTs.

**No new invariants required for code structure.** One **authoring contract** is documented as an invariant in §8.1: external-scope tasks must not place internal-only data (file paths, internal IDs, stack traces) in their `signals.error`, `data.*`, or `metadata.*` payloads. The wire surface passes these through verbatim.

## 8. AGENTS.md updates

### 8.1 `crates/dapple-prompt-external-api/AGENTS.md`

Update the **V1 Endpoint Surface** table. Add three new rows, preserve existing rows:

```markdown
| `GET /api/journeys` | List caller's journeys | Namespace-gated; pagination via FilterOptions |
| `GET /api/journeys/{id}` | Fetch a single journey | Namespace-gated; identical 404 body for foreign or unknown UUID |
| `GET /api/dispatches/{id}/task_attempts` | Per-task runtime phase (Running/Failed/Ok) | Namespace-gated; pass-through internal DTO (see authoring contract below) |
```

Insert the journey rows directly after the existing `GET /api/journeys/{id}/briefs` row (logical adjacency), and the task_attempts row directly after `GET /api/dispatches/{id}/briefs`.

Update the **Module map** section:

```markdown
├── handler/
│   ├── ...
│   ├── dispatch_intent_bindings.rs   # POST bindings
│   ├── dispatch_task_attempts.rs     # GET /api/dispatches/{id}/task_attempts
│   ├── dispatches.rs                 # POST/GET dispatches + briefs
│   ├── journey_briefs.rs             # GET /api/journeys/{id}/briefs
│   ├── journeys.rs                   # POST/GET/PATCH/DELETE /api/journeys[/{id}]  (GET single + list added)
│   ├── ...
```

Add a new **External-scope authoring contract** section near the existing Mandatory-Invariants / Anti-Patterns block:

```markdown
## External-scope authoring contract

The external surface passes several content fields through verbatim from
the internal data model. Guide authors writing external-scope tasks
(`scope = 'external'` paths) are responsible for ensuring these fields
are safe to display to a third-party operator:

- `DispatchTaskAttempt.error_detail` — operator-visible failure summary,
  lifted from `metadata.signals.error`.
- `DispatchTaskAttempt.context_data` — task pipeline output.
- `DispatchTaskAttempt.context_metadata` — signals/validation state.

Do not place internal-only data (file paths, internal table names, stack
traces, dapple-side identifiers) in these payloads. The wire surface does
not redact — the responsibility lives in the task author's nushell script.
```

No changes to **Mandatory invariants** (no new structural invariants emerged).
No changes to **Anti-patterns** (existing anti-patterns already cover the not-listed-here cases).

### 8.2 `crates/dapple-sdk` — no AGENTS.md exists today; update doc comments only

The crate has no `AGENTS.md`. Doc comments on `ExternalDappleClient` (in `crates/dapple-sdk/src/lib.rs`) currently list the available external accessors via the lib.rs doc comment block at lines 362-369. Extend that block:

```rust
// In the `// External-surface resource accessors` comment block in lib.rs:
//
// Only the resource clients applicable to the external surface live
// here. Internal-only endpoints (canonical sync, fluent-message admin,
// playbooks management, etc.) are absent by construction. Adding more
// is straightforward once a customer SDK actually needs them.
//
// Customer-facing accessors:
//   - `guides()`       — list, ingest
//   - `journeys()`     — create, get, list, update, hide
//   - `dispatches()`   — create, get, briefs, task_attempts
//   - `dispatch_intent_bindings()` — create
```

No further `dapple-sdk` documentation changes needed.

### 8.3 `CLAUDE.md` — no update needed

Top-level `CLAUDE.md` already documents the multi-surface posture and points at the right per-crate AGENTS files. The endpoint additions are below that level of abstraction.

## 9. Implementation guidance (for the executor agent)

### 9.1 File-by-file change set

**New files (3):**
- `crates/dapple-prompt-external-api/src/handler/dispatch_task_attempts.rs` — handler (§4.2)
- `crates/dapple-sdk/src/resource/external_dispatches.rs` — SDK client (§5.2)
- `crates/dapple-sdk/src/resource/external_dispatch_intent_bindings.rs` — SDK client (§5.3)

**New SQL (1, via .sqlc):**
- `crates/dapple-data/assets/sqlc/journeys/select_journeys_by_owner_company_id.sqlc` — authored source
- Generated: `crates/dapple-data/assets/.sql/journeys/select_journeys_by_owner_company_id.sql` (run `cargo sqlc compose`)

**Modified files:**
- `crates/dapple-prompt-external-api/src/handler.rs` — add `pub mod dispatch_task_attempts;`
- `crates/dapple-prompt-external-api/src/handler/journeys.rs` — add `journeys_get_handler` (§4.7) and `journeys_index_handler` (§4.8) alongside existing POST/PATCH/DELETE
- `crates/dapple-prompt-external-api/src/schema/journeys.rs` — add `ListExternalJourneysQuery` (§4.8.2)
- `crates/dapple-prompt-external-api/src/route/dispatches.rs` — add task_attempts route (§4.5)
- `crates/dapple-prompt-external-api/src/route/journeys.rs` — add GET /{id} and GET / routes
- `crates/dapple-data/src/query/journeys.rs` — add `select_journeys_by_owner_company_id` (§4.8.3)
- `crates/dapple-sdk/src/resource/mod.rs` — `#[cfg(feature = "external")] pub mod external_dispatches;` + same for `external_dispatch_intent_bindings`
- `crates/dapple-sdk/src/resource/external_journeys.rs` — add `create`, `get`, `list` methods (§5.1)
- `crates/dapple-sdk/src/lib.rs` — add `dispatches()` and `dispatch_intent_bindings()` accessors on `ExternalDappleClient` (§5.4) + update the doc comment block
- `crates/dapple-prompt-external-api/tests/external_lifecycle.rs` — add scenarios (§6.1–§6.17)
- `crates/dapple-prompt-external-api/AGENTS.md` — table + module map + authoring contract section (§8.1)

**No new migrations.** No new types in `dapple-types` (all DTOs already exist).

### 9.2 Ordering and milestones

1. Author the new SQL (`select_journeys_by_owner_company_id.sqlc`), run `cargo sqlc compose`, run `cargo sqlx prepare --workspace`, confirm the workspace builds.
2. Land the new public query function `select_journeys_by_owner_company_id` in `dapple-data`.
3. Land the handler additions (task_attempts, journeys_get, journeys_index) + route updates + AGENTS.md update; run the new lifecycle tests (6.1–6.5, 6.11–6.16).
4. Land the SDK clients + accessors; run the new SDK round-trip tests (6.6–6.10, 6.17).

Each step is a single PR-sized change. The endpoint and SDK additions don't need to land together — the handlers are independently useful (acme-api can call them directly with `reqwest`).

### 9.3 Things the executor agent should NOT do

- Do **not** add `GET /api/playbooks` or `GET /api/playbooks/{id}` — explicitly out of scope (§3).
- Do **not** add a `ByOwnerCompanyId` variant to the internal `GetJourneySetSchema` — the external list endpoint is its own handler with its own SQL; surfaces stay separate (§4.8.2).
- Do **not** add `list_by_*` methods on `ExternalDispatchesClient` beyond the four specified — every absent method is a deliberate decision per §5.2.
- Do **not** add write methods on `ExternalDispatchesClient::task_attempt_*` — customers don't create attempts or events.
- Do **not** create a narrow `ExternalDispatchTaskAttempt` projection — per Decisions §4, the endpoint passes through the internal DTO unchanged.
- Do **not** enrich the list-journeys response with `brief_packs`/`inference_profiles` beyond what the existing `Journey` DTO carries — internal `GET /api/journeys` does not enrich, and the external list mirrors the simplest shape that satisfies the runner.

## 10. Open questions and risks

1. **`error_detail` content safety** (Decisions §2). Until task-author guidance is published, error strings may leak more than intended. **Mitigation:** the AGENTS.md "External-scope authoring contract" section (§8.1) makes this an explicit contract; the OpenAPI doc string on the field should mirror this language.
2. **`context_data` / `context_metadata` pass-through** (Decisions §4). Same authoring contract as `error_detail`, but with larger blast radius — these blobs can be arbitrarily large. **Mitigation:** authoring guidance + observability of payload sizes; if real-world payloads grow unwieldy, a follow-up plan can introduce optional projection (`?fields=summary` query param) without a breaking change.
3. **Auto-spawn execution failure modes** (Decisions §3). The `POST /api/dispatch-intent-bindings` handler currently logs auto-spawn execution errors to stderr and does not propagate them. If an operator submits valid bindings but the spawned execution fails immediately (e.g., a step that fails on first task), the client only learns about it on the next `task_attempts` poll. **Risk:** silent failure window. **Mitigation:** none required for this plan; existing behaviour is unchanged. A future audit could expose `last_execution_error` on the dispatch DTO if customers ask for it.
4. **Task-attempts polling load**. A naive runner that polls `task_attempts` every 250ms while a step runs will issue many queries. The current SQL is a `LEFT JOIN LATERAL` over a per-dispatch attempt set — fine for V1 but worth monitoring. **Mitigation:** none required for this plan; polling frequency is consumer-side policy.
5. **Wire-shape divergence between internal and external task_attempts endpoints**. Internal at `/dispatch/{id}/task_attempt/` (singular) and per-attempt-id sub-paths; external at `/dispatches/{id}/task_attempts` (plural) and no per-attempt-id sub-path. This is intentional (Decisions §1, §5.2 scope discipline), but documentation should call it out so customers don't expect a per-attempt-id GET. **Mitigation:** the AGENTS.md V1 Endpoint Surface table entry says explicitly "Per-task runtime phase" — list-only.
6. **Future need for per-attempt detail** (e.g., events trace for debugging). When that arises, add `GET /api/dispatches/{id}/task_attempts/{attempt_id}` and `GET /api/dispatches/{id}/task_attempts/{attempt_id}/events` deliberately — keep the narrow-by-construction discipline.
7. **Per-journey briefs SDK gap.** `GET /api/journeys/{id}/briefs` exists on the wire but has no `ExternalJourneysClient` method. **Not addressed in this plan** because the consumer's report did not ask for an SDK wrapper there (the renderer reads briefs via dispatches, not journeys). Flag if the consumer comes back asking for it — straightforward addition.
8. **`select_journeys_by_owner_company_id.sqlc` is new — flag for the implementation executor.** The journeys directory's existing `select_*` sources may or may not currently follow the slot pattern (see `crates/dapple-data/AGENTS.md` "SELECT `.sqlc` Pattern"). Inspect a sibling first; if the directory uses flat `.sqlc` files (no body/page slot split), author the new file in the same flat shape. If the directory has been migrated to slot pattern, author a page CTE + caller pair instead. The SQL body shown in §4.8.3 is the **compose output** — the `.sqlc` source must use `:bind(name)::TYPE` directives, and the executor should verify the resulting `$N` positions match the Rust call by inspecting the generated `.sql`.
