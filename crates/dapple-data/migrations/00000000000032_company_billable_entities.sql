-- =============================================================================
-- Company → Billable Entity association
-- =============================================================================
-- Class-table-inheritance partner to rev.user_billable_entities (migration 21).
-- Each company that bills (e.g. an external customer like acme) gets one
-- billable_entity association so its Lago customer/subscription IDs can be
-- looked up via rev.billable_entities.
--
-- V1 lands the table only; metering activation through it is deferred. The
-- shape mirrors rev.user_billable_entities so the two integrate uniformly
-- when the org-aware billing pipeline ships.
--
-- See docs/plans/external-api/00-architecture.md §3 Decision 1 and §4.2.
-- =============================================================================

CREATE TABLE rev.company_billable_entities (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    billable_entity_id UUID NOT NULL REFERENCES rev.billable_entities (id),
    company_id UUID NOT NULL REFERENCES rev.companies (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT cbe_billable_unique UNIQUE (billable_entity_id),
    CONSTRAINT cbe_company_unique UNIQUE (company_id)
);

CREATE INDEX cbe_company_idx ON rev.company_billable_entities (company_id);
