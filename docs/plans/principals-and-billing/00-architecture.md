# Principals and Billing Architecture Plan

**Status:** Draft (v1)
**Created:** 2026-05-11
**Last Updated:** 2026-05-11
**Target Version:** 0.x (additive + revert of unmerged Mig 37)
**Dependencies:**
- `docs/plans/external-api/00-architecture.md` — established `rev.companies`, `rev.external_service_tokens`, the namespace_company_id model. Migration 37 (the file this plan replaces) was the in-progress work for service users.
- Migrations 5 (`rev.user_details`), 7 (`rev.journeys`, `rev.dispatches`), 21 (`rev.billable_entities`, `rev.user_billable_entities`), 31 (`rev.companies`), 32 (`rev.company_billable_entities`), 34 (`rev.external_service_tokens`), 36 (`rev.journeys.owner_company_id`, `rev.dispatches.owner_company_id`).

## 1. Executive Summary

Migration 37 (in progress, not yet committed) introduced service-user identities by tacking a `user_kind` discriminator + nullable FK column onto `rev.user_details`, and dropping the `auth.users` FK so service rows could exist without a NextAuth backing. This plan **reverts that work** and lands the actor model in the shape the codebase actually needs:

- A first-class `rev.principals` table — the actor primitive that journeys/dispatches reference.
- Four per-kind extension tables using class-table-inheritance (the same pattern Mig 21 already proved for `billable_entities`): `human_principals`, `internal_service_principals`, `external_service_principals`, `external_subject_principals`.
- A `rev.principal_billable_entities` table that generalizes `rev.user_billable_entities` — any kind of principal can have an exclusive billable entity (humans, external services that pay dapple directly).
- New columns on `rev.journeys` and `rev.dispatches`: `principal_id` (who issued), `subject_principal_id` (on whose behalf — nullable), `billable_entity_id` (who pays — resolved at create-time and locked into the row).

This is bundled with the principals work because the **billing project is next**, and `user_billable_entities` would need reshaping for it regardless. Landing both layers together avoids a second migration of journeys/dispatches when billing is wired through.

**The four actor shapes the model supports:**

| Shape | Example | `principal_id` | `subject_principal_id` | Billing target |
|---|---|---|---|---|
| A — Human user | Bob via dapple-ui | Bob's `human_principal` | NULL | Bob's PBE or his employer-company's CBE |
| B — Internal service on behalf of user | dapple-ui as Bob | dapple-ui's `internal_service_principal` | Bob's `human_principal` | Same as A — resolved from subject |
| C — External service | acme's backend | acme's `external_service_principal` | NULL | acme's `company_billable_entity` |
| D — External service on behalf of an external user | acme on behalf of Globex | acme's `external_service_principal` | Globex's `external_subject_principal` | Still acme's CBE — dapple bills the customer, the customer bills Globex |

Shape B's data-model wiring lands later (no internal handler today threads service-principal through journeys; today they just stamp the auth user's principal as both actor and subject). Shape D lands later (lazy-create on first reference). The schema in V1 leaves both fully accommodated.

## 2. Context and Background

### What's wrong with the current Migration 37 shape

Two structural smells:

1. **Lost integrity on `rev.user_details`.** Mig 37 drops `user_details_user_id_fkey` (the FK to `auth.users`) so service-user rows can have `user_id` values from a separate sequence. The convention is now "human rows have a user_id that matches `auth.users.id`" with no DB enforcement. Drift is possible from any backfill bug, manual fix, or future schema mistake.

2. **Discriminator in identity table.** `user_details` mixes two essentially-different row kinds (humans and services) distinguished by `user_kind` + a nullable FK + an invariant CHECK. We already rejected this pattern at the router/token-table level (Pattern B → two separate token tables instead of a discriminator on `web.service_tokens`). Applying the rejected pattern one layer down on `user_details` is structurally inconsistent.

### Design principles carried forward

- **"Who uses" vs "who pays" is the fundamental distinction.** Migration 21's split of `billable_entities` from `user_billable_entities` already encodes this. A user is a who-uses concept; a billable_entity is a who-pays concept; they're separate tables with a 1:1 link.
- **Class-table-inheritance for actor-like primitives.** Migration 21 (`user_billable_entities`) + Migration 32 (`company_billable_entities`) extend a base `billable_entities` table with per-flavor tables. This plan applies the same pattern at the actor layer.
- **Locked attribution at create-time, no modifications.** Journey-level billing attribution gets stamped at creation; corrections happen via separate amendment records, not by mutating the original row. The `billable_entity_id` column on journeys/dispatches is computed once and frozen.

### Locks from prior discussion

- Revert Migration 37 in-place (it has not been committed yet).
- Keep `rev.user_details` as the rev-side human-identity record. `human_principals` references it via `user_id BIGINT`. Restore the `auth.users` FK.
- Defer internal-service principal auto-create — Shape B's full wiring is a later project.
- Resolve `journeys.billable_entity_id` in Rust at create-time, locked into the journey row.
- Lazy-create `external_subject_principals` rows on first reference (Shape D, future).
- Defer `company_members` / `departments` / `teams` — every principal carries an `owner_company_id`, which is sufficient for V1's four-shape resolver.

## 3. Decisions

### Decision 1 — Principals base + four detail tables (no discriminator column)

```sql
CREATE TABLE rev.principals (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    owner_company_id UUID NOT NULL REFERENCES rev.companies(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ NULL
);

CREATE TABLE rev.human_principals (
    principal_id UUID PRIMARY KEY REFERENCES rev.principals(id),
    user_id BIGINT NOT NULL UNIQUE REFERENCES rev.user_details(user_id)
);

CREATE TABLE rev.internal_service_principals (
    principal_id UUID PRIMARY KEY REFERENCES rev.principals(id),
    service_token_id UUID NOT NULL UNIQUE REFERENCES web.service_tokens(id)
);

CREATE TABLE rev.external_service_principals (
    principal_id UUID PRIMARY KEY REFERENCES rev.principals(id),
    external_service_token_id UUID NOT NULL UNIQUE REFERENCES rev.external_service_tokens(id)
);

CREATE TABLE rev.external_subject_principals (
    principal_id UUID PRIMARY KEY REFERENCES rev.principals(id),
    parent_principal_id UUID NOT NULL REFERENCES rev.principals(id),
    external_key TEXT NOT NULL,
    UNIQUE (parent_principal_id, external_key)
);
```

Each `rev.principals` row has exactly one row in one of the four detail tables — the "kind" is implied by which detail table holds the join. No discriminator column on the parent. Strong FKs on each detail table:

- `human_principals` → `rev.user_details(user_id)` (which in turn → `auth.users(id)` after Mig 37 reverts the FK drop).
- `internal_service_principals` → `web.service_tokens(id)`.
- `external_service_principals` → `rev.external_service_tokens(id)`.
- `external_subject_principals` → `rev.principals(id)` (self-reference to the parent external_service principal) + opaque `external_key TEXT` (acme's internal user identifier for their Globex).

Strict "exactly one detail row per principal" is not DB-enforced; the auto-create paths (handlers, triggers) are responsible. The benefit of enforcement vs the cost of an extra trigger isn't worth it for V1.

### Decision 2 — `principal_billable_entities` generalizes `user_billable_entities`

```sql
CREATE TABLE rev.principal_billable_entities (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    billable_entity_id UUID NOT NULL UNIQUE REFERENCES rev.billable_entities(id),
    principal_id UUID NOT NULL UNIQUE REFERENCES rev.principals(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

Generalizes Mig 21's `user_billable_entities` to all principal kinds. In practice:

- **Human principals** typically have a PBE (their personal billing identity).
- **Internal service principals** typically don't have a PBE — they don't bill anyone; whatever they do attributes through their subject.
- **External service principals** typically have a PBE (acme-as-customer pays dapple). Could also rely on `company_billable_entities` for the company-level CBE — both work, but having a PBE on the external_service_principal itself is cleaner because the company can have multiple service-principal rows (multiple service tokens) and we want them to share one billing identity.
- **External subject principals** typically don't have a PBE — acme intermediates their billing.

`user_billable_entities` stays for one release as a deprecation window; all readers migrate to `principal_billable_entities`. The eventual drop is a separate later migration.

### Decision 3 — Journey/dispatch columns

```sql
ALTER TABLE rev.journeys
    ADD COLUMN principal_id UUID NOT NULL REFERENCES rev.principals(id),
    ADD COLUMN subject_principal_id UUID NULL REFERENCES rev.principals(id),
    ADD COLUMN billable_entity_id UUID NULL REFERENCES rev.billable_entities(id);

ALTER TABLE rev.dispatches
    ADD COLUMN principal_id UUID NOT NULL REFERENCES rev.principals(id),
    ADD COLUMN subject_principal_id UUID NULL REFERENCES rev.principals(id),
    ADD COLUMN billable_entity_id UUID NULL REFERENCES rev.billable_entities(id);
```

The existing `owner_company_id UUID NOT NULL FK rev.companies(id)` column (added by Mig 36) stays; it is denormalized from `principals.owner_company_id` for filter speed and namespace gating.

The existing `user_id BIGINT` column stays for one release; the deprecation window mirrors the `user_billable_entities` retirement above. A later migration drops both.

`billable_entity_id` is `NULL` in V1 (many users don't have a billable entity yet — billing integration isn't wired). Once user onboarding always creates a `billable_entity` row, a future migration tightens this to NOT NULL.

### Decision 4 — Resolver function in Rust (`resolve_billable_entity`)

Stamping `billable_entity_id` happens at journey-create time, in Rust, via a deterministic resolver:

```text
billable_entity = COALESCE(
    subject_principal.principal_billable_entity,
    subject_principal.owner_company.company_billable_entity,
    principal.principal_billable_entity,
    principal.owner_company.company_billable_entity
)
```

`None` is acceptable in V1 (users without billable entities). When billing goes live, a NULL `billable_entity_id` on a freshly-created journey becomes a hard error from the handler.

Lives in `dapple-data` as `query::principals::resolve_billable_entity(pool, principal_id, subject_principal_id) -> Result<Option<Uuid>>`.

### Decision 5 — Auto-create trigger on `rev.external_service_tokens` insert

Migration 37's trigger creates a service-user row on `rev.user_details`. The replacement creates a principal row on `rev.principals` + an `external_service_principals` row pointing back at the new token.

```sql
CREATE FUNCTION rev.create_principal_for_external_service_token()
    RETURNS TRIGGER AS $$
DECLARE new_principal_id UUID;
BEGIN
    INSERT INTO rev.principals (owner_company_id)
    VALUES (NEW.company_id)
    RETURNING id INTO new_principal_id;

    INSERT INTO rev.external_service_principals (principal_id, external_service_token_id)
    VALUES (new_principal_id, NEW.id);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER external_service_token_creates_principal
    AFTER INSERT ON rev.external_service_tokens
    FOR EACH ROW EXECUTE FUNCTION rev.create_principal_for_external_service_token();
```

The `nextval('rev.service_user_id_seq')` and the dedicated BIGINT sequence are dropped — principals.id is UUIDv7, no more BIGINT collisions with NextAuth's range to worry about.

## 4. Migration Sequencing

Three migrations replace one. Mig 37 is **deleted** (the file has not been committed); the new sequence is 37 → 38 → 39.

**Mig 37 — Principals (revert + new shape):**
1. Drop the auto-create trigger function + trigger from old Mig 37.
2. Create `rev.principals` + 4 detail tables.
3. Backfill from current `user_details` state:
   - Every `user_kind='human'` row → one `principals` row (`owner_company_id = DAPPLE_COMPANY_UUID`) + one `human_principals` row.
   - Every `user_kind='service'` row → one `principals` row (`owner_company_id = (its external_service_token's company_id)`) + one `external_service_principals` row.
4. Revert `user_details` to its pre-Mig-37 shape: drop `user_kind`, drop `external_service_token_id`, drop both indexes + both constraints, restore the `auth.users` FK.
5. Drop the `service_user_id_seq` sequence.
6. Create the new auto-create trigger function + trigger (targeting principals + external_service_principals).

**Mig 38 — `principal_billable_entities`:**
1. Create `rev.principal_billable_entities` (mirrors `user_billable_entities` shape).
2. Backfill from `user_billable_entities` via JOIN through `human_principals`.
3. Leave `user_billable_entities` in place for a release.

**Mig 39 — Journey/dispatch principal columns:**
1. Add `principal_id` (NULL initially), `subject_principal_id` (NULL), `billable_entity_id` (NULL) to both tables.
2. Backfill `principal_id` from `user_id BIGINT` via `human_principals` lookup. (Dev DB has no Mig-37 service-user journeys; if any existed, they'd route through `external_service_principals` via `user_details.external_service_token_id` — that field is gone after Mig 37 reverts, so the backfill SQL needs to run before the column drop.)

   Migration ordering matters: backfill happens INSIDE Mig 39 after the Mig 37 reverts have landed, so service-user journeys are not actually backfillable from `user_details` alone. The dev DB doesn't have any; production would need to handle this differently. **Open question §10**.
3. Backfill `billable_entity_id` from `user_billable_entities` for old rows that have one.
4. Tighten `principal_id` to `NOT NULL`. `billable_entity_id` and `subject_principal_id` stay NULLable in V1.
5. Add indexes: `journeys_principal_idx`, `journeys_subject_principal_idx` (partial), `journeys_billable_entity_idx` (partial). Same for dispatches.

## 5. Code Touch-Points

### `dapple-data`

**New module** — `src/query/principals.rs`:
- `select_human_principal_for_auth_user(pool, auth_user_id: i64) -> Result<Option<Uuid>>`
- `select_external_service_principal_for_token(pool, external_service_token_id: Uuid) -> Result<Uuid>`
- `select_principal_by_id(pool, principal_id: Uuid) -> Result<PrincipalModel>` (rarely needed)
- `resolve_billable_entity(pool, principal_id: Uuid, subject_principal_id: Option<Uuid>) -> Result<Option<Uuid>>`

**Updates:**
- `src/model/journeys.rs::InsertJourneyArgs` — rename `user_ids: Vec<Uuid>` → `principal_ids: Vec<Uuid>`; add `subject_principal_ids: Vec<Option<Uuid>>` and `billable_entity_ids: Vec<Option<Uuid>>`.
- `src/model/external_service_tokens.rs::ExternalServiceTokenWithCompanyModel` — rename `service_user_object_id: Uuid` → `principal_id: Uuid`.
- `src/query/journeys.rs::insert_journeys` — pass the new args; drop the `user_details` JOIN in the SQL.
- `src/query/external_service_tokens.rs` — return the new shape (JOIN `external_service_principals` instead of `user_details`).

**`.sqlc` source files updated:**
- `journeys/insert_journeys.sqlc` — take `principal_id UUID` directly; drop user_details JOIN.
- `journeys/select_journeys_by_user.sqlc` and siblings — filter by `principal_id` (the caller-provided UUID), not through `user_details`.
- `dispatches/insert_dispatch.sqlc` — inherit `principal_id`, `subject_principal_id`, `billable_entity_id` from parent journey via JOIN.
- `external_service_tokens/select_external_service_token_by_service_id.sqlc` — JOIN `external_service_principals` instead of `user_details`.

**New `.sqlc` files in `principals/`:**
- `select_human_principal_for_auth_user.sqlc`
- `select_external_service_principal_for_token.sqlc`
- `resolve_billable_entity.sqlc`
- `select_principal_by_id.sqlc`

### `dapple-prompt-api` (internal handlers)

- `src/handler/journeys.rs` — translate the JWT's `auth_user_id` → `human_principals.principal_id` via `select_human_principal_for_auth_user` before stamping into `InsertJourneyArgs.principal_ids`. Resolve `billable_entity_id` via the resolver. `subject_principal_id` stays None for V1 (Shape B wiring is later).
- Same for `src/handler/dispatches.rs` if any direct user_id stamping happens (most flows inherit through the SQL JOIN, but verify).

### `dapple-prompt-external-api`

- `src/extractor.rs::ExternalCaller` — rename `service_user_object_id: Uuid` → `principal_id: Uuid`.
- `src/handler/service_token.rs::ExternalServiceJwtClaims` — rename `service_user_object_id` → `principal_id`. JWT-mint flow looks up the external_service_principal at exchange time and stamps `principal_id` into claims.
- `src/handler/journeys.rs` — replace `user_ids: vec![caller.service_user_object_id]` with `principal_ids: vec![caller.principal_id]`; call `resolve_billable_entity(&state.db, caller.principal_id, None)` and stamp the returned UUID into `billable_entity_ids`.
- `src/handler/dispatches.rs` — verify the dispatch insert SQL inherits principal_id/subject_principal_id/billable_entity_id from parent journey; no caller-side resolution needed.
- `src/handler/dispatch_intent_bindings.rs` — unchanged (it doesn't touch user_id).

### Tests

- `crates/dapple-prompt-external-api/tests/external_lifecycle.rs::setup_customer` — auto-create trigger will produce the principal + external_service_principal. Helper renames: `service_user_id` → `service_principal_id`. All 22 in-process tests need only field-name updates. The happy-path lifecycle test (real HTTP via reqwest) gets a small refactor to assert `principal_id` flows through the JWT claim.
- `crates/dapple-prompt-api/tests/*_lifecycle.rs` — any test that previously poked `user_details.user_kind` needs cleanup. Most tests are unaffected.

### Docs

- `crates/dapple-data/AGENTS.md` — Multi-Tenancy Primitives section gets a new subsection on principals + billing. Replaces any service-user references inherited from Mig 37 docs.
- `crates/dapple-prompt-external-api/AGENTS.md` — Caller model description updates to mention `principal_id` instead of `service_user_object_id`.
- `CLAUDE.md` — verify there are no service-user references to update.

## 6. Cross-Cutting Concerns

### Forward compatibility

- **Shape B (internal service on behalf of user) data wiring** — V1 schema accommodates it via `subject_principal_id`; no schema change later. Just the handler change to populate it.
- **Shape D (external service on behalf of an external user) lazy-create** — V1 schema accommodates it via `external_subject_principals`; the auto-create on first reference is a handler-layer change later.
- **`company_members`, `departments`, `teams`** — every `principal.owner_company_id` is a single FK today. Adding multi-membership is a future additive change (new table); the principal/billing tables don't need to change.
- **Internal-service principals** — every internal-service that needs to act on behalf of a user gets one row in `internal_service_principals`. Today, dapple-ui, conduit, and flow each have a `web.service_tokens` row. When Shape B is wired through journeys, each gets a corresponding `internal_service_principals` row (separate migration, additive).

### What does not change

- `rev.companies` — unchanged.
- `rev.external_service_tokens` — unchanged; only its trigger target changes.
- `rev.web.service_tokens` — unchanged.
- `rev.billable_entities`, `rev.user_billable_entities`, `rev.company_billable_entities` — unchanged. New `principal_billable_entities` is additive; `user_billable_entities` retires later.
- `rev.object_paths` namespace/scope model — unchanged. Principals are orthogonal to the path/namespace layer (they live on journeys/dispatches, not on paths).

## 7. Verification

1. `cargo sqlc compose --source assets/sqlc/ --target assets/.sql/` clean.
2. `cargo sqlx prepare --workspace` clean.
3. `cargo check --workspace` clean.
4. `cargo test -p dapple-prompt-external-api` — all 26 tests pass.
5. `cargo test -p dapple-prompt-api` — internal lifecycle tests pass.
6. DB invariants (checkable from psql after `cargo sqlx migrate run`):
   - `SELECT COUNT(*) FROM rev.user_details ud LEFT JOIN rev.human_principals hp USING (user_id) WHERE hp.principal_id IS NULL;` → 0 (every human user has a principal).
   - `SELECT COUNT(*) FROM rev.external_service_tokens t LEFT JOIN rev.external_service_principals esp ON esp.external_service_token_id = t.id WHERE esp.principal_id IS NULL;` → 0 (every external token has a principal, enforced by the trigger).
   - `SELECT COUNT(*) FROM rev.journeys WHERE principal_id IS NULL;` → 0 (Mig 39 tightens NOT NULL).
   - `SELECT COUNT(*) FROM rev.dispatches d JOIN rev.journeys j ON j.id = d.journey_id WHERE d.principal_id <> j.principal_id;` → 0 (dispatch principal inherits from journey).
7. Manual smoke (TUI or curl):
   - Insert an external_service_token → confirm trigger creates the matching principal.
   - POST journey via internal API as a NextAuth user → confirm `principal_id` on the row points at the matching `human_principals` row.
   - POST journey via external API as a customer service token → confirm `principal_id` on the row matches `external_service_principals.principal_id` for the token.

## 8. Implementation Order

1. **Mig 37** (drop old, write new; principals + 4 detail tables + trigger + user_details revert).
2. **Mig 38** (`principal_billable_entities`).
3. **Mig 39** (journey/dispatch principal columns).
4. **`cargo sqlx migrate run`** and verify invariants.
5. **dapple-data updates**: new `principals` query module, `.sqlc` updates, model field renames, `cargo sqlc compose`, `cargo sqlx prepare`.
6. **dapple-prompt-external-api**: extractor + token handler + journey handler updates.
7. **dapple-prompt-api**: internal journey handler updates (auth_user_id → principal_id translation).
8. **Tests**: update helpers in `external_lifecycle.rs`, verify all suites pass.
9. **Docs**: AGENTS.md updates.

## 9. AGENTS.md Updates

- `crates/dapple-data/AGENTS.md` — Multi-Tenancy Primitives section: replace "service user via discriminator on user_details" with the principals model (4 tables, class-table-inheritance, auto-create trigger). Add a note that `user_details` returned to its pre-Mig-37 shape with the auth.users FK restored. Add a note on `principal_billable_entities` generalizing `user_billable_entities`.
- `crates/dapple-prompt-external-api/AGENTS.md` — "Surface posture" section: ExternalCaller's `service_user_object_id` is renamed to `principal_id`. Journey/dispatch attribution now goes through principals.
- `crates/dapple-prompt-api/AGENTS.md` — Single-Tenant Posture section: internal handlers now resolve auth_user_id → human_principal.principal_id at handler entry; this is the in-process equivalent of what the external surface does at token-exchange time.

## 10. Open Questions

1. **Production backfill for Mig 39's `principal_id`.** The dev DB has zero service-user journeys (Mig 37 hasn't shipped). If there are pre-existing service-user journeys in production by the time this work merges, the Mig 39 backfill needs a path from `journeys.user_id BIGINT` to the service principal. Options: (a) require a staging migration that snapshots `user_details.external_service_token_id` before Mig 37 reverts it, then joins through that snapshot to populate principal_id; (b) accept that production hasn't seen any service-user traffic yet and the backfill is a no-op. Likely (b); document the constraint.

2. **Compliance audit chain for Shape D.** When acme acts on behalf of Globex, the `external_subject_principal` is lazy-created on first reference. Compliance might want an explicit consent record ("acme registered Globex as a subject on YYYY-MM-DD via this endpoint"). Possible future work: a `principal_registration_events` audit table, or a registration endpoint on the external API that requires explicit subject creation before journeys can reference it. Deferred.

3. **Explicit "this principal has no billable entity yet" signal.** V1 lets `journeys.billable_entity_id` be NULL silently. Once billing integration is wired, the handler should hard-fail at journey-create when `resolve_billable_entity` returns None for a principal whose journey should produce billable events. Open: where does the policy live (handler? resolver? journey insert SQL CHECK)?

4. **Pairing enforcement.** Today nothing prevents inserting a `principals` row with no matching detail row, or two detail rows (one in human_principals AND one in internal_service_principals). The handler/trigger paths don't produce this, but the schema doesn't enforce it either. Worth a check trigger? Probably not for V1; reconsider after a few months of operating data.
