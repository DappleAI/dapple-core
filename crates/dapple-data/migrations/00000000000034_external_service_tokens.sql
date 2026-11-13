-- =============================================================================
-- External Service Tokens — customer-tier auth
-- =============================================================================
-- Customer-owned Ed25519 service tokens for the external API surface
-- (external-api.dapple.x). Lives in rev (admin/billing schema) rather than
-- web (session/auth schema) because these are TUI-managed admin artifacts
-- tied to companies, not web-app session data.
--
-- web.service_tokens (the internal-tier table from migration 5) is
-- UNCHANGED — internal and external tiers never share a row, an issuance
-- endpoint, or a JWT signing key.
--
-- See docs/plans/external-api/00-architecture.md §3 Decision 2 and §4.4.
-- See docs/books/_foundation/authentication/external-service-tokens.md
-- for the full auth model.
-- =============================================================================

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

-- ---------------------------------------------------------------------------
-- Revocation log
-- ---------------------------------------------------------------------------
-- The extractor refuses tokens whose external_service_token_id appears here.
-- Revoking an entire customer (all their tokens at once) is done by setting
-- rev.companies.revoked_at; revoking an individual token uses this table.

CREATE TABLE rev.external_service_token_revocations (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    external_service_token_id UUID NOT NULL
        REFERENCES rev.external_service_tokens (id),
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_by_user_id BIGINT REFERENCES rev.user_details (user_id),
    reason TEXT
);

CREATE INDEX external_service_token_revocations_token_idx
    ON rev.external_service_token_revocations (external_service_token_id);
