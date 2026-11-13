# Usage Tracking

Abilities, billable metrics, resource caps, and liability tracking form the foundation of usage-based billing.

> **Terminology:** This section uses Lago-aligned terminology. See [Glossary](../glossary.md) for the terminology mapping from legacy docs.

---

## A Token's Journey

Let's trace what happens when Sarah clicks "Generate Resume" in the Dapple UI.

**1. User Action (10:32:15 AM)**
Sarah hits the button. The UI creates a dispatch request for her Journey's current step.

**2. Dispatch Starts (10:32:15.100 AM)**
The dispatch system picks up the request, creates a `dispatch_task_attempt`, and starts executing the step's tasks.

**3. Conduit Makes the LLM Call (10:32:15.250 AM)**
The task calls dapple-prompt-conduit to run a prompt. Before making the call, conduit counts the input tokens locally (~1,500 tokens). Then it sends the request to Anthropic.

**4. Claim Created (10:32:15.251 AM)**
Immediately after sending, conduit creates a **resource claim**—a record that says "I'm about to consume LLM tokens." The claim includes:
- Correlation ID (links this to Sarah's dispatch)
- Input token count (our estimate)
- Provider (Anthropic)
- Model (Claude 3.5 Sonnet)

This is the "claim" part of claim-then-collect. We record what we're consuming *before* we know the final result.

**5. LLM Returns (10:32:18.892 AM)**
Claude responds with the generated resume content. The response includes Anthropic's token counts (1,502 input, 487 output). Conduit updates the claim with:
- Output token count (our estimate: 490)
- Provider's counts (for reconciliation)

**6. Dispatch Completes (10:32:19.100 AM)**
The dispatch handler sees the task succeeded. It runs the "collection" step:
- Find all claims for this attempt
- Create permanent resource records (`dispatch_task_attempt_event_llm_usages`)
- Each record gets a UUIDv7 `object_id` that becomes the Lago `transaction_id`

**7. Lago Sync Queued (10:32:19.150 AM)**
A sync record is created linking the LLM usage record to a pending Lago event. The sync worker will send:
```json
{
  "transaction_id": "019xyz...",  // UUIDv7 from usage record
  "external_subscription_id": "dapple_sub_abc123",
  "code": "llm_tokens",
  "timestamp": "2024-01-15T10:32:19Z",
  "properties": {
    "tokens": 1989  // input + output
  }
}
```

**8. Lago Receives Event (10:32:19.500 AM)**
Lago ingests the event. It's now part of Sarah's billable usage for this billing period.

**9. End of Month**
Lago aggregates all `llm_tokens` events for Sarah's subscription. Her invoice shows the total token consumption.

**The Key Pattern: Claim-Then-Collect**

Why not just create the resource record after the LLM call completes?

Because things fail. If the dispatch crashes after the LLM call but before recording usage, we've consumed tokens we'll never bill for. The claim pattern ensures:

1. **Claims exist even if collection fails** — Orphan claims surface as audit anomalies
2. **Correlation IDs link everything** — One dispatch, multiple LLM calls, all traceable
3. **Our counts vs provider counts** — We reconcile discrepancies, not hide them

---

## Overview

<!-- ANCHOR: domain-usage-tracking -->
Usage tracking connects user actions to billing through Lago:

```
Platform Action → Internal Event → Lago Usage Event → Billable Metric → Invoice
       ↓               ↓                  ↓                 ↓
   Ability ID     Audit trail      external_id        Aggregated usage
```

Each link in this chain must be auditable and reconcilable between our system and Lago.
<!-- /ANCHOR -->

---

## Abilities

<!-- ANCHOR: domain-abilities -->
An **Ability** is a trackable user action that may be free or paid.

### Characteristics
- Unique identifier (`ability_key`) for each type of action
- May be free (no usage event to Lago) or billable (generates usage event)
- Logged internally with Lago `external_id` correlation
- Enables granular usage analytics

### Examples
| Ability | Billable | Lago Metric | Liability |
|---------|----------|-------------|-----------|
| Create Journey | Varies by tier | `journey_created` | None |
| Execute Dispatch Step | Yes | `dispatch_executed` | Deferred |
| Generate LLM Output | Yes | `llm_tokens` (sum) | Immediate |
| Generate PDF | Yes | `assets_generated` | Deferred |
| View Dashboard | No | (not sent) | None |

### Audit Concerns
- Every ability usage should be logged as an internal event
- Billable abilities must generate corresponding Lago usage events
- Non-billable abilities should be explicitly marked (not inferred from missing events)
<!-- /ANCHOR -->

---

## Billable Metrics

<!-- ANCHOR: domain-billable-metrics -->
**Billable Metrics** define how usage events are aggregated and charged (Lago terminology).

### Metric Types

| Type | Aggregation | Examples | Liability Timing |
|------|-------------|----------|------------------|
| **LLM Token Usage** | Sum of `tokens` property | Claude/GPT I/O tokens | Immediate (third-party cost) |
| **Service Calls** | Count of events | Zapier triggers, SendGrid | Immediate (third-party cost) |
| **Generated Assets** | Count of events | PDFs, images, audio | Deferred |
| **Compute Time** | Sum of `duration` | Processing minutes | Immediate |

### Billing Models (Lago Charge Types)

| Model | Lago Charge | Use Case |
|-------|-------------|----------|
| **Pay-as-you-go** | `standard` | Charge per unit consumed |
| **Tiered/Graduated** | `graduated` | Price breaks at volume thresholds |
| **Package** | `package` | Prepaid bundles (e.g., 1000 LLM tokens) |
| **Percentage** | `percentage` | % of transaction value |

### Usage Event → Invoice Flow
```
Platform sends Usage Event to Lago
           ↓
Lago aggregates by Billable Metric code
           ↓
At billing period end, Lago calculates charges
           ↓
Invoice generated with line items per metric
           ↓
Revenue recognized per GAAP rules
```

### Audit Concerns
- Every internal event should have a corresponding Lago usage event
- `transaction_id` must be unique (idempotency)
- Aggregation method must match expected behavior
- Package/prepaid balances must reconcile with usage
<!-- /ANCHOR -->

---

## Resource Caps (Slots & Limits)

<!-- ANCHOR: domain-resource-caps -->
**Resource Caps** are tier-based limits that constrain usage without depleting. Unlike billable metrics, caps don't consume—they restrict.

### Types of Caps

| Cap Type | Examples | Enforcement | Upgrade Trigger |
|----------|----------|-------------|-----------------|
| **Concurrent Slots** | 5 active journeys | Queue or reject | Consistently hitting limit |
| **Storage Limits** | 10GB, 100GB | Reject uploads | Approaching threshold |
| **Team Size** | 3, 10, unlimited members | Block invites | Need more seats |
| **Feature Gates** | Premium playbooks | Hide/disable | User requests feature |

### Slots Example
A "5-slot" subscription allows:
- Up to 5 simultaneous Journey executions
- The 6th request queues until a slot frees
- Peak usage tracked for upgrade funnel analysis

### Enforcement Flow
```
User requests resource
        ↓
Check current usage vs. cap
        ↓
If under cap → Allow, log usage
If at cap → Queue, reject, or prompt upgrade
        ↓
Log enforcement event (for analytics)
```

### Audit Concerns
- Track peak concurrent usage over time
- Verify cap limits are correctly enforced
- Log cap violations for upgrade funnel analysis
- Ensure upgrades immediately expand caps
<!-- /ANCHOR -->

---

## Liability Tracking

<!-- ANCHOR: domain-liability -->
**Liability** (deferred revenue) represents prepaid amounts not yet recognized.

### When Liability is Created
- Customer purchases one-time tokens
- Customer pays subscription invoice upfront
- Customer purchases annual plans

### When Liability is Reduced
- Tokens are consumed (ability usage)
- Daily revenue recognition (subscription period)
- Tokens expire
- Credits/refunds issued

### Liability Calculation
```
Current Liability =
    Sum of Prepaid Amounts
  - Recognized Revenue to Date
  - Expired/Refunded Amounts
```

### Audit Concerns
- Liability balances must reconcile with accounting
- Expiration should be tracked proactively
- Large liability balances may indicate unused purchases
<!-- /ANCHOR -->

---

## Audit Queries

### Events Without Lago Correlation
```sql
-- Internal events missing external Lago event_id
SELECT e.id, e.ability_key, e.created_at
FROM rev.billing_events e
WHERE e.lago_event_id IS NULL
  AND e.billable = true;
```

### Ability Usage Without Billing Event
```sql
-- Platform actions that should have generated billing events but didn't
SELECT d.id, d.dispatch_phase_key, d.created_at
FROM rev.dispatches d
WHERE d.dispatch_phase_key = 'ok'  -- completed successfully
  AND NOT EXISTS (
    SELECT 1 FROM rev.billing_events e
    WHERE e.dispatch_id = d.id
  );
```

### Resource Cap Violations
```sql
-- Requests that hit resource caps (for upgrade funnel analysis)
SELECT
    customer_id,
    cap_type,
    COUNT(*) as violation_count,
    MAX(created_at) as last_violation
FROM rev.cap_enforcement_log
WHERE action = 'rejected'
  AND created_at > now() - interval '30 days'
GROUP BY customer_id, cap_type
ORDER BY violation_count DESC;
```

### Lago Reconciliation Check
```sql
-- Compare internal event counts vs expected Lago aggregation
SELECT
    DATE_TRUNC('day', e.created_at) as day,
    e.billable_metric_code,
    COUNT(*) as internal_count,
    SUM(e.properties->>'tokens')::int as internal_token_sum
FROM rev.billing_events e
WHERE e.created_at > now() - interval '7 days'
GROUP BY 1, 2
ORDER BY 1 DESC, 2;
-- Compare these counts against Lago's billable metric dashboard
```

---

## Implementation Specifications

The following sections contain implementation details for agents building these features.

**Existing dependencies:**
- `rev.dispatch_task_attempt_events` — Parent table (exists)
- `rev.asset_kinds` — image, document, audio, video, binary (exists)
- `rev.providers` — anthropic, openai, mistral, self-hosted, stub (exists)
- `rev.model_releases` — Specific model configurations (exists)

### Migration Order

1. Lookup tables: `resource_types`, `integration_services`
2. `resource_claims`
3. `dispatch_task_attempt_event_llm_usages` (references claims, providers, model_releases)
4. `dispatch_task_attempt_event_assets` (references claims, asset_kinds)
5. `dispatch_task_attempt_event_service_calls` (references claims, integration_services)
6. `resource_voids`, `resource_amendments`
7. `lago_event_sync`

---

<!-- ANCHOR: impl-resource-types-lookup -->
### Resource Types Lookup Table

**Issue #TODO-RESOURCE-TYPES** — Create rev.resource_types lookup table
**Status:** Not Started
**Dependencies:** None (foundational lookup)

```sql
CREATE TABLE rev.resource_types (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE
);

INSERT INTO rev.resource_types (key) VALUES
    ('llm_usage'),
    ('asset'),
    ('service_call');
```

**Notes:**
- Used by `resource_claims`, `resource_voids`, `resource_amendments`, `lago_event_sync`

**Acceptance Criteria:**
- [ ] Migration creates table and seeds values
- [ ] No model/query needed (joined via FK)
<!-- /ANCHOR -->

---

<!-- ANCHOR: impl-integration-services-lookup -->
### Integration Services Lookup Table

**Issue #TODO-INTEGRATION-SERVICES** — Create rev.integration_services lookup table
**Status:** Not Started
**Dependencies:** None (foundational lookup)

```sql
CREATE TABLE rev.integration_services (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    display_name TEXT
);

INSERT INTO rev.integration_services (key, display_name) VALUES
    ('zapier', 'Zapier'),
    ('sendgrid', 'SendGrid'),
    ('twilio', 'Twilio');
```

**Notes:**
- Used by `dispatch_task_attempt_event_service_calls`
- Extensible — add new services as integrations are built

**Acceptance Criteria:**
- [ ] Migration creates table and seeds initial values
- [ ] No model/query needed (joined via FK)
<!-- /ANCHOR -->

---

<!-- ANCHOR: impl-resource-capture-flow -->
### Resource Capture Flow

**Issue #TODO-RESOURCE-CAPTURE** — Implement claim creation in conduit and collection in dispatch

This documents where and how resource consumption is captured in the codebase.

**Capture Points:**

| Resource Type | Capture Location | Trigger |
|---------------|------------------|---------|
| LLM tokens | `dapple-prompt-conduit` | After LLM backend returns response |
| Assets | Asset generation service | After asset created |
| Service calls | Integration handlers | After third-party call completes |

**LLM Token Capture (Conduit):**

Location: `crates/dapple-prompt-conduit/src/service/llm/mod.rs`

```rust
// Pseudocode for claim creation in LlmService

impl LlmService {
    pub async fn execute(&self, request: PromptRequest) -> Result<PromptResponse> {
        // 1. Count input tokens locally (before sending)
        let input_tokens = self.count_tokens(&request.prompt);

        // 2. Execute LLM call
        let response = self.backend.execute_streaming(request).await?;

        // 3. Count output tokens locally
        let output_tokens = self.count_tokens(&response.content);

        // 4. Extract provider-reported counts (if available)
        let provider_input = response.usage.map(|u| u.input_tokens);
        let provider_output = response.usage.map(|u| u.output_tokens);

        // 5. Create claim
        let claim = ResourceClaim {
            claim_type: "llm_usage",
            claim_data: json!({
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "provider_input_tokens": provider_input,
                "provider_output_tokens": provider_output,
                "provider": self.backend.provider_name(),
                "model": request.model,
            }),
            dispatch_task_attempt_id: request.context.attempt_id,
            correlation_id: request.context.correlation_id,
        };

        self.claims_service.create_claim(claim).await?;

        // 6. Return response (claim is independent)
        Ok(response)
    }
}
```

**Collection (Dispatch Handler):**

Location: `crates/dapple-prompt-api/src/handler/dispatches.rs` (or dispatch completion handler)

```rust
// Pseudocode for claim collection when dispatch completes

async fn on_dispatch_task_complete(
    pool: &PgPool,
    event: &DispatchTaskAttemptEvent,
) -> Result<()> {
    // Only collect on successful completion
    if event.phase_code != "ok" {
        return Ok(());
    }

    // Find all claims for this attempt
    let claims = claims::select_by_attempt_id(pool, event.attempt_id).await?;

    // Create resource records for each claim
    for claim in claims {
        match claim.claim_type.as_str() {
            "llm_usage" => {
                let data: LlmClaimData = serde_json::from_value(claim.claim_data)?;
                llm_usages::insert(pool, InsertLlmUsageArgs {
                    event_id: event.id,
                    claim_object_id: claim.object_id,
                    input_tokens: data.input_tokens,
                    output_tokens: data.output_tokens,
                    provider_input_tokens: data.provider_input_tokens,
                    provider_output_tokens: data.provider_output_tokens,
                    provider: data.provider,
                    model: data.model,
                }).await?;
            }
            "asset" => { /* similar */ }
            "service_call" => { /* similar */ }
            _ => { /* log unknown type */ }
        }
    }

    Ok(())
}
```

**Token Counting:**

For local token counting, options:
1. **tiktoken-rs** — OpenAI's tokenizer (works for GPT models)
2. **Anthropic's tokenizer** — For Claude models (if available as library)
3. **Approximate** — Character-based heuristic as fallback

```rust
// Token counting trait
trait TokenCounter {
    fn count_tokens(&self, text: &str) -> usize;
}

// Implementation varies by provider/model
```

**Correlation ID:**

The `correlation_id` groups claims from the same execution context:
- Set at dispatch start
- Passed through to conduit via request context
- All claims from one dispatch share the same correlation_id
- Useful for debugging and grouping

**Error Handling:**

- If claim creation fails: Log error, but don't fail the LLM call (billing is secondary to functionality)
- If collection fails: Claims remain orphaned, surfaced by audit query, investigated via ticket

**Notes:**
- Claims are created synchronously during execution (not queued)
- Collection happens as part of dispatch completion (transactional with event logging)
- Failed dispatches leave orphan claims (intentional—we still consumed resources)
<!-- /ANCHOR -->

<!-- ANCHOR: impl-resource-claims-table -->
### Resource Claims Table

**Issue #TODO-RESOURCE-CLAIMS** — Create rev.resource_claims table
**Status:** Not Started
**Dependencies:** `rev.resource_types`, `rev.dispatch_task_attempts`

Claims are made when resources are consumed (e.g., conduit makes LLM calls). Collection happens when dispatch completes and creates resource records that reference these claims.

**Schema:**
```sql
CREATE TABLE rev.resource_claims (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    claim_data JSONB NOT NULL,
    dispatch_task_attempt_id BIGINT REFERENCES rev.dispatch_task_attempts(id),
    correlation_id UUID,  -- groups claims from same execution context
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT valid_claim_data CHECK (jsonb_typeof(claim_data) = 'object')
);

CREATE UNIQUE INDEX idx_claims_object_id ON rev.resource_claims(object_id);
CREATE INDEX idx_claims_type ON rev.resource_claims(resource_type_id);
CREATE INDEX idx_claims_attempt ON rev.resource_claims(dispatch_task_attempt_id);
CREATE INDEX idx_claims_correlation ON rev.resource_claims(correlation_id);
CREATE INDEX idx_claims_created ON rev.resource_claims(created_at);
```

**Claim Data Examples:**
```json
// LLM usage claim (resource_type_id = 1)
{
  "input_tokens": 1500,
  "output_tokens": 500,
  "provider_input_tokens": 1502,
  "provider_output_tokens": 498,
  "provider_id": 1,
  "model_release_id": 5
}

// Asset claim (resource_type_id = 2)
{
  "asset_kind_id": 2,
  "size_bytes": 102400
}

// Service call claim (resource_type_id = 3)
{
  "integration_service_id": 1,
  "endpoint": "/hooks/abc123",
  "units": 1
}
```

**Rust Types:**
- `ResourceClaimModel` — Full model with joined resource_type key
- `InsertResourceClaimArgs` — `{ resource_type_ids, claim_data, dispatch_task_attempt_ids, correlation_ids }`
- `InsertedResourceClaimModel` — INSERT...RETURNING result

**Queries:**
- `insert_resource_claims.sql` — Batch insert
- `select_resource_claims_by_attempt.sql` — For collection
- `select_uncollected_claims.sql` — Orphan detection

**Acceptance Criteria:**
- [ ] Migration creates table with FK to resource_types
- [ ] Model in `dapple-data/src/model/resource_claims.rs`
- [ ] Queries in `dapple-data/assets/sql/resource_claims/`
- [ ] Integration test: create claim, collect claim
<!-- /ANCHOR -->

<!-- ANCHOR: impl-llm-usage-table -->
### LLM Usage Table

**Issue #TODO-LLM-USAGE** — Create rev.dispatch_task_attempt_event_llm_usages table
**Status:** Not Started
**Dependencies:** `rev.dispatch_task_attempt_events`, `rev.resource_claims`, `rev.providers`, `rev.model_releases`

Child table tracking LLM token consumption per dispatch task attempt event.
Source of truth for usage facts. No cost data—accounting handles allocation.

**Schema:**
```sql
CREATE TABLE rev.dispatch_task_attempt_event_llm_usages (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    event_id BIGINT NOT NULL REFERENCES rev.dispatch_task_attempt_events(id),
    claim_id BIGINT NOT NULL REFERENCES rev.resource_claims(id),

    -- Our counts (always populated, local calculation)
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,

    -- Provider's counts (when available, for reconciliation)
    provider_input_tokens INTEGER,
    provider_output_tokens INTEGER,

    -- Model reference (existing taxonomy)
    provider_id SMALLINT NOT NULL REFERENCES rev.providers(id),
    model_release_id INTEGER NOT NULL REFERENCES rev.model_releases(id),

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT positive_tokens CHECK (input_tokens >= 0 AND output_tokens >= 0)
);

CREATE UNIQUE INDEX idx_llm_usages_object_id ON rev.dispatch_task_attempt_event_llm_usages(object_id);
CREATE INDEX idx_llm_usages_event_id ON rev.dispatch_task_attempt_event_llm_usages(event_id);
CREATE INDEX idx_llm_usages_claim ON rev.dispatch_task_attempt_event_llm_usages(claim_id);
CREATE INDEX idx_llm_usages_created_at ON rev.dispatch_task_attempt_event_llm_usages(created_at);
CREATE INDEX idx_llm_usages_provider ON rev.dispatch_task_attempt_event_llm_usages(provider_id);
CREATE INDEX idx_llm_usages_model ON rev.dispatch_task_attempt_event_llm_usages(model_release_id);
```

**Notes:**
- One row per LLM call within an event (event may have multiple calls)
- `claim_id` links to the claim made when resource was consumed (normal FK pattern)
- Both our counts and provider counts stored for reconciliation
- `provider_id` FK to existing `rev.providers` (model taxonomy)
- `model_release_id` FK to existing `rev.model_releases` (lineage + tier + release)
- No cost columns—vendor invoice reconciliation happens in accounting
- `object_id` uses UUIDv7 for time-ordering and serves as Lago `transaction_id`

**Rust Types:**
- `LlmUsageModel` — Full model with joined provider/model details
- `InsertLlmUsageArgs` — `{ event_ids, claim_ids, input_tokens, output_tokens, provider_ids, model_release_ids, ... }`
- `InsertedLlmUsageModel` — INSERT...RETURNING result

**Queries:**
- `insert_llm_usages.sql` — Batch insert
- `select_llm_usages_by_event.sql` — Get usages for an event
- `select_llm_usages_token_discrepancy.sql` — Our vs provider count mismatches

**Reconciliation Queries:**
```sql
-- Our count vs provider count discrepancies
SELECT lu.object_id, lu.input_tokens, lu.provider_input_tokens,
       lu.output_tokens, lu.provider_output_tokens,
       p.key as provider, mr.tier as model_tier
FROM rev.dispatch_task_attempt_event_llm_usages lu
JOIN rev.providers p ON p.id = lu.provider_id
JOIN rev.model_releases mr ON mr.id = lu.model_release_id
WHERE lu.input_tokens != lu.provider_input_tokens
   OR lu.output_tokens != lu.provider_output_tokens;
```

**Lago Mapping:**
| Internal Field | Lago Event Field |
|----------------|------------------|
| `object_id` | `transaction_id` |
| `input_tokens + output_tokens` | `properties.tokens` |
| `created_at` | `timestamp` |
| (derived from event) | `external_subscription_id` |
| `'llm_tokens'` | `code` |

**Acceptance Criteria:**
- [ ] Migration creates table with FKs to providers, model_releases, claims
- [ ] Model in `dapple-data/src/model/llm_usages.rs`
- [ ] Queries in `dapple-data/assets/sql/llm_usages/`
- [ ] Integration test: insert usage, query by event
<!-- /ANCHOR -->

<!-- ANCHOR: impl-asset-usage-table -->
### Asset Generation Table

**Issue #TODO-ASSET-USAGE** — Create rev.dispatch_task_attempt_event_assets table
**Status:** Not Started
**Dependencies:** `rev.dispatch_task_attempt_events`, `rev.resource_claims`, `rev.asset_kinds`

Child table tracking generated assets (PDFs, images, audio) per event.

**Schema:**
```sql
CREATE TABLE rev.dispatch_task_attempt_event_assets (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    event_id BIGINT NOT NULL REFERENCES rev.dispatch_task_attempt_events(id),
    claim_id BIGINT NOT NULL REFERENCES rev.resource_claims(id),
    asset_kind_id BIGINT NOT NULL REFERENCES rev.asset_kinds(id),
    size_bytes BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT positive_size CHECK (size_bytes >= 0)
);

CREATE UNIQUE INDEX idx_asset_usages_object_id ON rev.dispatch_task_attempt_event_assets(object_id);
CREATE INDEX idx_asset_usages_event_id ON rev.dispatch_task_attempt_event_assets(event_id);
CREATE INDEX idx_asset_usages_claim ON rev.dispatch_task_attempt_event_assets(claim_id);
CREATE INDEX idx_asset_usages_created_at ON rev.dispatch_task_attempt_event_assets(created_at);
CREATE INDEX idx_asset_usages_kind ON rev.dispatch_task_attempt_event_assets(asset_kind_id);
```

**Notes:**
- `claim_id` links to the claim made when asset was generated (normal FK pattern)
- References existing `rev.asset_kinds` lookup table
- One row per asset generated
- `size_bytes` for potential storage-based billing in future

**Rust Types:**
- `AssetUsageModel` — Full model with joined asset_kind key
- `InsertAssetUsageArgs` — `{ event_ids, claim_ids, asset_kind_ids, size_bytes }`
- `InsertedAssetUsageModel` — INSERT...RETURNING result

**Queries:**
- `insert_asset_usages.sql` — Batch insert
- `select_asset_usages_by_event.sql` — Get usages for an event

**Lago Mapping:**
| Internal Field | Lago Event Field |
|----------------|------------------|
| `object_id` | `transaction_id` |
| `1` (count) | `properties.units` |
| `created_at` | `timestamp` |
| `'assets_generated'` | `code` |

**Acceptance Criteria:**
- [ ] Migration creates table with FKs
- [ ] Model in `dapple-data/src/model/asset_usages.rs`
- [ ] Queries in `dapple-data/assets/sql/asset_usages/`
- [ ] Integration test passing
<!-- /ANCHOR -->

<!-- ANCHOR: impl-service-call-table -->
### Third-Party Service Call Table

**Issue #TODO-SERVICE-CALLS** — Create rev.dispatch_task_attempt_event_service_calls table
**Status:** Not Started
**Dependencies:** `rev.dispatch_task_attempt_events`, `rev.resource_claims`, `rev.integration_services`

Child table tracking calls to third-party services (Zapier, SendGrid, etc.) per event.

**Schema:**
```sql
CREATE TABLE rev.dispatch_task_attempt_event_service_calls (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    event_id BIGINT NOT NULL REFERENCES rev.dispatch_task_attempt_events(id),
    claim_id BIGINT NOT NULL REFERENCES rev.resource_claims(id),
    integration_service_id BIGINT NOT NULL REFERENCES rev.integration_services(id),
    endpoint TEXT,              -- specific API endpoint if relevant
    units INTEGER NOT NULL DEFAULT 1,  -- emails sent, triggers fired, etc.
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT positive_units CHECK (units > 0)
);

CREATE UNIQUE INDEX idx_service_calls_object_id ON rev.dispatch_task_attempt_event_service_calls(object_id);
CREATE INDEX idx_service_calls_event_id ON rev.dispatch_task_attempt_event_service_calls(event_id);
CREATE INDEX idx_service_calls_claim ON rev.dispatch_task_attempt_event_service_calls(claim_id);
CREATE INDEX idx_service_calls_created_at ON rev.dispatch_task_attempt_event_service_calls(created_at);
CREATE INDEX idx_service_calls_service ON rev.dispatch_task_attempt_event_service_calls(integration_service_id);
```

**Notes:**
- `claim_id` links to the claim made when service was called (normal FK pattern)
- `integration_service_id` FK to lookup table (zapier, sendgrid, twilio, etc.)
- `units` allows for batch operations (e.g., 5 emails sent in one call)
- `endpoint` is optional—useful for analytics on which integrations are used

**Rust Types:**
- `ServiceCallModel` — Full model with joined service key
- `InsertServiceCallArgs` — `{ event_ids, claim_ids, integration_service_ids, endpoints, units }`
- `InsertedServiceCallModel` — INSERT...RETURNING result

**Queries:**
- `insert_service_calls.sql` — Batch insert
- `select_service_calls_by_event.sql` — Get calls for an event

**Lago Mapping:**
| Internal Field | Lago Event Field |
|----------------|------------------|
| `object_id` | `transaction_id` |
| `units` | `properties.units` |
| `created_at` | `timestamp` |
| (join integration_services.key) | `properties.service` |
| `'service_calls'` | `code` |

**Acceptance Criteria:**
- [ ] Migration creates table with FKs
- [ ] Model in `dapple-data/src/model/service_calls.rs`
- [ ] Queries in `dapple-data/assets/sql/service_calls/`
- [ ] Integration test passing
<!-- /ANCHOR -->

<!-- ANCHOR: impl-resource-voids-table -->
### Resource Voids Table

**Issue #TODO-RESOURCE-VOIDS** — Create rev.resource_voids table
**Status:** Not Started
**Dependencies:** `rev.resource_types`

Marks original resource records as invalid. Immutable once written. Does not contain corrected data—just flags the original as void.

**Schema:**
```sql
CREATE TABLE rev.resource_voids (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    original_object_id UUID NOT NULL,
    reason TEXT NOT NULL,
    issue_reference TEXT NOT NULL,  -- 'Issue #142' — required, no void without ticket
    voided_by TEXT NOT NULL,  -- who approved the void
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_voids_object_id ON rev.resource_voids(object_id);
CREATE UNIQUE INDEX idx_voids_original ON rev.resource_voids(resource_type_id, original_object_id);
CREATE INDEX idx_voids_issue ON rev.resource_voids(issue_reference);
```

**Notes:**
- One void per original record (unique constraint on type + original_object_id)
- `issue_reference` is required—no silent voids
- `voided_by` tracks accountability
- Void doesn't delete anything—original record stays, this marks it invalid

**Rust Types:**
- `ResourceVoidModel` — Full model with joined resource_type key
- `InsertResourceVoidArgs` — `{ resource_type_ids, original_object_ids, reasons, issue_references, voided_bys }`

**Queries:**
- `insert_resource_voids.sql` — Create void record
- `select_voids_by_original.sql` — Check if record is voided

**Acceptance Criteria:**
- [ ] Migration creates table with FK to resource_types
- [ ] Model in `dapple-data/src/model/resource_voids.rs`
- [ ] Queries in `dapple-data/assets/sql/resource_voids/`
- [ ] Integration test: void a record, verify original still exists
<!-- /ANCHOR -->

<!-- ANCHOR: impl-resource-amendments-table -->
### Resource Amendments Table

**Issue #TODO-RESOURCE-AMENDMENTS** — Create rev.resource_amendments table
**Status:** Not Started
**Dependencies:** `rev.resource_types`, `rev.dispatch_task_attempt_events`, `rev.resource_voids`

Contains corrected or backfilled resource data. May reference a void (correction) or stand alone (backfill).

**Schema:**
```sql
CREATE TABLE rev.resource_amendments (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    event_id BIGINT NOT NULL REFERENCES rev.dispatch_task_attempt_events(id),
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    void_id BIGINT REFERENCES rev.resource_voids(id),  -- NULL for backfills, set for corrections

    -- Type-specific data as JSONB (flexible for all resource types)
    amendment_data JSONB NOT NULL,

    reason TEXT NOT NULL,
    issue_reference TEXT NOT NULL,
    amended_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT valid_amendment_data CHECK (jsonb_typeof(amendment_data) = 'object')
);

CREATE UNIQUE INDEX idx_amendments_object_id ON rev.resource_amendments(object_id);
CREATE INDEX idx_amendments_event ON rev.resource_amendments(event_id);
CREATE INDEX idx_amendments_void ON rev.resource_amendments(void_id) WHERE void_id IS NOT NULL;
CREATE INDEX idx_amendments_issue ON rev.resource_amendments(issue_reference);
CREATE INDEX idx_amendments_type ON rev.resource_amendments(resource_type_id);
```

**Notes:**
- `void_id` NULL = backfill (new data), NOT NULL = correction (replaces voided)
- `amendment_data` is JSONB to handle all resource types:
  - LLM: `{"input_tokens": 500, "output_tokens": 100, "provider_id": 1, "model_release_id": 5}`
  - Asset: `{"asset_kind_id": 1, "size_bytes": 1024}`
  - Service: `{"integration_service_id": 1, "endpoint": "/hooks/123", "units": 1}`
- `issue_reference` required—no amendments without tickets
- Links to `event_id` to maintain parent relationship

**Rust Types:**
- `ResourceAmendmentModel` — Full model with joined resource_type key
- `InsertResourceAmendmentArgs` — `{ event_ids, resource_type_ids, void_ids, amendment_data, reasons, issue_references, amended_bys }`

**Queries:**
- `insert_resource_amendments.sql` — Create amendment record
- `select_amendments_by_void.sql` — Get amendment for a voided record

**Lago Implications:**
- Amendments need to sync to Lago (correcting events or new events)
- The `lago_event_sync` table should handle amendments same as originals

**Acceptance Criteria:**
- [ ] Migration creates table with FKs
- [ ] Model in `dapple-data/src/model/resource_amendments.rs`
- [ ] Queries in `dapple-data/assets/sql/resource_amendments/`
- [ ] Integration test: void record, create amendment, verify chain
<!-- /ANCHOR -->

<!-- ANCHOR: impl-lago-sync-table -->
### Lago Event Sync Table

**Issue #TODO-LAGO-SYNC** — Create rev.lago_event_sync table
**Status:** Not Started
**Dependencies:** `rev.resource_types`

Tracks sync status between internal resource records and Lago. Separate from resource tables to avoid mixing concerns.

**Schema:**
```sql
CREATE TABLE rev.lago_event_sync (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    resource_type_id BIGINT NOT NULL REFERENCES rev.resource_types(id),
    resource_object_id UUID NOT NULL,  -- references the resource record's object_id
    lago_transaction_id UUID NOT NULL,  -- what we sent as transaction_id (= resource_object_id)
    sent_at TIMESTAMPTZ,
    lago_response_status INTEGER,  -- HTTP status code
    lago_response_body JSONB,
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_lago_sync_object_id ON rev.lago_event_sync(object_id);
CREATE UNIQUE INDEX idx_lago_sync_resource ON rev.lago_event_sync(resource_type_id, resource_object_id);
CREATE INDEX idx_lago_sync_pending ON rev.lago_event_sync(next_retry_at)
    WHERE sent_at IS NULL OR lago_response_status >= 400;
CREATE INDEX idx_lago_sync_transaction ON rev.lago_event_sync(lago_transaction_id);
```

**Notes:**
- Decouples "what happened" (resource tables) from "what we told Lago" (this table)
- Supports retry logic for failed sends
- `resource_object_id` + `resource_type_id` is the composite reference (no FK due to multiple source tables)
- `lago_transaction_id` = `resource_object_id` (UUIDv7 provides idempotency)
- Enables reconciliation: resource tables vs this table vs Lago's records

**Rust Types:**
- `LagoEventSyncModel` — Full model with joined resource_type key
- `InsertLagoEventSyncArgs` — `{ resource_type_ids, resource_object_ids, lago_transaction_ids }`

**Queries:**
- `insert_lago_event_syncs.sql` — Create pending sync records
- `select_pending_syncs.sql` — Get records to send
- `update_sync_sent.sql` — Mark as sent with response
- `select_failed_syncs.sql` — Get records needing retry

**States:**
| State | Condition |
|-------|-----------|
| Pending | `sent_at IS NULL` |
| Sent | `sent_at IS NOT NULL AND lago_response_status < 400` |
| Failed | `lago_response_status >= 400` |
| Retrying | `next_retry_at IS NOT NULL AND next_retry_at > now()` |

**Acceptance Criteria:**
- [ ] Migration creates table with FK to resource_types
- [ ] Model in `dapple-data/src/model/lago_event_sync.rs`
- [ ] Queries in `dapple-data/assets/sql/lago_event_sync/`
- [ ] Integration test: create sync, mark sent, verify state transitions
<!-- /ANCHOR -->

---

## Related Sections

- [Glossary: Billable Metric](../glossary.md#billable-metric)
- [Glossary: Usage Event](../glossary.md#usage-event)
- [Glossary: Resource Cap](../glossary.md#resource-cap)
- [Debit & Credit Memos](debit-credit-memos.md)
- [Lago Integration](../integrations/recurring-billing.md)
