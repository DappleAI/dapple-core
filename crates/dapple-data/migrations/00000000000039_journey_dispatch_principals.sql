-- =============================================================================
-- journeys/dispatches: add principal_id, subject_principal_id,
-- billable_entity_id
-- =============================================================================
-- Wires the principals layer (Mig 37) and the billing layer (Mig 38) into
-- journeys and dispatches. Three new columns on each table:
--
--   principal_id          UUID NOT NULL — who issued the request (the actor)
--   subject_principal_id  UUID NULL     — on whose behalf (NULL = self)
--   billable_entity_id    UUID NULL     — who pays (resolved at create time)
--
-- The existing user_id BIGINT and owner_company_id UUID columns stay:
--   - owner_company_id is the denormalized namespace handle (= principal's
--     owner_company_id); unchanged by this migration.
--   - user_id stays for one release as a deprecation window. Handlers
--     switch to writing principal_id; readers migrate; a future migration
--     drops user_id from both tables.
--
-- billable_entity_id stays NULLABLE in V1: many existing users don't have
-- a billable_entity row yet (billing onboarding isn't fully wired). When
-- onboarding always creates one, a future migration tightens it.
--
-- See docs/plans/principals-and-billing/00-architecture.md §3 Decision 3.
-- =============================================================================

-- ---------------------------------------------------------------------------
-- 1. journeys
-- ---------------------------------------------------------------------------

ALTER TABLE rev.journeys
    ADD COLUMN principal_id UUID NULL REFERENCES rev.principals (id),
    ADD COLUMN subject_principal_id UUID NULL REFERENCES rev.principals (id),
    ADD COLUMN billable_entity_id UUID NULL REFERENCES rev.billable_entities (id);

-- Backfill principal_id from user_id via the human_principals lookup.
-- Every existing journey has user_id BIGINT FK to user_details, and every
-- user_details row has a corresponding human_principal after Mig 37's
-- backfill — so this is total for human journeys.
--
-- For any service-user journeys (none expected in pre-commit dev DBs since
-- the old Mig 37 was uncommitted), this would leave principal_id NULL and
-- the SET NOT NULL below would fail loud. That's intentional — production
-- pre-existing service journeys would need a custom one-off backfill, but
-- that scenario doesn't apply to current state.

UPDATE rev.journeys j
SET principal_id = hp.principal_id
FROM rev.human_principals hp
WHERE hp.user_id = j.user_id;

-- Backfill billable_entity_id from existing user_billable_entities for old
-- rows that have one. Many will be NULL — that's fine.

UPDATE rev.journeys j
SET billable_entity_id = ube.billable_entity_id
FROM rev.user_billable_entities ube
WHERE ube.user_id = j.user_id;

ALTER TABLE rev.journeys ALTER COLUMN principal_id SET NOT NULL;

CREATE INDEX journeys_principal_idx ON rev.journeys (principal_id);
CREATE INDEX journeys_subject_principal_idx
    ON rev.journeys (subject_principal_id)
    WHERE subject_principal_id IS NOT NULL;
CREATE INDEX journeys_billable_entity_idx
    ON rev.journeys (billable_entity_id)
    WHERE billable_entity_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- 2. dispatches
-- ---------------------------------------------------------------------------

ALTER TABLE rev.dispatches
    ADD COLUMN principal_id UUID NULL REFERENCES rev.principals (id),
    ADD COLUMN subject_principal_id UUID NULL REFERENCES rev.principals (id),
    ADD COLUMN billable_entity_id UUID NULL REFERENCES rev.billable_entities (id);

-- Inherit all three columns from the parent journey at backfill time.
-- This matches the runtime semantics: dispatch insertion will copy these
-- from the parent journey via JOIN in the .sqlc.

UPDATE rev.dispatches d
SET principal_id = j.principal_id,
    subject_principal_id = j.subject_principal_id,
    billable_entity_id = j.billable_entity_id
FROM rev.journeys j
WHERE j.id = d.journey_id;

ALTER TABLE rev.dispatches ALTER COLUMN principal_id SET NOT NULL;

CREATE INDEX dispatches_principal_idx ON rev.dispatches (principal_id);
CREATE INDEX dispatches_subject_principal_idx
    ON rev.dispatches (subject_principal_id)
    WHERE subject_principal_id IS NOT NULL;
CREATE INDEX dispatches_billable_entity_idx
    ON rev.dispatches (billable_entity_id)
    WHERE billable_entity_id IS NOT NULL;
