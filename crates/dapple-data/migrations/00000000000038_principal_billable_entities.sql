-- =============================================================================
-- principal_billable_entities: generalize user_billable_entities to all
-- principal kinds
-- =============================================================================
-- Migration 21 introduced `rev.user_billable_entities` (1:1 link between a
-- `rev.user_details.user_id` BIGINT and a `rev.billable_entities.id`).
-- Migration 32 added the analogous `rev.company_billable_entities` for
-- company-owned billing.
--
-- With the principals layer in place (Migration 37), the human-specific
-- mapping is replaced by a principal-keyed table that works for any actor
-- kind. In practice:
--   - Human principals get a PBE for personal billing.
--   - External service principals get a PBE for customer billing (dapple
--     bills acme; acme can also have a company-level CBE via
--     Migration 32, but a PBE on the service principal itself works for
--     V1's single-token-per-customer flow).
--   - Internal service / external subject principals typically do NOT get
--     a PBE — they attribute billing through the journey's subject or
--     parent principal.
--
-- `rev.user_billable_entities` is kept for one release as a deprecation
-- window — a future migration will drop it once all readers switch over.
--
-- See docs/plans/principals-and-billing/00-architecture.md §3 Decision 2.
-- =============================================================================

CREATE TABLE rev.principal_billable_entities (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    billable_entity_id UUID NOT NULL UNIQUE REFERENCES rev.billable_entities (id),
    principal_id UUID NOT NULL UNIQUE REFERENCES rev.principals (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX principal_billable_entities_principal_idx
    ON rev.principal_billable_entities (principal_id);

-- ---------------------------------------------------------------------------
-- Backfill from rev.user_billable_entities
-- ---------------------------------------------------------------------------
-- Every existing user_billable_entities row maps cleanly to a
-- principal_billable_entities row via the human_principals lookup:
--   user_billable_entities.user_id (BIGINT)
--     → human_principals.user_id (BIGINT)
--     → human_principals.principal_id (UUID)
--
-- Mig 37's human-principals backfill guarantees a human_principal row for
-- every user_details row, and user_billable_entities.user_id FKs to
-- user_details(user_id), so the JOIN is total.

INSERT INTO rev.principal_billable_entities (billable_entity_id, principal_id)
SELECT ube.billable_entity_id, hp.principal_id
FROM rev.user_billable_entities ube
JOIN rev.human_principals hp ON hp.user_id = ube.user_id;
