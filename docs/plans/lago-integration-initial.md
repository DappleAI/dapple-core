# Lago Integration — Initial Foundation

> **Implementation Step 0:** Write this plan to `docs/plans/lago-integration-initial.md`

## Context

The dapple-prompt system needs billing integration via self-hosted Lago. The K8s infrastructure is deployed (`assets/k8s/registry/lago/`, `assets/nu/k8s/setup_lago.nu`), and comprehensive specs exist in `docs/guides/journey-auditor-guide/`. No Rust implementation exists yet.

**Problem:** There is no metering, billing, or access control. LLM token usage is estimated (not actual), and token data doesn't reach task attempt events. Users can execute dispatches without any entitlement checks.

**Outcome:** A complete billing foundation — accurate token tracking, usage capture, background sync to Lago, and pre-flight access control at dispatch time.

### Key Decisions

| Decision | Choice |
|----------|--------|
| Lago client | `lago-client = "0.1.6"`, extend with reqwest for gaps |
| Client crate | New `dapple-lago` crate |
| Sync worker | New `dapple-prompt-lago-worker` binary |
| Migration strategy | Single migration for all tables |
| Token accuracy | Fix Anthropic backend to extract real counts |
| Token propagation | Fix flow pipeline (conduit → dispatch_runner) |
| Sync trigger | PostgreSQL LISTEN/NOTIFY |
| Access control | Hard block at dispatch_runner level |
| Billable scope | User-level only (no company) |
| Testing | Real local Lago instance |
| Flagforge | Not in scope |

---

## Phase 1: Database Schema

**Dependencies:** None
**Goal:** Single migration creating all billing tables.

### Migration

**Create:** `crates/dapple-data/migrations/00000000000021_billing_foundation.sql`

#### 1. Lookup Tables

```sql
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
    ('zapier', 'Zapier'), ('sendgrid', 'SendGrid'), ('twilio', 'Twilio');
```

#### 2. Billable Entities (user-level only)

```sql
CREATE TABLE rev.billable_entities (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    lago_customer_external_id TEXT,
    lago_subscription_external_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX billable_entities_lago_customer_idx
    ON rev.billable_entities (lago_customer_external_id) WHERE lago_customer_external_id IS NOT NULL;

CREATE TABLE rev.user_billable_entities (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    billable_entity_id UUID NOT NULL REFERENCES rev.billable_entities(id),
    user_id BIGINT NOT NULL REFERENCES rev.user_details(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT user_billable_entities_be_unique UNIQUE (billable_entity_id),
    CONSTRAINT user_billable_entities_user_unique UNIQUE (user_id)
);
CREATE INDEX user_billable_entities_user_idx ON rev.user_billable_entities (user_id);
```

#### 3. Resource Claims

```sql
CREATE TABLE rev.resource_claims (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    dispatch_task_attempt_id UUID,
    correlation_id UUID,
    claim_data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT valid_claim_data CHECK (jsonb_typeof(claim_data) = 'object')
);
CREATE INDEX resource_claims_attempt_idx ON rev.resource_claims (dispatch_task_attempt_id);
CREATE INDEX resource_claims_correlation_idx ON rev.resource_claims (correlation_id);
```

#### 4. Usage Detail Tables

```sql
CREATE TABLE rev.dispatch_task_attempt_event_llm_usages (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    event_id UUID NOT NULL,
    claim_id UUID REFERENCES rev.resource_claims(id),
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    provider_input_tokens INTEGER,
    provider_output_tokens INTEGER,
    provider_id SMALLINT NOT NULL REFERENCES rev.providers(id),
    model_release_id INTEGER NOT NULL REFERENCES rev.model_releases(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT positive_tokens CHECK (input_tokens >= 0 AND output_tokens >= 0)
);
CREATE INDEX llm_usages_event_idx ON rev.dispatch_task_attempt_event_llm_usages (event_id);

CREATE TABLE rev.dispatch_task_attempt_event_assets (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    event_id UUID NOT NULL,
    claim_id UUID REFERENCES rev.resource_claims(id),
    asset_kind_id BIGINT NOT NULL REFERENCES rev.asset_kinds(id),
    size_bytes BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT positive_size CHECK (size_bytes >= 0)
);
CREATE INDEX asset_usages_event_idx ON rev.dispatch_task_attempt_event_assets (event_id);

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
CREATE INDEX service_calls_event_idx ON rev.dispatch_task_attempt_event_service_calls (event_id);
```

#### 5. Correction Tables

```sql
CREATE TABLE rev.resource_voids (
    id UUID NOT NULL DEFAULT uuidv7() PRIMARY KEY,
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    original_id UUID NOT NULL,
    reason TEXT NOT NULL,
    issue_reference TEXT NOT NULL,
    voided_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

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
```

#### 6. Lago Sync

```sql
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
    ON rev.lago_event_sync (created_at) WHERE sent_at IS NULL;
```

### Post-Migration

```bash
nu -c 'cargo sqlx migrate run'
nu -c 'cargo sqlx prepare --workspace'
```

### Verification
- `cargo sqlx migrate info` shows the new migration applied
- `psql` `\dt rev.*` shows all new tables
- FK constraints verified via `\d+ rev.dispatch_task_attempt_event_llm_usages`

---

## Phase 2: Token Pipeline Fix

**Dependencies:** None (parallel with Phase 1)
**Goal:** Real token counts from Anthropic → through flow → into task attempt events.

### Current State (broken)

```
Anthropic API → rig-core agent.prompt() returns String (usage discarded)
  → handler estimates input_tokens = content.len()/4, counts output chunks
  → SSE Done sends estimates → flow client DISCARDS them
  → PromptResult::Text(String) has no token data
  → dispatch_runner inserts events WITHOUT tokens
```

### 2.1 UsageMetadata Struct + LlmBackend Trait Change

**File:** `crates/dapple-prompt-conduit/src/service/llm/mod.rs`

```rust
#[derive(Debug, Clone, Default)]
pub struct UsageMetadata {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub provider_input_tokens: Option<u32>,
    pub provider_output_tokens: Option<u32>,
    pub model_id: Option<String>,
}
```

Update `LlmBackend` trait and `LlmService::execute_streaming` return type.

### 2.2 Fix AnthropicBackend — Real Token Extraction

**File:** `crates/dapple-prompt-conduit/src/service/llm/anthropic.rs`

Use rig-core's lower-level completion API or fall back to direct Anthropic HTTP API.

### 2.3-2.7 Pipeline Changes

Update all backends, prompt handler, conduit client, prompt_send command, and dispatch_runner.

### Files Modified
- `crates/dapple-prompt-conduit/src/service/llm/mod.rs`
- `crates/dapple-prompt-conduit/src/service/llm/anthropic.rs`
- `crates/dapple-prompt-conduit/src/service/llm/stub.rs`
- `crates/dapple-prompt-conduit/src/service/llm/mistral.rs`
- `crates/dapple-prompt-conduit/src/service/llm/mistralrs_backend.rs`
- `crates/dapple-prompt-conduit/src/handler/prompt.rs`
- `crates/dapple-prompt-flow/src/conduit_client.rs`
- `crates/dapple-prompt-flow/src/commands/prompt_send.rs`
- `crates/dapple-prompt-flow/src/dispatch_runner.rs`

---

## Phase 3: dapple-lago Crate

**Dependencies:** None (parallel with Phase 1 and 2)
**Goal:** Lago HTTP client crate wrapping `lago-client` 0.1.6.

### New Crate: `crates/dapple-lago/`

Key modules: config, client, models, service (events, customers, access).

### lago-client Gap Strategy

Use `lago-client` for endpoints it supports. For missing endpoints (Events API, current_usage, entitlements, wallets), use direct `reqwest` calls.

---

## Phase 4: Usage Capture

**Dependencies:** Phase 1 (tables), Phase 2 (token data available)
**Goal:** dapple-data models + queries for billing tables. Wire LLM usage recording into dispatch execution.

### Models, SQL, Queries, Wiring

New model files, .sqlc files, query functions, and dispatch_runner integration.

---

## Phase 5: Lago Sync Worker

**Dependencies:** Phase 1, Phase 3, Phase 4
**Goal:** New binary that listens for sync notifications and pushes events to Lago.

### New Crate: `crates/dapple-prompt-lago-worker/`

LISTEN/NOTIFY loop + periodic sweep, event builder, sync logic with retry backoff.

---

## Phase 6: Pre-flight Access Control

**Dependencies:** Phase 3 (dapple-lago), Phase 4 (billable entity resolution)
**Goal:** Block dispatch execution if user lacks billing entitlements.

Access check at dispatch_runner level with graceful degradation.

---

## New Environment Variables

| Variable | Service(s) | Required | Purpose |
|----------|-----------|----------|---------|
| `LAGO_API_URL` | lago-worker, flow | No | Lago REST API endpoint |
| `LAGO_API_KEY` | lago-worker, flow | No | Lago API authentication key |

When unset, Lago features are silently disabled.

---

## Critical File Index

| File | Phase | Change |
|------|-------|--------|
| `crates/dapple-data/migrations/00000000000021_billing_foundation.sql` | 1 | Create |
| `crates/dapple-prompt-conduit/src/service/llm/mod.rs` | 2 | UsageMetadata + trait change |
| `crates/dapple-prompt-conduit/src/service/llm/anthropic.rs` | 2 | Real token extraction |
| `crates/dapple-prompt-conduit/src/handler/prompt.rs` | 2 | Use real tokens |
| `crates/dapple-prompt-flow/src/conduit_client.rs` | 2 | PromptResult carries tokens |
| `crates/dapple-prompt-flow/src/commands/prompt_send.rs` | 2 | Return token metadata |
| `crates/dapple-prompt-flow/src/dispatch_runner.rs` | 2,4,6 | Tokens + usage + access |
| `crates/dapple-lago/` | 3 | New crate |
| `crates/dapple-data/src/model/` | 4 | New model files |
| `crates/dapple-data/assets/sqlc/` | 4 | New .sqlc files |
| `crates/dapple-data/src/query/` | 4 | New query files |
| `crates/dapple-prompt-lago-worker/` | 5 | New crate + binary |
| `crates/dapple-prompt-flow/src/access.rs` | 6 | New module |
