-- =============================================================================
-- Companies as first-class actors on journeys + dispatches
-- =============================================================================
-- Every journey and dispatch is owned by a company. Internal (dapple-team)
-- flows belong to dapple; external (customer) flows belong to the customer.
-- This mirrors how `rev.object_paths.namespace_company_id` already works,
-- making "companies own things" the consistent multi-tenancy primitive.
--
-- user_id becomes nullable: human-initiated flows (dapple-ui delegated
-- tokens) keep setting it; service-initiated flows (external API)
-- leave it NULL.
--
-- The DEFAULT-then-DROP DEFAULT pattern backfills existing rows in one
-- ALTER (Postgres 11+ optimizes constant defaults to a fast metadata
-- update) and then forces all future inserts to explicitly set the column,
-- avoiding silent fall-through to dapple if a handler forgets.
-- =============================================================================

-- ---------------------------------------------------------------------------
-- 1. journeys
-- ---------------------------------------------------------------------------

ALTER TABLE rev.journeys
    ADD COLUMN owner_company_id UUID NOT NULL
        DEFAULT '019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID
        REFERENCES rev.companies (id),
    ALTER COLUMN user_id DROP NOT NULL;

ALTER TABLE rev.journeys ALTER COLUMN owner_company_id DROP DEFAULT;

CREATE INDEX journeys_owner_company_idx ON rev.journeys (owner_company_id);

-- ---------------------------------------------------------------------------
-- 2. dispatches
-- ---------------------------------------------------------------------------

ALTER TABLE rev.dispatches
    ADD COLUMN owner_company_id UUID NOT NULL
        DEFAULT '019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID
        REFERENCES rev.companies (id),
    ALTER COLUMN user_id DROP NOT NULL;

ALTER TABLE rev.dispatches ALTER COLUMN owner_company_id DROP DEFAULT;

CREATE INDEX dispatches_owner_company_idx ON rev.dispatches (owner_company_id);
