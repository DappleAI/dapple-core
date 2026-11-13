-- =============================================================================
-- Principals: actor primitive for journeys/dispatches/audit
-- =============================================================================
-- Replaces the in-progress service-user design (the previous Migration 37,
-- which has been deleted before commit). That design tacked a `user_kind`
-- discriminator + nullable `external_service_token_id` onto rev.user_details
-- and dropped the auth.users FK — two structural smells the auditor book
-- already rejected at other layers.
--
-- This migration introduces a first-class actor primitive: `rev.principals`,
-- a base table with four per-kind extension tables (class-table-inheritance,
-- the same pattern Mig 21 already proved for billable_entities). Every
-- actor that journeys/dispatches reference is a row in `rev.principals` plus
-- exactly one row in one of the detail tables.
--
-- Four actor shapes the model supports:
--   A — Human user                              (human_principals)
--   B — Internal service on behalf of a user    (internal_service_principals)
--   C — External service (acting as itself)     (external_service_principals)
--   D — External service on behalf of a subject (external_subject_principals)
--
-- Subsequent migrations:
--   38 — rev.principal_billable_entities  (generalizes user_billable_entities)
--   39 — journeys/dispatches gain principal_id / subject_principal_id /
--        billable_entity_id columns
--
-- See docs/plans/principals-and-billing/00-architecture.md.
--
-- DEV-DB NOTE: If you had previously applied the deleted old Migration 37,
-- reset your DB before running migrations: `dropdb dapple && createdb dapple`
-- (or equivalent) and re-run `cargo sqlx migrate run`. The fresh path assumes
-- post-Mig-36 state: user_details has no `user_kind` column, journeys/
-- dispatches.user_id is NULLABLE.
-- =============================================================================

-- ---------------------------------------------------------------------------
-- 1. rev.principals — the actor base table
-- ---------------------------------------------------------------------------
-- One row per actor. The "kind" is implied by which detail table holds the
-- join — no discriminator column on this table.

CREATE TABLE rev.principals (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    owner_company_id UUID NOT NULL REFERENCES rev.companies (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ NULL
);

CREATE INDEX principals_owner_company_idx ON rev.principals (owner_company_id);

-- ---------------------------------------------------------------------------
-- 2. Detail tables (1:1 with principals.id, one per actor kind)
-- ---------------------------------------------------------------------------

-- Shape A: human user (NextAuth-backed).
CREATE TABLE rev.human_principals (
    principal_id UUID PRIMARY KEY REFERENCES rev.principals (id),
    user_id BIGINT NOT NULL UNIQUE REFERENCES rev.user_details (user_id)
);

CREATE INDEX human_principals_user_idx ON rev.human_principals (user_id);

-- Shape B: internal service (dapple-ui, conduit, flow, lago-worker).
-- 1:1 with web.service_tokens. Lands now for symmetry; the V1 internal
-- API handlers don't yet thread service-principal IDs through journeys
-- (that's a later project), but the schema accommodates it without
-- another migration.
CREATE TABLE rev.internal_service_principals (
    principal_id UUID PRIMARY KEY REFERENCES rev.principals (id),
    service_token_id UUID NOT NULL UNIQUE REFERENCES web.service_tokens (id)
);

CREATE INDEX internal_service_principals_token_idx
    ON rev.internal_service_principals (service_token_id);

-- Shape C: external service (customer companies authenticating with their
-- own Ed25519 tokens via dapple-prompt-external-api).
CREATE TABLE rev.external_service_principals (
    principal_id UUID PRIMARY KEY REFERENCES rev.principals (id),
    external_service_token_id UUID NOT NULL UNIQUE REFERENCES rev.external_service_tokens (id)
);

CREATE INDEX external_service_principals_token_idx
    ON rev.external_service_principals (external_service_token_id);

-- Shape D: external subject — a user identity inside a customer's own
-- system (e.g. one of a customer's own end-customers). Lazy-created on first reference
-- by the external API. `external_key` is opaque to dapple; the customer is
-- the source of truth for what it identifies.
CREATE TABLE rev.external_subject_principals (
    principal_id UUID PRIMARY KEY REFERENCES rev.principals (id),
    parent_principal_id UUID NOT NULL REFERENCES rev.principals (id),
    external_key TEXT NOT NULL,
    UNIQUE (parent_principal_id, external_key)
);

CREATE INDEX external_subject_principals_parent_idx
    ON rev.external_subject_principals (parent_principal_id);

-- ---------------------------------------------------------------------------
-- 3. Backfill: every existing rev.user_details row → human_principal
-- ---------------------------------------------------------------------------
-- Existing user_details rows are all NextAuth-backed humans (we're on
-- post-Mig-36 state; there are no service-kind rows). Each becomes a
-- principal in dapple's namespace plus a human_principals link.
--
-- Use a CTE so the new principal rows can be correlated back to the
-- originating user_details by ordinal position (UUIDv7 gen order matches
-- the insert order under ORDER BY).

WITH ordered_users AS (
    SELECT user_id, row_number() OVER (ORDER BY user_id) AS ord
    FROM rev.user_details
),
new_principals AS (
    INSERT INTO rev.principals (owner_company_id)
    SELECT '019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID
    FROM ordered_users
    ORDER BY ord
    RETURNING id
),
numbered_principals AS (
    SELECT id AS principal_id, row_number() OVER (ORDER BY id) AS ord
    FROM new_principals
)
INSERT INTO rev.human_principals (principal_id, user_id)
SELECT np.principal_id, ou.user_id
FROM numbered_principals np
JOIN ordered_users ou ON ou.ord = np.ord;

-- ---------------------------------------------------------------------------
-- 4. Backfill: every existing rev.external_service_tokens row →
--    external_service_principal
-- ---------------------------------------------------------------------------
-- Mirrors the human-principals backfill; uses the token row's company_id
-- as the principal's owner_company_id.

WITH ordered_tokens AS (
    SELECT id AS token_id, company_id, row_number() OVER (ORDER BY id) AS ord
    FROM rev.external_service_tokens
),
new_principals AS (
    INSERT INTO rev.principals (owner_company_id)
    SELECT company_id FROM ordered_tokens ORDER BY ord
    RETURNING id
),
numbered_principals AS (
    SELECT id AS principal_id, row_number() OVER (ORDER BY id) AS ord
    FROM new_principals
)
INSERT INTO rev.external_service_principals (principal_id, external_service_token_id)
SELECT np.principal_id, ot.token_id
FROM numbered_principals np
JOIN ordered_tokens ot ON ot.ord = np.ord;

-- ---------------------------------------------------------------------------
-- 5. Auto-create trigger: every new rev.external_service_tokens row gets
--    a matching principal + external_service_principal
-- ---------------------------------------------------------------------------
-- Replaces the deleted old Migration 37's
-- `create_service_user_for_external_token` trigger. Same intent: customer
-- onboarding (insert into external_service_tokens) is atomic with creating
-- the actor identity dapple references for that customer.

CREATE OR REPLACE FUNCTION rev.create_principal_for_external_service_token()
    RETURNS TRIGGER AS $$
DECLARE
    new_principal_id UUID;
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
