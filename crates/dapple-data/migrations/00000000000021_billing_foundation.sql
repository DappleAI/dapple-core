-- =============================================================================
-- Billing Foundation for Lago Integration
-- =============================================================================
-- Introduces resource tracking, usage detail, correction, and Lago sync tables
-- to support metered billing via Lago.
-- =============================================================================

-- ---------------------------------------------------------------------------
-- 1. Lookup Tables
-- ---------------------------------------------------------------------------

CREATE TABLE rev.resource_types (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE
);

INSERT INTO rev.resource_types (key) VALUES ('llm_usage'), ('asset'), ('service_call');

CREATE TABLE rev.integration_services (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    display_name TEXT
);

INSERT INTO rev.integration_services (key, display_name) VALUES
    ('zapier', 'Zapier'),
    ('sendgrid', 'SendGrid'),
    ('twilio', 'Twilio');

-- ---------------------------------------------------------------------------
-- 2. Billable Entities
-- ---------------------------------------------------------------------------

CREATE TABLE rev.billable_entities (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    lago_customer_external_id TEXT,
    lago_subscription_external_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX billable_entities_lago_customer_idx
    ON rev.billable_entities (lago_customer_external_id)
    WHERE lago_customer_external_id IS NOT NULL;

CREATE TABLE rev.user_billable_entities (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    billable_entity_id UUID NOT NULL REFERENCES rev.billable_entities(id),
    user_id BIGINT NOT NULL REFERENCES rev.user_details(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT user_billable_entities_be_unique UNIQUE (billable_entity_id),
    CONSTRAINT user_billable_entities_user_unique UNIQUE (user_id)
);

CREATE INDEX user_billable_entities_user_idx
    ON rev.user_billable_entities (user_id);

-- ---------------------------------------------------------------------------
-- 3. Resource Claims
-- ---------------------------------------------------------------------------

CREATE TABLE rev.resource_claims (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    dispatch_task_attempt_id UUID,
    correlation_id UUID,
    claim_data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT valid_claim_data CHECK (jsonb_typeof(claim_data) = 'object')
);

CREATE INDEX resource_claims_attempt_idx
    ON rev.resource_claims (dispatch_task_attempt_id);

CREATE INDEX resource_claims_correlation_idx
    ON rev.resource_claims (correlation_id);

-- ---------------------------------------------------------------------------
-- 4. Usage Detail Tables
-- ---------------------------------------------------------------------------

-- LLM token usage per dispatch task attempt event
CREATE TABLE rev.dispatch_task_attempt_event_llm_usages (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    event_id UUID NOT NULL,
    claim_id UUID REFERENCES rev.resource_claims(id),
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    provider_input_tokens INTEGER,
    provider_output_tokens INTEGER,
    provider_id SMALLINT NOT NULL REFERENCES rev.providers(id),
    model_release_id UUID NOT NULL REFERENCES rev.model_releases(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT positive_tokens CHECK (input_tokens >= 0 AND output_tokens >= 0)
);

CREATE INDEX llm_usages_event_idx
    ON rev.dispatch_task_attempt_event_llm_usages (event_id);

-- Asset usage per dispatch task attempt event
CREATE TABLE rev.dispatch_task_attempt_event_assets (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    event_id UUID NOT NULL,
    claim_id UUID REFERENCES rev.resource_claims(id),
    asset_kind_id SMALLINT NOT NULL REFERENCES rev.asset_kinds(id),
    size_bytes BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT positive_size CHECK (size_bytes >= 0)
);

CREATE INDEX asset_usages_event_idx
    ON rev.dispatch_task_attempt_event_assets (event_id);

-- External service call usage per dispatch task attempt event
CREATE TABLE rev.dispatch_task_attempt_event_service_calls (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    event_id UUID NOT NULL,
    claim_id UUID REFERENCES rev.resource_claims(id),
    integration_service_id BIGINT NOT NULL REFERENCES rev.integration_services(id),
    endpoint TEXT,
    units INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT positive_units CHECK (units > 0)
);

CREATE INDEX service_calls_event_idx
    ON rev.dispatch_task_attempt_event_service_calls (event_id);

-- ---------------------------------------------------------------------------
-- 5. Correction Tables
-- ---------------------------------------------------------------------------

-- Void a previously recorded resource usage (append-only audit trail)
CREATE TABLE rev.resource_voids (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    original_id UUID NOT NULL,
    reason TEXT NOT NULL,
    issue_reference TEXT NOT NULL,
    voided_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Amend a previously recorded resource usage (append-only audit trail)
CREATE TABLE rev.resource_amendments (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    event_id UUID NOT NULL,
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    void_id UUID REFERENCES rev.resource_voids(id),
    amendment_data JSONB NOT NULL,
    reason TEXT NOT NULL,
    issue_reference TEXT NOT NULL,
    amended_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT valid_amendment_data CHECK (jsonb_typeof(amendment_data) = 'object')
);

-- ---------------------------------------------------------------------------
-- 6. Lago Event Sync
-- ---------------------------------------------------------------------------

CREATE TABLE rev.lago_event_sync (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    resource_id UUID NOT NULL,
    lago_transaction_id UUID NOT NULL UNIQUE,
    sent_at TIMESTAMPTZ,
    lago_response_status INTEGER,
    lago_response_body JSONB,
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX lago_event_sync_resource_idx
    ON rev.lago_event_sync (resource_type_id, resource_id);

CREATE INDEX lago_event_sync_pending_idx
    ON rev.lago_event_sync (created_at)
    WHERE sent_at IS NULL;
