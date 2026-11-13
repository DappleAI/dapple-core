-- =============================================================================
-- Companies — canonical org primitive
-- =============================================================================
-- Foundation for the multi-surface API split: every path-bearing entity is
-- owned by a row in rev.companies, and every external-tier service token is
-- bound to one. The dapple row uses a fixed UUID so all environments share
-- the same canonical identifier.
--
-- Slug is immutable (enforced by trigger): a "rename" is a new row + an
-- explicit data migration, never an UPDATE.
--
-- See docs/plans/external-api/00-architecture.md §3 Decision 1 and §4.1.
-- =============================================================================

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

-- ---------------------------------------------------------------------------
-- Slug immutability
-- ---------------------------------------------------------------------------
-- The slug appears in JWT claim payloads, audit logs, TUI commands, and admin
-- inspection. Once a customer is onboarded, their slug becomes embedded in
-- external systems. Mutability would silently break those references.

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

-- ---------------------------------------------------------------------------
-- Seed: dapple
-- ---------------------------------------------------------------------------
-- Fixed UUID generated via `psql -c 'SELECT uuidv7();'` on 2026-05-10.
-- This UUID is the canonical dapple identifier across all environments
-- (sandbox, dev, stage, perf, prod). It is also exported as
-- $env.DAPPLE_COMPANY_UUID by the nushell environment overlay.

INSERT INTO rev.companies (id, slug, name) VALUES
    ('019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID, 'dapple', 'Dapple');
