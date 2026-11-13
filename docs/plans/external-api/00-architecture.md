# External API Architecture Plan

**Status:** Draft (v3 — incorporates namespace_company_id + flat-slug model)
**Created:** 2026-05-10
**Last Updated:** 2026-05-10
**Target Version:** 0.x (additive)
**Dependencies:**
- `~/.claude/plans/i-would-like-for-abstract-hamming.md` — review/replacement of the original five issues; supplies conceptual model and the iterative refinements that landed this design.
- `docs/plans/guide-evolution.md §13–14` — `partner` scope is planned, distinct from external-org concerns.
- Migrations 2 (`rev.shared_scopes`, `rev.providers`), 3 (`rev.object_paths`), 5 (`web.service_tokens`, `rev.user_details`), 13 (`rev.canonical_release_pins`), 21 (`rev.billable_entities`, `rev.user_billable_entities`).

## 1. Executive Summary

This plan locks the **schema-touching decisions** that subsequent issues (router skeleton, external service-token issuance, list-guides endpoint, SDK skeleton) depend on. Endpoint surface, SDK module layout, and book outline are deferred.

Third-party REST consumers (e.g. *Acme*) are served by a separate Axum router (`dapple-prompt-external-api`) plus a thin SDK crate (`dapple-external-sdk`). The system runs as **three logical surfaces**, each fronted by its own K8s ingress:

| Surface | Hostname | Purpose |
|---|---|---|
| Internal | `api.dapple.x` | dapple-team services (`dapple-ui`, `dapple-conduit`); not exposed to the public |
| External | `external-api.dapple.x` | Customer companies (Acme et al); public |
| User (future) | `user-api.dapple.x` | End-user-facing; reserved, not implemented in V1 |

The internal API stays single-tenant. External isolation is real at the network/scheduling level (separate Deployments, separate NetworkPolicies), at the auth level (separate signing keys, separate token tables), and at the data layer.

The schema work is small and additive:

- A real `rev.companies` entity with an immutable `slug` column (the canonical org primitive).
- A separate `rev.external_service_tokens` table (not a discriminator on `web.service_tokens`).
- `rev.object_paths` gains a `namespace_company_id UUID FK` column. `scope` becomes a flat `NOT NULL` enum (`public`/`common`/`internal`/`external`, future `partner`); orgs are *not* embedded in scope. The vestigial `ref_key` column is dropped entirely (16 of 17 entity types ignored it; only `shared_content` had data, and only 2 orphan rows existed).
- Same column shape applied to `rev.canonical_release_pins` (which gains `namespace_company_id`; its existing `ref_key` continues to carry source_path-style strings for matching against `rev.guides.source_path`).
- Existing rows backfilled to dapple's namespace.
- Shared-content identifier system deferred to V2+ (no live shared content today; `guide_shared_refs` is empty).

**Scope semantics (V1):**
- `public` — dapple's broadly-visible tier (dapple namespace only).
- `common` — shared content within a namespace; available across guides in that namespace (any namespace).
- `internal` — dapple's private tier (dapple namespace only).
- `external` — customer's private tier (non-dapple namespace only). Customer-private content the customer asked dapple to host. Mirror of `internal` on the customer side, deliberately named differently so it's always obvious from the scope alone which side of the dapple/customer boundary the content is on.
- `partner` (V2+) — code/content from a partner that dapple runs for dapple's purposes. Dapple namespace, partner-source dimension.

**Pairing rules:** dapple namespace allows `public`/`common`/`internal`; non-dapple namespace allows `common`/`external`. Enforced primarily by Pattern B's structural router separation — the external API's code never sees `internal` as a valid value, the internal API's code never produces `external`. DB-level CHECK is the union of all V1 values; stricter trigger-based pairing enforcement is an open question (§10).

**No `external_org_id` column anywhere.** Org attribution lives on the `namespace_company_id` FK. **No hierarchical scope strings.** Scope is just the access tier.

## 2. Context and Background

The review at `~/.claude/plans/i-would-like-for-abstract-hamming.md` closed five misframed issues and replaced them with three sharper ones; this plan is the architecture document Replacement C points at. After multiple rounds of feedback, the items below are locked.

**Locked from prior work and iterative refinements:**

- **Two columns on `object_paths`, two concerns.** `namespace_company_id` carries ownership; `scope` carries access tier (a flat enum with no orgs embedded). The `ref_key` column on `object_paths` is dropped — it was vestigial (16 of 17 entity types never set it) and the existing path identifiers live elsewhere (`rev.guides.source_path`, `rev.{entity}_path_details.slug`).
- **No directories, no hierarchical paths.** Identifiers stay where they are today (`source_path`, `slug`); namespace+scope on `object_paths` provides the per-namespace attribution layer that the external API filters by.
- **Slugs are flat, immutable.** `rev.companies.slug` matches `^[a-z][a-z0-9_-]{1,31}$`; cannot contain `/`; cannot be updated after creation. The slug is for human-readable lookup (admin, TUI, JWT claim payload, audit logs); the UUID `id` is the FK target everywhere.
- **Internal `dapple-prompt-api` stays single-tenant.** External isolation is a separate router with a separate extractor — never feature flags.
- **Two service-token tables, not a discriminator.** Internal-tier and external-tier tokens never share a row, never share a signing key, never share an issuance endpoint.
- **Three K8s ingresses, port-per-surface, Pattern B deployment.** Single Rust binary, `--surface={internal,external,user}` flag selects which router to mount, three Deployments.
- **Separate JWT signing keys per surface.** A JWT signed for one surface cannot be verified by another — cryptographic isolation, not just an `aud` claim.
- **Classification stays a free string for V1**, namespaced per-org by the consumer's scope.
- **V1 includes external shared content.** acme can author `(namespace=acme, scope=common, ...)` and `(namespace=acme, scope=internal, ...)` content from day one.

**Implementation reality (verified against migrations):**

- `rev.billable_entities` and `rev.user_billable_entities` exist (migration 21) and use `UUID PRIMARY KEY DEFAULT uuidv7()` directly. We follow that convention.
- `rev.companies` is **not** implemented yet.
- `rev.providers` is the LLM-provider taxonomy. Acme is a *customer*, not a provider. **No naming collision.**
- `rev.object_paths.scope` is currently nullable `TEXT` with values `'common'`/`'internal'` set only for shared-content rows and `NULL` for the other 13,116 rows. `ref_key` is NULL for 13,116 of 13,118 rows; the 2 non-NULL rows are orphan shared-content rows with `'common/common'` and `'internal/internal'` (no `versioned_objects`, no `guide_shared_refs` reference them). After Migration 33 the layout is `(namespace_company_id, scope)` — `ref_key` is dropped, the orphans deleted.
- `rev.canonical_release_pins.scope` is empty in this DB (0 rows); by design it carries source_path-style values matching `rev.guides.source_path`. After Migration 33 it carries flat `'public'`/`'common'`/`'internal'`/`'external'` scope plus a new `namespace_company_id` column. `ref_key` stays as-is (it's how pins join to `guides.source_path`).
- `rev.guides.source_path` is the actual path-shaped identifier for guide entities (e.g. `'public/dapple/score-business-plan-preparation'`, `'internal/dapple/guide-builder'`). It stays unchanged by V1 — the external API filters via JOIN to `object_paths.namespace_company_id`, not via parsing source_path.

## 3. Decisions

### Decision 1 — Org primitive: minimal `rev.companies` with immutable flat slug

Companies is the canonical org primitive. Building a parallel `external_orgs` table would repeat the rejected `host`-column mistake at a higher level. We land **only** the minimal company table now plus `rev.company_billable_entities` to position the future billing hook. Members/teams/departments/parents are deferred.

The `slug` column carries the company's human-readable identifier — used in JWT claims, TUI commands, audit logs, and DB inspection. **It is not a FK target for ownership references; UUID `id` is.** Slug exists so DB rows and logs are readable without a join.

```sql
CREATE TABLE rev.companies (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ NULL,
    CONSTRAINT companies_slug_format
        CHECK (slug ~ '^[a-z][a-z0-9_-]{1,31}$')
);

CREATE INDEX companies_active_idx ON rev.companies (id) WHERE revoked_at IS NULL;

INSERT INTO rev.companies (slug, name) VALUES ('dapple', 'Dapple');
```

**Slug immutability.** The slug must not be updated after creation. Enforcement options at write time: a `BEFORE UPDATE` trigger that rejects any change to `slug`, or strict app-layer convention. Recommendation: trigger — defense-in-depth at the DB level matches the philosophy elsewhere in this plan. If a company genuinely needs a different slug, that is a new company row + an explicit data migration, never an `UPDATE`.

`revoked_at` is the soft-delete primitive; the external extractor refuses tokens belonging to revoked companies. The `dapple` row is seeded so Migration 33's backfill has a UUID to FK against.

**Explicitly deferred:** `company_parents`, `departments`, `teams`, `team_departments`, `company_members`, `department_members`, `team_members`, `company_member_roles`, `company_type`. Good designs; not needed for V1.

### Decision 2 — Service tokens: two separate tables

Internal-tier service tokens (`dapple_ui`, `dapple_conduit`) and external-tier service tokens (acme's clients) live in **separate tables**. A discriminator-on-shared-table is the same "feature flag as auth boundary" mistake already rejected at the router layer. Two tables means two issuance paths, two extractors, two signing-key infrastructures, and a blast-radius isolated from each other.

```sql
-- web.service_tokens — UNCHANGED. Internal services only.

-- New external table (rev schema — TUI-managed admin artifacts tied to companies):
CREATE TABLE rev.external_service_tokens (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    service_id TEXT NOT NULL UNIQUE,
    company_id UUID NOT NULL REFERENCES rev.companies (id),
    name TEXT NOT NULL,
    description TEXT,
    public_key_pem TEXT NOT NULL,
    key_algorithm TEXT NOT NULL DEFAULT 'Ed25519',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by_user_id BIGINT REFERENCES rev.user_details (user_id),
    CONSTRAINT external_service_id_format
        CHECK (service_id ~ '^[a-z][a-z0-9_]{2,31}$')
);

CREATE INDEX rev_external_service_tokens_company_idx
    ON rev.external_service_tokens (company_id);

CREATE TABLE rev.external_service_token_revocations (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    external_service_token_id UUID NOT NULL
        REFERENCES rev.external_service_tokens (id),
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_by_user_id BIGINT REFERENCES rev.user_details (user_id),
    reason TEXT
);
```

**JWT claims for external tokens:** `{ "sub": company_id (UUID), "company_slug": "acme", "service_id": "...", "token_use": "external_service", "exp": ..., "iss": "external-api.dapple.x" }`. Internal validators reject any JWT whose `iss` doesn't match their own surface — and the cryptographic-key-separation makes verification impossible regardless.

**Issuance endpoints are surface-local.** Each surface's router has its own `POST /api/get-service-token` handler that only consults its own table. There is no shared endpoint.

### Decision 3 — Path layer: namespace + scope on `object_paths`; `ref_key` dropped

`rev.object_paths` gains an explicit ownership column and tightens scope. The vestigial `ref_key` column is dropped — it was meant to be `shared_content`'s in-place identifier but only ever held 2 orphan rows (`'common/common'`, `'internal/internal'`) that nothing references. Identifiers for active entity types live elsewhere (`rev.guides.source_path`, `rev.{entity}_path_details.slug`) and are not touched by V1.

| Column on `rev.object_paths` after Migration 33 | Type | Carries |
|---|---|---|
| `entity_type_id` | SMALLINT FK | What kind of thing |
| `namespace_company_id` | UUID FK to `rev.companies(id)` | Who owns it |
| `scope` | TEXT, CHECK enum, NOT NULL | Access pattern |
| (dropped) `ref_key` | — | Removed; was vestigial |

**Scope** is a flat enum: `public`, `common`, `internal`, `external` (V1), with `partner` planned for V2+. **No orgs embedded in scope.**

**Tier semantics and namespace-pairing rules:**
- `public` — dapple's broadly-visible tier. Dapple namespace only.
- `common` — shared-content tier. Any namespace.
- `internal` — dapple-private tier. Dapple namespace only.
- `external` — customer-private tier. Non-dapple namespace only. Mirror of `internal` for content where dapple is the host but the customer is the owner.
- `partner` (V2+) — partner-sourced code dapple runs for dapple's purposes. Dapple namespace.

The pairing constraint (`public`/`internal` only in dapple's namespace; `external` only in non-dapple namespaces) is enforced primarily by Pattern B's router separation — the external API's request-handling code never references `internal` as a valid scope value, the internal API never produces `external`, and the cryptographic-key-separated JWTs make cross-router replay impossible. The DB-level CHECK enforces only the union (`scope IN ('public', 'common', 'internal', 'external')`). Stricter CHECK-based pairing enforcement (using dapple's fixed UUID as a constant in the CHECK expression) is an open question — see §10.

**Identity does not change.** Each path is still a UUID `id`. There is no per-namespace uniqueness constraint on the path layer because the actual identifiers (`source_path`, `slug`) live on entity tables, not on `object_paths`. **Hostile-collision protection comes from Pattern B**, not from a unique index — acme's external tokens can only INSERT into `object_paths` with `namespace_company_id = acme`'s UUID, so cross-namespace shadow writes are blocked at the auth layer. If/when stronger DB-level integrity is needed, per-entity unique indexes can be added (e.g. `UNIQUE (namespace_company_id, source_path)` on `rev.guides`) — flagged in §10 as a follow-up if a real mistake ever lands.

```sql
ALTER TABLE rev.object_paths
    ADD COLUMN namespace_company_id UUID NOT NULL
        REFERENCES rev.companies (id),
    ALTER COLUMN scope SET NOT NULL,
    ADD CONSTRAINT object_paths_scope_format
        CHECK (scope IN ('public', 'common', 'internal', 'external'));

DROP INDEX object_paths_ref_key_unique_idx;
ALTER TABLE rev.object_paths DROP COLUMN ref_key;

CREATE INDEX object_paths_namespace_scope_idx
    ON rev.object_paths (namespace_company_id, scope);
```

**Same namespace+scope treatment applied to `rev.canonical_release_pins`** — pins identify *which* version is current for a (namespace, scope, environment, ref_key) tuple. `ref_key` on `canonical_release_pins` is *not* dropped; it carries the source_path-style string that joins to `rev.guides.source_path` (this table's `ref_key` is genuinely used and currently 0-row, so no backfill).

```sql
ALTER TABLE rev.canonical_release_pins
    ADD COLUMN namespace_company_id UUID NOT NULL
        REFERENCES rev.companies (id),
    ADD CONSTRAINT canonical_release_pins_scope_format
        CHECK (scope IN ('public', 'common', 'internal', 'external'));

DROP INDEX IF EXISTS canonical_release_pins_scope_env_idx;
CREATE INDEX canonical_release_pins_namespace_scope_env_idx
    ON rev.canonical_release_pins (namespace_company_id, scope, environment, object_type, ref_key);
```

**`rev.shared_scopes` lookup table** is repurposed to hold the four V1 tier values (`public`, `common`, `internal`, `external`). Migration 33 deletes legacy rows and re-seeds. Alternative: drop the table entirely and rely on the CHECK constraint; pick at migration-write time.

**Shared content (V2+).** Today's `_shared/` content reference system is not actively populated (`guide_shared_refs` has 0 rows, `object_paths` has only 2 orphan shared-content rows being deleted). When shared content is wired up for real — including the acme-shared-content use case the user locked as supported — its identifier system needs a proper home. The right design at that point is likely a `rev.shared_content_path_details` table mirroring the other 16 path-detail tables; this is deferred to a separate plan.

### Decision 4 — Deployment topology, auth extractor, JWT shape

**Three logical surfaces, three K8s ingresses, Pattern B deployment.**

| Surface | Ingress hostname | Listener port | Public? | Token table | Signing key |
|---|---|---|---|---|---|
| Internal | `api.dapple.x` | 8080 (default) | No (cluster-internal) | `web.service_tokens` | internal-only |
| External | `external-api.dapple.x` | 8081 | Yes | `rev.external_service_tokens` | external-only |
| User (future) | `user-api.dapple.x` | 8082 | Yes | TBD (V2+) | user-only |

**Pattern B mechanics.** Single Rust binary takes a `--surface={internal,external,user}` flag at startup. The flag selects:
- Which router to mount (each surface has its own).
- Which port to bind.
- Which JWT signing key to load.
- Which extractor to use for service-token verification.

K8s runs three Deployments from the same image with different `--surface` args. Independent scaling, independent NetworkPolicies (the internal Deployment refuses traffic from the public ingress), independent pod identities.

**`ExternalCaller` extractor:**

```rust
pub struct ExternalCaller {
    pub company_id: Uuid,         // rev.companies.id — the FK used in WHERE clauses
    pub company_slug: String,     // rev.companies.slug — for logs, audit, and outbound payloads
    pub external_service_token_id: Uuid,
}

impl FromRequestParts<Arc<ExternalAppState>> for ExternalCaller {
    type Rejection = ExternalAuthRejection;
    async fn from_request_parts(parts: &mut Parts, state: &Arc<ExternalAppState>) -> Result<Self, Self::Rejection> {
        // 1. Decode JWT from Authorization: Bearer using the EXTERNAL signing key only.
        //    Tokens signed for any other surface fail at signature verification.
        // 2. Validate iss == "external-api.dapple.x" and token_use == "external_service".
        // 3. Look up rev.companies by JWT's sub (company_id); reject if revoked.
        // 4. Pre-resolve company_slug from that row.
        // 5. Return ExternalCaller.
    }
}
```

The slug is pre-resolved in the extractor so handlers always have both `company_id` (for SQL WHERE clauses) and `company_slug` (for logs/payloads) without a per-request join.

**Cross-surface token replay** is impossible by signing-key separation. Even if a JWT structure leaked, the verifier on a different surface would fail at signature check before it reached claim validation.

### Decision 5 — SQL filter pattern (end-to-end proof)

External-API queries filter on `namespace_company_id` (UUID) directly. The slug is not part of the filter — it's only used in pre-resolution and in display.

For external guide listings, the scope is **`external`** (the customer-private tier). The external API never queries dapple's `public`/`internal` content — those tiers are not valid in non-dapple namespaces and the external router's code does not reference them.

The pin-to-guide join continues to use `canonical_release_pins.ref_key = guides.source_path` (unchanged from today). The added namespace filter applies on both `canonical_release_pins.namespace_company_id` and `object_paths.namespace_company_id` as defense-in-depth — the FK chain makes disagreement unreachable, but each table has its own index and the planner uses them cleanly.

```sql
-- assets/sqlc/guides/pages/by_external_caller.sqlc
WITH current_pins AS (
    SELECT crp.ref_key, crp.versioned_object_id, crp.id AS pin_id
    FROM rev.canonical_release_pins crp
    LEFT JOIN rev.canonical_release_pins newer ON newer.supersedes_id = crp.id
    WHERE newer.id IS NULL
      AND crp.namespace_company_id = :bind(caller_company_id)::UUID
      AND crp.scope = 'external'
      AND crp.environment = :bind(environment)::TEXT
      AND crp.object_type = 'guides'
      AND crp.resolved_at IS NOT NULL
),
guide_latest AS (
    SELECT DISTINCT ON (op.id) g.id
    FROM rev.guides g
    JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN current_pins cp
        ON cp.ref_key = g.source_path           -- pin matches guide source_path
       AND cp.versioned_object_id = vo.id
    -- … latest_deprecation join unchanged …
    WHERE op.namespace_company_id = :bind(caller_company_id)::UUID
      AND op.scope = 'external'
    ORDER BY op.id, cp.pin_id NULLS LAST, vo.created_at DESC
),
guide_page AS (
    SELECT id FROM guide_latest
    ORDER BY id
    LIMIT :bind(limit)::BIGINT OFFSET :bind(offset)::BIGINT
)
```

The Rust function `select_external_guides_by_caller` takes `caller.company_id` as its single attribution parameter. There is no code path in the external router that selects guides without binding `caller_company_id`; that is the structural-isolation property.

The internal `dapple-prompt-api` doesn't need to filter by namespace at all — its single-tenant assumption means it implicitly works on the dapple namespace. (At startup, dapple's `company_id` could be cached by `AppState` and applied to writes that need a namespace. Reads do not filter; they see all rows in `rev.object_paths` because internal tooling like the TUI is the legitimate cross-cutting consumer.)

### Decision 6 — Crate layout

```
crates/
  dapple-prompt-api/             ← existing, internal handlers + RegisteredUser; mounted by --surface=internal
  dapple-prompt-external-api/    ← NEW, external handlers + ExternalCaller; mounted by --surface=external
  dapple-prompt-user-api/        ← reserved stub, not built in V1
  dapple-prompt-server/          ← NEW main binary; --surface flag dispatches router selection
```

`dapple-prompt-server` is responsible for:
- Parsing `--surface`.
- Loading the surface-specific JWT signing key (env var or secret path varies per surface).
- Constructing the chosen surface's router and `AppState`.
- Binding the surface's port.

The internal `dapple-prompt-api` and external `dapple-prompt-external-api` crates each export a `routes() -> Router` and `AppState` constructor. `dapple-prompt-server` is glue.

`dapple-external-sdk` is a separate crate (also new) — a thin HTTP client over the external surface. **No dependency on `dapple-data` or `dapple-prompt-api`** (or `dapple-prompt-external-api`); it talks over HTTP only. Module layout deferred.

## 4. Schema Additions (DDL Sketches)

Migration ordering matters; new migration numbers continue from 30 (highest current).

### 4.1 `00000000000031_companies.sql`

```sql
CREATE TABLE rev.companies (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ NULL,
    CONSTRAINT companies_slug_format
        CHECK (slug ~ '^[a-z][a-z0-9_-]{1,31}$')
);

CREATE INDEX companies_active_idx ON rev.companies (id) WHERE revoked_at IS NULL;

-- Enforce slug immutability at the DB level
CREATE OR REPLACE FUNCTION rev.companies_slug_immutable_trigger()
    RETURNS TRIGGER AS $$
BEGIN
    IF OLD.slug IS DISTINCT FROM NEW.slug THEN
        RAISE EXCEPTION 'rev.companies.slug is immutable (was %, attempted to change to %)',
            OLD.slug, NEW.slug;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER companies_slug_immutable
    BEFORE UPDATE ON rev.companies
    FOR EACH ROW EXECUTE FUNCTION rev.companies_slug_immutable_trigger();

-- Seed dapple with a fixed UUID so all environments share one identifier.
-- This UUIDv7 was generated once via psql and is now the canonical dapple ID
-- across all environments (sandbox, dev, stage, perf, prod).
INSERT INTO rev.companies (id, slug, name) VALUES
    ('019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID, 'dapple', 'Dapple');
```

**Why a fixed UUID for dapple:**
- All environments (sandbox, dev, stage, perf, prod) share the same dapple `id`. Cross-env queries, audit comparisons, and admin tooling stay deterministic.
- The dapple UUID becomes a constant that can appear in CHECK constraints later — if we ever want DB-level enforcement of the namespace-tier pairing rule (currently §10 open question), the CHECK can read `namespace_company_id = '019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID` directly without a join.
- This particular UUID was generated via `psql … -c 'SELECT uuidv7();'` on 2026-05-10 and is now the canonical dapple identifier. The implementer never invents one inline; the migration file always uses this exact value.

### 4.2 `00000000000032_company_billable_entities.sql`

```sql
CREATE TABLE rev.company_billable_entities (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    billable_entity_id UUID NOT NULL REFERENCES rev.billable_entities (id),
    company_id UUID NOT NULL REFERENCES rev.companies (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT cbe_billable_unique UNIQUE (billable_entity_id),
    CONSTRAINT cbe_company_unique UNIQUE (company_id)
);

CREATE INDEX cbe_company_idx ON rev.company_billable_entities (company_id);
```

FK target for "this org's billing." Wiring billing events through it is **deferred**; the table is landed now so metering activation later requires no migration. Shape mirrors `rev.user_billable_entities` (migration 21).

### 4.3 `00000000000033_path_layer_namespace.sql`

```sql
-- 1. Drop the unique index that depends on ref_key (it only ever protected 2 orphan rows)
DROP INDEX object_paths_ref_key_unique_idx;

-- 2. Delete the 2 orphan shared_content rows.
--    Verified at plan time: zero versioned_objects reference them and guide_shared_refs
--    is empty, so they have no live dependents.
DELETE FROM rev.object_paths
WHERE entity_type_id = (SELECT id FROM rev.entity_types WHERE key = 'shared_content')
  AND ref_key IS NOT NULL;

-- 3. Drop the vestigial ref_key column from object_paths.
--    (canonical_release_pins.ref_key stays — it carries source_path strings.)
ALTER TABLE rev.object_paths DROP COLUMN ref_key;

-- 4. Add namespace_company_id to object_paths and canonical_release_pins
ALTER TABLE rev.object_paths
    ADD COLUMN namespace_company_id UUID NULL REFERENCES rev.companies (id);

ALTER TABLE rev.canonical_release_pins
    ADD COLUMN namespace_company_id UUID NULL REFERENCES rev.companies (id);

-- 5. Backfill: every existing row belongs to the dapple namespace
UPDATE rev.object_paths
SET namespace_company_id = (SELECT id FROM rev.companies WHERE slug = 'dapple');

UPDATE rev.canonical_release_pins
SET namespace_company_id = (SELECT id FROM rev.companies WHERE slug = 'dapple');

-- 6. Backfill scope on object_paths: NULL → 'public' (regular paths)
--    Existing 'common'/'internal' values stay (they were the shared-content rows;
--    after the orphan delete in step 2 there are no scope='common'/'internal' rows
--    left, but the UPDATE is harmless if any sneak in)
UPDATE rev.object_paths SET scope = 'public' WHERE scope IS NULL;

-- canonical_release_pins.scope is empty (0 rows in this DB); nothing to backfill there.

-- 7. Tighten constraints
ALTER TABLE rev.object_paths
    ALTER COLUMN namespace_company_id SET NOT NULL,
    ALTER COLUMN scope SET NOT NULL,
    ADD CONSTRAINT object_paths_scope_format
        CHECK (scope IN ('public', 'common', 'internal', 'external'));

ALTER TABLE rev.canonical_release_pins
    ALTER COLUMN namespace_company_id SET NOT NULL,
    ADD CONSTRAINT canonical_release_pins_scope_format
        CHECK (scope IN ('public', 'common', 'internal', 'external'));

-- 8. Add new indexes
CREATE INDEX object_paths_namespace_scope_idx
    ON rev.object_paths (namespace_company_id, scope);

DROP INDEX IF EXISTS canonical_release_pins_scope_env_idx;
CREATE INDEX canonical_release_pins_namespace_scope_env_idx
    ON rev.canonical_release_pins (namespace_company_id, scope, environment, object_type, ref_key);

-- 9. Repurpose rev.shared_scopes lookup table — tier-only enum
DELETE FROM rev.shared_scopes;
INSERT INTO rev.shared_scopes (key, description) VALUES
    ('public',   'Dapple-only tier: broadly visible content within the dapple namespace'),
    ('common',   'Any-namespace tier: shared content available across guides in a namespace'),
    ('internal', 'Dapple-only tier: dapple-private content'),
    ('external', 'Customer-only tier: customer-private content hosted by dapple (mirror of internal on the customer side)');
```

The migration is small and additive aside from the `ref_key` drop and the orphan delete (both verified safe at plan time). No source_path/slug data is touched.

### 4.4 `00000000000034_external_service_tokens.sql`

```sql
CREATE TABLE rev.external_service_tokens (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    service_id TEXT NOT NULL UNIQUE,
    company_id UUID NOT NULL REFERENCES rev.companies (id),
    name TEXT NOT NULL,
    description TEXT,
    public_key_pem TEXT NOT NULL,
    key_algorithm TEXT NOT NULL DEFAULT 'Ed25519',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by_user_id BIGINT REFERENCES rev.user_details (user_id),
    CONSTRAINT external_service_id_format
        CHECK (service_id ~ '^[a-z][a-z0-9_]{2,31}$')
);

CREATE INDEX rev_external_service_tokens_company_idx
    ON rev.external_service_tokens (company_id);

CREATE TABLE rev.external_service_token_revocations (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    external_service_token_id UUID NOT NULL
        REFERENCES rev.external_service_tokens (id),
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_by_user_id BIGINT REFERENCES rev.user_details (user_id),
    reason TEXT
);
```

Existing `web.service_tokens` is unchanged. Onboarding acme is one `INSERT` into `rev.companies` and one into `rev.external_service_tokens`.

## 5. Code Touch-Points

Paths committed; function signatures deferred to implementation issues.

- **New crates:**
  - `crates/dapple-prompt-external-api/` — `Cargo.toml`, `src/lib.rs`, `src/extractor.rs` (`ExternalCaller`), `src/state.rs`, `src/route.rs`.
  - `crates/dapple-prompt-user-api/` — reserved stub, `Cargo.toml` + `src/lib.rs` placeholder.
  - `crates/dapple-prompt-server/` — main binary with `--surface` arg parsing and per-surface bootstrap.
  - `crates/dapple-external-sdk/` — `Cargo.toml`, `src/lib.rs`. Thin HTTP client.
- **Companies entity:** `crates/dapple-data/src/model/companies.rs`, `src/query/companies.rs`, `assets/sqlc/companies/*.sqlc`.
- **External-service-tokens entity:** `crates/dapple-data/src/model/external_service_tokens.rs`, `src/query/external_service_tokens.rs`, `assets/sqlc/external_service_tokens/*.sqlc`.
- **Path-layer model updates:** `crates/dapple-data/src/model/object_paths.rs` and `canonical_pins.rs` gain the `namespace_company_id: Uuid` field; scope becomes `String` (not `Option<String>`) and matches a tighter enum. The `ref_key` field is removed from the `object_paths` model (kept on `canonical_pins` model since that table retains it).
- **External guides page:** `crates/dapple-data/assets/sqlc/guides/pages/by_external_caller.sqlc` (new variant, namespace-keyed).
- **Internal API change:** small but real. After Migration 33, the dapple-team rows have `namespace_company_id = (dapple's UUID)` and `scope` is `'public'`/`'common'`/`'internal'` (no NULLs). Any existing Rust code or `.sqlc` file that matches on `scope IS NULL` for regular paths must be updated to `scope = 'public'`. Any code that referenced `object_paths.ref_key` directly must be removed (that field is gone). Audit at Issue 1 implementation time.

## 6. Migration Ordering

Migrations land in numeric order:

1. **31** — `companies` (no DB dependencies; foundation for 32, 33, 34).
2. **32** — `company_billable_entities` (depends on 31 and on existing `billable_entities`).
3. **33** — `path_layer_namespace` (depends on 31; deletes 2 orphan shared-content rows, drops vestigial `object_paths.ref_key`, backfills `namespace_company_id` and `scope` to dapple defaults, tightens constraints).
4. **34** — `external_service_tokens` (depends on 31).

Migration 33 is small in surface but does three notable things: (1) drops `object_paths.ref_key` (verified vestigial via SQL inventory at plan time), (2) deletes 2 orphan shared-content rows (verified to have zero live dependents), (3) adds `namespace_company_id` to both `object_paths` and `canonical_release_pins` and tightens `scope`. No `source_path` or `slug` data on entity tables is touched. Once 34 lands, the schema is fully ready; the next implementation issue is the `dapple-prompt-server` binary skeleton plus the external router scaffolding (no further DDL).

## 7. Out of Scope

- **Endpoint specs** beyond the V1 narrow surface (list-guides, create-journey, get-briefs, post-bindings, create-dispatch, poll-dispatch).
- **`dapple-external-sdk` module layout.** Confirmed only as: thin HTTP client, no `dapple-data`/`dapple-prompt-api`/`dapple-prompt-external-api` dependency.
- **External-API book chapter outline.**
- **User API surface (`user-api.dapple.x`).** Crate is reserved; not built in V1.
- **Members / teams / departments / company_parents.**
- **Billing wiring through `company_billable_entities`** — table exists; metering activation is a separate issue.
- **Partner scope** (V2+). When it lands: extend the scope CHECK to allow `'partner'`, plus add `partner_consumer_company_id UUID NULL REFERENCES rev.companies(id)` to `rev.object_paths` and `rev.canonical_release_pins`, set only when `scope = 'partner'`. This is a self-contained future migration.
- **TUI changes** (`canonical/scopes.toml`, prefix→manifest decoupling) — tracked in Replacement B of the prior review.
- **Per-org classification lookup tables** — user-locked as free string for V1.
- **Variant pins for external orgs** (`rev.canonical_variant_pins`).
- **First-class directories on paths.** Not a V1 concern; explicitly rejected in favor of "namespace+scope on object_paths, identifiers on entity tables."
- **Shared-content identifier system.** The `_shared/` content reference system is not actively populated today (`guide_shared_refs` empty, `object_paths.ref_key` carried only orphans). When shared content is wired up — including the acme-shared-content use case — its identifier needs a proper home (likely a new `rev.shared_content_path_details` table mirroring the other 16 path-detail tables). Out of scope for this plan; design it when shared content is real functionality.
- **Per-entity unique constraints on `(namespace_company_id, source_path)` or `(namespace_company_id, slug)`.** V1 relies on Pattern B's structural router separation for hostile-collision protection. Per-entity DB-level uniqueness constraints can be added later if a real mistake lands.

## 8. Verification and Implementation-Issue Split

This plan is "done" when:

1. The four migration sketches in §4 can be turned into real migration files without further design decisions — column names, types, FKs, CHECKs, and indexes are committed.
2. The SQL filter pattern in §3 Decision 5 is reproducible verbatim by an implementation agent reading this plan.
3. Decisions 1–6 each have a stated answer that the user can agree or push back on; rationale is on the record.
4. The implementation agent can split the work into ordered issues:
   - **Issue 1:** Migrations 31–34 + companies model/query + external-service-tokens model/query + ref_key inventory and transformation rule + audit of internal-handler call sites that match on `scope IS NULL` for regular guide paths (those become `scope = 'public'`).
   - **Issue 2:** External JWT-claim shape, separate signing-key load path, surface-aware token-exchange handler.
   - **Issue 3:** `dapple-prompt-server` binary with `--surface` arg + external router skeleton + `ExternalCaller` extractor + 401 negative test for cross-surface token replay.
   - **Issue 4:** External list-guides endpoint (proves the SQL filter end-to-end).
   - **Issue 5:** Remaining V1 endpoints (journey/briefs/dispatch/poll).
   - **Issue 6:** `dapple-external-sdk` skeleton.

## 9. AGENTS.md Updates Called For

Surgical additions only.

- **`crates/dapple-data/AGENTS.md`** — Under "Schema Organization," document `rev.companies` as the canonical org primitive and the `namespace_company_id UUID FK` on `rev.object_paths` and `rev.canonical_release_pins`. Note: scope is a flat enum (`public`/`common`/`internal`, future `partner`) — orgs are *not* embedded in scope. ref_keys are flat (no `/`).
- **`crates/dapple-prompt-api/AGENTS.md`** — Add a "Single-Tenant Posture" callout: the internal API does NOT filter queries by namespace; external-tier consumers go through `dapple-prompt-external-api` over `external-api.dapple.x`. Anti-pattern: caller-scope filters on internal handlers.
- **`docs/books/_foundation/authentication/service-tokens.md`** — Restructure into two sibling sections (or split into `service-tokens.md` + `external-service-tokens.md`). Document: separate tables (`web.service_tokens` vs. `rev.external_service_tokens`), separate issuance endpoints, separate signing keys per surface, `iss` claim values, JWT-replay impossibility across surfaces.
- **`assets/k8s/AGENTS.md`** — Document the three-surface deployment topology: `--surface={internal,external,user}` flag, port-per-surface binding, separate Deployments per surface, separate NetworkPolicies, ingress hostname → Deployment mapping.

## 10. Open Questions and Risks

- **Namespace-tier pairing enforcement at the DB layer.** Pattern B's structural router separation already prevents the wrong scope appearing in the wrong namespace (the external router never produces `internal`, the internal router never produces `external`, and key-separated JWTs make cross-router replay impossible). Should we *also* enforce the pairing rule at the DB level? With dapple's fixed UUID hardcoded in DDL (Migration 31), this is a simple CHECK constraint, not a trigger:

  ```sql
  CHECK (
      (namespace_company_id = '019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID
          AND scope IN ('public', 'common', 'internal'))
      OR
      (namespace_company_id <> '019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID
          AND scope IN ('common', 'external'))
  )
  ```

  Pro: defense-in-depth, no per-row trigger overhead, no join. Con: hardcodes dapple's UUID into the CHECK (acceptable because it IS hardcoded by Migration 31 anyway). Default: skip for V1; the CHECK is cheap to add later if a real mistake lands. Resolve at Issue 1.
- **`rev.shared_scopes`: shrink to tier-enum or drop entirely?** Both work. Shrinking keeps a queryable enumeration; dropping relies on CHECK regex. Pick at Migration-33 write time.
- **Internal-handler call sites that match on legacy scope strings.** Any Rust code or `.sqlc` file that compares `scope IS NULL` for regular guide paths needs updating to `scope = 'public'` during Issue 1. Catalog before writing the migration.
- **Slug immutability enforcement mechanism.** Recommended: trigger (sketched in §4.1). Alternative: app-layer convention only. Pick at write time; trigger is recommended for defense-in-depth.
- **Company onboarding flow.** V1 uses manual `INSERT` by an admin (or one-off CLI / TUI). A `POST /api/companies` admin endpoint comes later, gated on org admin work.
- **External token-issuance endpoint hostname.** External clients call `external-api.dapple.x/api/get-service-token` to exchange a signed token for a JWT. The handler exists only on the external surface. The signing key for *issuance* (server side) is the same as for *verification* on that surface. Confirm at Issue 2.
