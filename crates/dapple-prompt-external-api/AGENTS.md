# Agent Instructions — dapple-prompt-external-api

The customer-facing REST surface, served on `external-api.dapple.x`. Mounted as a separate Axum router with its own extractor, its own service-token table (`rev.external_service_tokens`), and its own JWT signing key. **Cryptographically isolated** from the internal `dapple-prompt-api`: a JWT signed for one surface cannot be verified by the other.

> **See also:**
> - [`docs/books/_foundation/authentication/external-service-tokens.md`](../../docs/books/_foundation/authentication/external-service-tokens.md) — the auth model
> - [`crates/dapple-data/AGENTS.md`](../dapple-data/AGENTS.md) — schema (Multi-Tenancy Primitives section)
> - [`crates/dapple-prompt-api/AGENTS.md`](../dapple-prompt-api/AGENTS.md) — the internal counterpart (do NOT cross the surfaces)

## Surface posture (read this first)

This crate serves **customer companies only** (third parties such as Acme). Internal dapple services use `dapple-prompt-api` over `api.dapple.x`. The two surfaces share no token table, no signing key, and no extractor.

- **Hostname:** `external-api.dapple.x` (public-facing).
- **Listener port:** 8081 (override via `DAPPLE_PROMPT_EXTERNAL_API_PORT`).
- **JWT signing key:** `DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY` env var. Missing → fail-fast at startup.
- **JWT `iss` claim:** `external-api.dapple.x`.
- **JWT `token_use` claim:** `external_service`.
- **JWT `sub` claim:** the company UUID (`rev.companies.id`), not a service_id string.

Every authenticated request flows through `ExternalCaller`, which produces a `(company_id, company_slug, service_id, principal_id)` tuple. `principal_id` is the `rev.principals.id` of the external_service_principal auto-created by Migration 37's trigger on `rev.external_service_tokens` insert — it's stamped onto every journey and dispatch the customer creates as `principal_id`. Every query SHOULD filter by `caller.company_id` — there is no code path that legitimately returns data outside the caller's namespace.

## Quick checklist: adding a new endpoint

1. [ ] Schema in `src/schema/{resource}.rs` — request + response + error types
2. [ ] Handler in `src/handler/{resource}.rs` — takes `caller: ExternalCaller` + `State<Arc<ExternalAppState>>` + `Query`/`Json` as needed
3. [ ] Route in `src/route/{resource}.rs` — usually `ApiRouter::new().api_route("/", method(handler))`
4. [ ] Register the module in `src/handler.rs`, `src/schema.rs`, `src/route.rs`
5. [ ] Mount in `lib.rs::build_router()` under the appropriate `/api/...` prefix
6. [ ] Filter every SQL query by `caller.company_id` (or use a query function that requires it)
7. [ ] Add a test scenario to `tests/external_lifecycle.rs` (seed → call → assert)

## Mandatory invariants

These are non-negotiable; CI/code-review should reject anything that breaks them.

- ✅ **Every external query takes `caller.company_id` as a binding.** No "list all" endpoints. No fallback that drops the namespace filter "for testing."
- ✅ **Scope filter is hard-coded to `'external'`** at the SQL site. Customers cannot see `public`/`common`/`internal` content (those are dapple-namespace tiers).
- ✅ **JWTs are signed with the external surface's key only.** Don't import `DAPPLE_PROMPT_API_SIGNING_KEY` here.
- ✅ **No delegation.** The customer's API does not act on behalf of dapple users; reject any handler that takes `auth_user_id` or similar.
- ✅ **Cross-tier auth is structurally impossible**: the `ExternalCaller` extractor only accepts JWTs with `iss = "external-api.dapple.x"` and `token_use = "external_service"`. Don't add an "internal token override" escape hatch.

## Anti-patterns

- ❌ Importing types from `dapple-prompt-api` other than the published `verify_service_token` / `ServiceTokenError` re-exports. Anything else couples the surfaces and risks data-isolation bugs.
- ❌ A "list all guides" endpoint (any variant that doesn't filter by `caller.company_id`).
- ❌ Calling `select_company_by_slug(pool, slug)` from a handler with a slug from the request body — that lets a customer impersonate another company. Always derive the company from the JWT claim, never from request input.
- ❌ Stamping `iss` or `token_use` from a request param. They are constants (`EXTERNAL_JWT_ISS`, `EXTERNAL_TOKEN_USE`) baked into the crate.

## V1 Endpoint Surface

| Endpoint | Purpose | Note |
|---|---|---|
| `POST /api/get-service-token` | Ed25519 token → JWT exchange | Mints JWT with `principal_id` claim |
| `GET /api/guides` | List customer's guides | Filtered by `caller.company_id` + `scope = 'external'` |
| `POST /api/guides/ingest` | Upload a tarballed guide | Stamps `(namespace_company_id = caller.company_id, scope = 'external')` on every produced object_path via per-tx Postgres GUCs. Field name in the multipart body is `file`. `source_path` is read from `guide.toml`'s top-level `source_path = "..."`. |
| `PATCH /api/guides/{path_id}/ingest` | Re-ingest an existing guide | Namespace-gated via `guide_path_in_namespace`; foreign or unknown `path_id` returns 404 `RESOURCE_NOT_FOUND` (no existence leak). Same GUC override as POST, applied to BOTH the version-create and refresh code paths so derived `object_paths` stay in the customer's namespace. Response carries `outcome: "refreshed"` (200) for content-hash matches or `outcome: "new_version"` (201) for new hashes. SDK callers should prefer `ExternalGuidesClient::upsert_from_dir`. |
| `POST /api/journeys` | Create journey on caller's playbook | Namespace-validated. Optional `predecessor_journey_id` chains journeys (whole binding pool inherits); optional `fork_stage_id` (requires a predecessor; must be a stage of the predecessor's playbook) makes it a stage-level fork — only bindings from stages strictly before the fork stage inherit, so the fork resumes at that pane. |
| `GET /api/journeys` | List caller's journeys | Namespace-gated; pagination via `FilterOptions` |
| `GET /api/journeys/{id}` | Fetch a single journey | Namespace-gated; identical 404 body for foreign or unknown UUID |
| `GET /api/journeys/{id}/briefs` | Per-journey brief list | Namespace-gated |
| `GET /api/journeys/{id}/stages` | Per-stage completion (`done` per playbook stage) | Namespace-gated; overlay-driven via `select_stage_binding_status_for_journey`. A stage is `done` when every value slot it exposes (its overlays) is bound in the journey chain; a stage with no overlays is trivially `done` (strict default). |
| `GET /api/journeys/{id}/steps` | Per-step exposed-slot status (`done` + `exposed_slots` per playbook step) | Namespace-gated; overlay-driven via `select_step_binding_status_for_journey`. Finer than `/stages`: each exposed slot is **self-describing** — `producer` (`"operator"` ⇒ the client POSTs a binding, rendering the control from `input`; `"computed"` ⇒ the server produces it once the producing step coheres (all its tasks succeed, no `unsafe` veto) — show a waiting state, don't bind **and don't dispatch**: the runtime auto-dispatches every pure-compute step itself, including a generate that feeds a picker, so a client that POSTs a dispatch for one double-runs it), `value_slot_id` (the journey-pinned bind target, now populated for computed slots too — use `producer`, not a NULL id, as the discriminator), `value_kind`, `bound`, and for operator slots an `input` render spec (`control_type`, `label_text`, `help_text`, `placeholder_text`, `choices`) so the client renders the right control without a second join to `/brief-form`. |
| `POST /api/dispatches` | Create dispatch on caller's journey | Auto-spawns in-process execution once the dispatch is ready (`dispatch_ready`: every step the dispatch contains has its overlay-exposed slots bound). Journey *completion* is NOT a gate term — it is a UI-only signal ("all steps done" / confetti). |
| `GET /api/dispatches/{id}` | Poll dispatch status | Namespace-gated |
| `GET /api/dispatches/{id}/briefs` | Per-dispatch brief state | Namespace-gated |
| `GET /api/dispatches/{id}/task_attempts` | Per-task runtime phase (running / ok / failed / `unsafe`) | Namespace-gated; returns `ExternalDispatchTaskAttempt` — projects `context_metadata.signals` into a `signals` field AND `context_metadata.computed_outputs` into a typed `computed_outputs` field (per-output production results, on the step's final `ok` attempt), drops the internal `context_metadata` (see authoring contract below). A task attempt ending `unsafe` is the `signals.unsafe` safety veto: the step stopped and is non-retryable (vs `failed`, which is retryable via re-dispatch). |
| `POST /api/dispatch-intent-bindings` | Submit value-slot bindings | Triggers execution of each touched dispatch that is now `dispatch_ready` (step-scoped: the binding flips the gate of the dispatch whose step exposes that slot) |

### Module map

```
src/
├── lib.rs                     # ExternalAppState, surface constants, build_router()
├── main.rs                    # Binary entry point (port 8081, fail-fast on missing key)
├── extractor.rs               # ExternalCaller + ExternalAuthRejection
├── handler.rs                 # mod declarations
├── handler/
│   ├── service_token.rs              # POST /api/get-service-token
│   ├── guides.rs                     # GET /api/guides
│   ├── guide_ingest.rs               # POST + PATCH /api/guides[/{path_id}]/ingest
│   ├── journeys.rs                   # POST/GET/PATCH/DELETE /api/journeys[/{id}]
│   ├── journey_briefs.rs             # GET /api/journeys/{id}/briefs
│   ├── journey_stages.rs             # GET /api/journeys/{id}/stages (per-stage completion)
│   ├── journey_steps.rs              # GET /api/journeys/{id}/steps (per-step exposed-slot status)
│   ├── dispatches.rs                 # POST/GET dispatches + briefs
│   ├── dispatch_task_attempts.rs     # GET /api/dispatches/{id}/task_attempts
│   └── dispatch_intent_bindings.rs   # POST bindings
├── schema/                    # request/response types for each handler
├── route/                     # route registration per handler
tests/
└── external_lifecycle.rs      # End-to-end customer lifecycle
```

## External-scope authoring contract

The external surface passes several content fields through from the internal
data model. Guide authors writing external-scope tasks (`scope = 'external'`
paths) are responsible for ensuring these fields are safe to display to a
third-party operator:

- `ExternalDispatchTaskAttempt.error_detail` — operator-visible failure
  summary, lifted from the failure event's `context_data.error`.
- `ExternalDispatchTaskAttempt.context_data` — task pipeline output.
- `ExternalDispatchTaskAttempt.signals` — author-emitted task signals
  (`metadata.signals.*`), the operator-facing talk-back channel (e.g.
  `menu_thin`, `items_deduped`, plus token counts).
- `ExternalDispatchTaskAttempt.computed_outputs` — per-output production results
  for the computed `:::brief-output`s this step produces. Each entry is
  `{ value_slot_id, name, outcome }` where `outcome` ∈ `produced` |
  `source_path_missing` | `type_mismatch` | `unsupported_kind`. Runtime-emitted
  (not author-controlled), so it is safe to display: it lets a customer see WHY a
  computed slot is (or is not) bound — distinguishing "the server is still
  working" from "the producer ran but couldn't extract this slot" — instead of a
  stuck-pendente chapter with no explanation.

The task-attempts endpoint does **not** pass the internal `context_metadata`
through. `GET /api/dispatches/{id}/task_attempts` maps each internal
`DispatchTaskAttempt` to [`ExternalDispatchTaskAttempt`]
(`dapple-types::model::dispatch_task_attempts`, shared with the SDK), which
**projects only `context_metadata.signals`** (→ `signals`) **and
`context_metadata.computed_outputs`** (→ `computed_outputs`) and drops the raw
`context_metadata`. This is a structural boundary: `context_metadata` is an
event-type-dependent grab-bag (token metrics + author signals on task-outcome
events, but dapple-side directive identifiers on internal `flow_directive` audit
events), and the customer surface must never expose those internals. The runner
persists the signals object under `context_metadata.signals` (see
`dispatch_runner::signals_metadata`) and the computed-output outcomes under
`context_metadata.computed_outputs` (written by
`dispatch_runner::produce_computed_output_bindings` on the `ok` event); events
with neither key (e.g. directive audits) project both to their empty form and so
are excluded automatically.

Do not place internal-only data (file paths, internal table names, stack
traces, dapple-side identifiers) in `context_data` or in author `signals`. The
wire surface does not redact — the responsibility lives in the task author's
nushell script.

### Asset upload note (V1 limitation)

`POST /api/guides/ingest` uses the per-transaction GUC pattern (`SET LOCAL`-style via `set_config(name, value, true)`) to redirect all `rev.object_paths` inserts from the dapple namespace to the caller's. **Asset uploads still land in the dapple namespace** because the asset-upload path runs *outside* the materialization transaction (chunked binary, idempotent by design). For the immediate test fixture (`assets/guides/stub-flow`, no assets) this is a no-op. Real customer guides with assets need a follow-up that threads the tx through `upload_assets` → `insert_drafts` / `insert_draft_chunks` / `finalize_draft`. Tracked as future work.

## Constants (from `lib.rs`)

| Constant | Value | Notes |
|---|---|---|
| `EXTERNAL_JWT_SIGNING_KEY_ENV` | `"DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY"` | Different from `DAPPLE_PROMPT_API_SIGNING_KEY` |
| `EXTERNAL_JWT_ISS` | `"external-api.dapple.x"` | Stamped into every minted JWT and validated by the extractor |
| `EXTERNAL_TOKEN_USE` | `"external_service"` | Same |

## Testing

`tests/external_lifecycle.rs` is the integration spine. Every endpoint should add a happy-path scenario plus at least one negative case to that file.

```bash
nu -c "cd crates/dapple-prompt-external-api; cargo test"
```

Each `#[sqlx::test]` gets its own fresh database (auto-migrated). Tests that mutate `EXTERNAL_JWT_SIGNING_KEY_ENV` use `#[serial_test::serial(external_signing_key)]` so they don't race each other.

The 14 baseline tests cover:
- Onboarding → JWT exchange → list (empty) → seed → list (returns own)
- Cross-namespace data isolation (alpha cannot see beta's guides)
- Cross-surface JWT replay (signature mismatch)
- Wrong `iss` / wrong `token_use` (claim defense-in-depth)
- Expired JWT, missing Authorization header
- Token revocation, company revocation, unknown service_id
- Missing signing key (503)

> **See also:** "Testing Repair Operations" in [`crates/dapple-prompt-api/AGENTS.md`](../dapple-prompt-api/AGENTS.md) — required reading before adding any test against a backfill / refresh / rebuild endpoint. The external surface doesn't expose `?backfill=true` today, but if/when it does the same rule applies: corrupt-then-verify in SQL, never assert only on response shape.

## Onboarding a customer (V1 manual flow)

1. Generate the customer's Ed25519 keypair on their side (private key stays with them).
2. Insert into `rev.companies` (slug, name; `id` defaults to `uuidv7()`).
3. Insert into `rev.external_service_tokens` (service_id, company_id, name, public_key_pem).
4. Share the `service_id` with the customer.
5. Customer signs tokens locally and exchanges them at `POST external-api.dapple.x/api/get-service-token`.

A `POST /api/companies` admin endpoint is a future improvement; until it lands, onboarding is a manual SQL or TUI action.

## Future work

- **Pattern B unified binary**: `dapple-prompt-server --surface={internal,external,user}` consolidating the binaries. Doesn't change auth or routing; pure ops refactor.
- **`dapple-external-sdk` crate**: thin HTTP client over this surface. No `dapple-data` dependency.
- **Remaining V1 endpoints** (Issues 5+): create-journey, get-briefs, post-dispatch-intent-bindings, create-dispatch, poll-dispatch.
- **Extracting `verify_service_token` to a shared `dapple-auth` crate** so this crate doesn't pull in the internal API's transitive deps.
