-- Migration 5: User layer — auth tables, user_details, roles, tokens
-- auth.* tables keep BIGINT PKs (NextAuth compatibility)

-- ============================================================================
-- auth.users (NextAuth — BIGINT PK)
-- ============================================================================
CREATE TABLE auth.users (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name TEXT,
    email TEXT,
    email_verified TIMESTAMPTZ,
    image TEXT
);

ALTER TABLE auth.users OWNER TO dapple;

-- ============================================================================
-- auth.accounts (NextAuth — BIGINT PK)
-- ============================================================================
CREATE TABLE auth.accounts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id BIGINT NOT NULL,
    type TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_account_id TEXT NOT NULL,
    refresh_token TEXT,
    access_token TEXT,
    expires_at BIGINT,
    id_token TEXT,
    scope TEXT,
    session_state TEXT,
    token_type TEXT
);

ALTER TABLE auth.accounts OWNER TO dapple;

-- ============================================================================
-- auth.sessions (NextAuth — BIGINT PK)
-- ============================================================================
CREATE TABLE auth.sessions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id BIGINT NOT NULL,
    expires TIMESTAMPTZ NOT NULL,
    session_token TEXT NOT NULL
);

ALTER TABLE auth.sessions OWNER TO dapple;

-- ============================================================================
-- auth.verification_token (NextAuth — composite PK)
-- ============================================================================
CREATE TABLE auth.verification_token (
    identifier TEXT NOT NULL,
    expires TIMESTAMPTZ NOT NULL,
    token TEXT NOT NULL,
    PRIMARY KEY (identifier, token)
);

ALTER TABLE auth.verification_token OWNER TO dapple;

-- ============================================================================
-- rev.user_details — Bridge between auth.users and rev schema
-- user_id is BIGINT FK to auth.users (which keeps BIGINT PK)
-- ============================================================================
CREATE TABLE rev.user_details (
    user_id BIGINT NOT NULL PRIMARY KEY REFERENCES auth.users,
    object_id UUID DEFAULT uuidv7() NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

-- ============================================================================
-- web.user_roles — User role assignments
-- ============================================================================
CREATE TABLE web.user_roles (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES rev.user_details (user_id),
    role TEXT NOT NULL REFERENCES web.role_types (role),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, role)
);

CREATE INDEX user_roles_user_idx ON web.user_roles (user_id);
CREATE INDEX user_roles_role_idx ON web.user_roles (role);

-- ============================================================================
-- web.api_tokens — Programmatic API access (prefix: dpl_)
-- ============================================================================
CREATE TABLE web.api_tokens (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES rev.user_details (user_id),
    name TEXT NOT NULL CHECK (LENGTH(name) <= 255),
    token_hash BYTEA NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL UNIQUE CHECK (LENGTH(token_prefix) = 8),
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX api_tokens_user_idx ON web.api_tokens (user_id);
CREATE INDEX api_tokens_token_hash_idx ON web.api_tokens (token_hash);

-- ============================================================================
-- web.api_token_revocations — Append-only audit log
-- ============================================================================
CREATE TABLE web.api_token_revocations (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    api_token_id UUID NOT NULL REFERENCES web.api_tokens (id),
    reason TEXT,
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX api_token_revocations_api_token_idx ON web.api_token_revocations (api_token_id);

-- ============================================================================
-- web.admin_api_tokens — Sparse table (presence = admin privileges)
-- ============================================================================
CREATE TABLE web.admin_api_tokens (
    api_token_id UUID PRIMARY KEY REFERENCES web.api_tokens (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================================
-- web.service_tokens — Service-to-service asymmetric key auth (prefix: dps_)
-- ============================================================================
CREATE TABLE web.service_tokens (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    service_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL CHECK (LENGTH(name) <= 255),
    description TEXT,
    public_key_pem TEXT NOT NULL,
    key_algorithm TEXT NOT NULL DEFAULT 'Ed25519' CHECK (key_algorithm IN ('Ed25519', 'ES256')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by_user_id BIGINT REFERENCES rev.user_details (user_id),
    CONSTRAINT service_id_format CHECK (service_id ~ '^[a-z][a-z0-9_]{2,31}$')
);

CREATE INDEX service_tokens_service_id_idx ON web.service_tokens (service_id);

-- ============================================================================
-- web.service_token_revocations — Append-only audit log
-- ============================================================================
CREATE TABLE web.service_token_revocations (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    service_token_id UUID NOT NULL REFERENCES web.service_tokens (id),
    reason TEXT,
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX service_token_revocations_token_idx ON web.service_token_revocations (service_token_id);

-- ============================================================================
-- web.service_token_usage — Usage audit log
-- ============================================================================
CREATE TABLE web.service_token_usage (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    service_token_id UUID NOT NULL REFERENCES web.service_tokens (id),
    used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    source_ip INET,
    endpoint_called TEXT
);

CREATE INDEX service_token_usage_token_idx ON web.service_token_usage (service_token_id);
CREATE INDEX service_token_usage_used_at_idx ON web.service_token_usage (used_at);
