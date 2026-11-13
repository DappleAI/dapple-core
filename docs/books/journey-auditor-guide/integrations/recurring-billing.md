# Recurring Billing (Lago)

Lago integration for subscriptions, invoicing, usage metering, and event correlation.

---

## Overview

<!-- ANCHOR: integration-lago -->
Lago (open-source billing platform) serves as the Recurring Billing System (RBS):
- Subscription lifecycle management
- Invoice generation and payment processing
- Usage-based metering and billing
- Product catalog management
- Event webhooks for system integration

**Important: Lago is Third-Party Software**

Even when self-hosted, Lago should be treated as third-party vendor software:
- We do NOT modify Lago source code
- Feature requests go through Lago's open source channels
- Support contracts with Lago for production issues
- Our investment is in **integration**, not billing system development

This approach ensures we benefit from Lago's domain expertise, security patches, and community contributions rather than maintaining billing complexity ourselves.

**Data Flow:**
```
Outbound (usage events):
dapple-prompt (conduit) → resource tables → lago_event_sync → Lago Events API

Inbound (access control):
Conduit request → Lago API (usage/entitlements/wallets) → Access decision
```

**Design Decision:** We call Lago's API directly for access control rather than caching state locally. This eliminates staleness concerns at the cost of added latency. We'll add caching only if latency becomes a verified issue.
<!-- /ANCHOR -->

---

## Scalability Considerations

<!-- ANCHOR: integration-lago-scalability -->
### When This Stack Works (~50K Customers)

The minimal Lago stack (PostgreSQL + Redis) is appropriate for:
- **< 50,000 customers**
- **< 1M events/day**
- **Minute-level billing latency** (not real-time)

This covers the majority of SaaS use cases. Lago's PostgreSQL-based event storage handles typical volumes efficiently.

### When to Scale Beyond

Add Kafka/ClickHouse if you encounter:
- **> 100,000 customers**
- **> 10M events/day**
- **Sub-second analytics requirements**

The migration path is straightforward—Lago supports both architectures and can switch by enabling an events-processor service. However, start minimal and scale only when metrics indicate bottlenecks.
<!-- /ANCHOR -->

---

## Platform Event → Lago Usage Event Mapping

<!-- ANCHOR: integration-platform-events -->
### Event Generation Points

Events are generated at specific points in the dapple-prompt execution lifecycle:

| Platform Event | Trigger | Lago Metric Code | Properties |
|----------------|---------|------------------|------------|
| **Dispatch Completed** | `dispatch_task_attempt_event.phase_code = 'ok'` | `dispatch_executed` | `journey_id`, `dispatch_id`, `task_id` |
| **LLM Tokens Used** | Prompt response from conduit | `llm_tokens` | `input_tokens`, `output_tokens`, `provider`, `model` |
| **Asset Generated** | PDF/image/audio generation | `assets_generated` | `asset_type`, `size_bytes` |
| **Journey Created** | Journey row inserted | `journey_created` | `playbook_id`, `user_id` |

### Event Payload Structure

Events sent to Lago follow this structure (from `models/event.go`):

```json
{
  "organization_id": "<lago-org-id>",
  "external_subscription_id": "<customer-subscription-id>",
  "transaction_id": "<unique-idempotency-key>",
  "code": "llm_tokens",
  "timestamp": 1706799600,
  "properties": {
    "input_tokens": 1500,
    "output_tokens": 500,
    "provider": "anthropic",
    "model": "claude-3-sonnet"
  }
}
```

### Key Fields

| Lago Field | Source | Notes |
|------------|--------|-------|
| `transaction_id` | `dispatch_task_attempt_event.object_id` | UUIDv7, provides idempotency |
| `external_subscription_id` | Customer's Lago subscription ID | Must be stored in dapple-prompt |
| `code` | Billable metric code | Maps to Lago billable metric |
| `timestamp` | Event creation time | Unix timestamp |
| `properties` | Event-specific data | Aggregated based on metric type |

### Billable Metric Configuration

Each metric type in Lago is configured with:

| Metric Code | Aggregation | Field Key | Rounding |
|-------------|-------------|-----------|----------|
| `llm_tokens` | Sum | `input_tokens` + `output_tokens` | Nearest 1 |
| `dispatch_executed` | Count | - | - |
| `assets_generated` | Count | - | - |
| `journey_created` | Count | - | - |

### Third-Party Cost Tracking (Immediate Liability)

For LLM usage, costs are incurred immediately with third-party providers:

```
dapple-prompt → Anthropic API (cost incurred)
      ↓
Internal event logged with token counts
      ↓
Lago usage event sent (for billing customer)
      ↓
Invoice reflects aggregated usage
```

**Audit Concern:** Internal token counts must reconcile with:
1. Provider invoice (Anthropic, OpenAI)
2. Lago aggregated usage
3. Customer invoices
<!-- /ANCHOR -->

---

## Webhook Events

<!-- ANCHOR: integration-lago-webhooks -->
### Purpose: Notifications Only

Since we call Lago's API directly for access control, webhooks are used for **notifications and logging only**, not for maintaining local state.

### Events to Monitor

| Event | Use Case |
|-------|----------|
| `subscription.terminated` | Log for analytics, notify ops if unexpected |
| `invoice.payment_failed` | Trigger dunning workflow, notify support |
| `event.error` | Alert for investigation (our events rejected) |

### Events to Ignore (Lago Handles)

| Event | Why Ignore |
|-------|------------|
| `invoice.created` | We don't store invoices locally |
| `subscription.started` | Access control via API, not local state |
| `credit_note.created` | Credits managed in Lago |

### Webhook Receiver

Minimal receiver for logging and alerts:

```rust
// Pseudocode
async fn handle_lago_webhook(webhook: LagoWebhook) -> Result<()> {
    // 1. Verify signature
    verify_lago_signature(&webhook)?;

    // 2. Log for analytics
    log::info!(
        webhook_type = %webhook.webhook_type,
        lago_id = %webhook.object_lago_id(),
        "Lago webhook received"
    );

    // 3. Handle specific events
    match webhook.webhook_type.as_str() {
        "invoice.payment_failed" => {
            // Notify ops, trigger dunning
            notify_payment_failure(&webhook).await?;
        }
        "event.error" => {
            // Alert: our events being rejected
            alert_event_error(&webhook).await?;
        }
        _ => {
            // Log only, no action needed
        }
    }

    Ok(())
}
```
<!-- /ANCHOR -->

---

## Product Configuration

<!-- ANCHOR: integration-lago-products -->
### Billable Metrics
Define what usage to track:
- **Standard:** Count occurrences
- **Sum:** Aggregate numeric values
- **Max:** Track peak usage
- **Unique count:** Count distinct values

### Plans
Each subscription plan in Lago:
- Unique plan code
- Pricing model (flat, graduated, package, percentage)
- Usage charges (linked to billable metrics)
- Commitment amounts (if applicable)

### Charges
Usage-based charges attached to plans:
- Charge model (standard, graduated, package, percentage)
- Minimum spend thresholds
- Prorated billing options
<!-- /ANCHOR -->

---

## Reconciliation

### What We Reconcile

Since we don't store invoices/subscriptions locally, reconciliation focuses on:

1. **Usage Event Sync** — Did our events reach Lago?
2. **Provider Token Counts** — Do our counts match LLM provider invoices?

### Usage Event Sync Query

```sql
-- Events we sent vs Lago's record
-- (requires Lago events export in temp table)
SELECT
    les.resource_object_id,
    les.lago_transaction_id,
    les.sync_status,
    les.synced_at
FROM rev.lago_event_sync les
LEFT JOIN lago_events_export le ON le.transaction_id = les.lago_transaction_id
WHERE les.sync_status = 'synced'
  AND les.synced_at > NOW() - INTERVAL '7 days'
  AND le.transaction_id IS NULL;  -- We think we sent it, Lago doesn't have it
```

### Provider Token Reconciliation

```sql
-- Compare our token counts with what we billed
SELECT
    DATE_TRUNC('month', u.created_at) AS month,
    p.name AS provider,
    SUM(u.our_token_count) AS our_tokens,
    SUM(u.provider_token_count) AS provider_tokens,
    SUM(u.provider_token_count) - SUM(u.our_token_count) AS variance
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.providers p ON p.id = u.provider_id
WHERE u.created_at > DATE_TRUNC('month', NOW() - INTERVAL '3 months')
GROUP BY DATE_TRUNC('month', u.created_at), p.id, p.name
ORDER BY month DESC, provider;
```

### Audit Concerns

| Concern | Mitigation |
|---------|------------|
| Events not reaching Lago | Monitor `lago_event_sync` pending/failed status |
| Token count mismatch | Compare `our_token_count` vs `provider_token_count` |
| Webhook delivery failures | Lago retries automatically; monitor for alerts |

---

## Integration Architecture

<!-- ANCHOR: integration-lago-architecture -->
### Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         dapple-prompt                                │
│                                                                      │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────────┐   │
│  │  Dispatch    │───▶│  Resource    │───▶│  Lago Event Service  │   │
│  │  Handler     │    │  Collection  │    │  (outbound)          │   │
│  └──────────────┘    └──────────────┘    └──────────┬───────────┘   │
│                                                      │               │
│  ┌──────────────┐    ┌──────────────┐               │               │
│  │  Conduit     │───▶│  Lago API    │               │               │
│  │  (pre-check) │    │  Client      │               │               │
│  └──────────────┘    └──────────────┘               │               │
└──────────────────────────────────────────────────────┼───────────────┘
                                                       │
                                                       ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         Lago Cluster (K8s)                           │
│                                                                      │
│  lago-api ◀──── POST /events (usage)                                │
│      │    ◀──── GET /customers/{id}/current_usage                   │
│      │    ◀──── GET /subscriptions/{id}/entitlements                │
│      │    ◀──── GET /wallets?external_customer_id=...               │
│      │                                                               │
│      ▼                                                               │
│  Sidekiq Workers → Aggregation → Invoicing                          │
│      │                                                               │
│      ▼                                                               │
│  Webhooks (notifications only) ───────────────────────────────────▶  │
└─────────────────────────────────────────────────────────────────────┘
```

### Implementation Components

| Component | Status | Description |
|-----------|--------|-------------|
| **Lago Event Service** | Not started | HTTP client to POST usage events |
| **Lago API Client** | Not started | HTTP client for usage/entitlement queries |
| **Conduit Pre-flight Check** | Not started | Call Lago API before processing request |
| **Webhook Receiver** | Not started | Log/alert on payment failures, event errors |

### Data Tables

| Table | Anchor | Purpose |
|-------|--------|---------|
| `rev.dispatch_task_attempt_event_llm_usages` | `impl-llm-usage-table` | LLM token consumption |
| `rev.dispatch_task_attempt_event_assets` | `impl-asset-usage-table` | Generated asset tracking |
| `rev.dispatch_task_attempt_event_service_calls` | `impl-service-call-table` | Third-party API calls |
| `rev.lago_event_sync` | `impl-lago-sync-table` | Tracks what we've sent to Lago |
| `rev.billable_entities` | `impl-billable-entities-table` | Customer → Lago mapping |

**Note:** We do NOT store invoices, credits, or subscription history locally. That data lives in Lago.
<!-- /ANCHOR -->

---

## Customer-Subscription Mapping

<!-- ANCHOR: integration-lago-customer-mapping -->
### How Billable Entities Map to Lago

**Mapping Chain:**
```
User / Company
       ↓
BillableEntity (rev.billable_entities)
       ↓
lago_customer_external_id → Lago Customer
       ↓
Lago Customer → Lago Subscriptions (query via API)
```

> **Note:** Subscription data lives in Lago. We do NOT store subscription history locally. Query Lago API for subscription details.

**Key Relationships:**

| Our Entity | Lago Entity | Relationship |
|------------|-------------|--------------|
| `billable_entity` | `customer` | 1:1 via `lago_customer_external_id` |
| `lago_event_sync` | `event` | 1:1 via `lago_transaction_id` |

### External ID Generation

We generate external IDs (what we send to Lago) using predictable patterns:

| Lago Entity | External ID Pattern | Example |
|-------------|---------------------|---------|
| Customer | `dapple_be_{billable_entity.object_id}` | `dapple_be_0192abc...` |
| Subscription | `dapple_sub_{subscription.object_id}` | `dapple_sub_0192def...` |
| Event | `{resource.object_id}` (UUIDv7) | `0192ghi...` |

**Why this pattern:**
- Prefixes prevent ID collisions across different systems
- UUIDv7 in resource object_ids provides idempotency
- Easy to trace back: Lago ID → our record

### Usage Event Routing

When a resource record needs to be sent to Lago, we determine which customer to bill:

```
LLM Usage Record (rev.dispatch_task_attempt_event_llm_usages)
       ↓
Dispatch Task Attempt Event (rev.dispatch_task_attempt_events)
       ↓
Journey (web.journeys) — has user_id and optional billable_entity_id
       ↓
Billable Entity (rev.billable_entities)
       ↓
lago_customer_external_id → Lago Event API
```

> **Note:** We send events to the Lago customer. Lago determines which subscription to apply the event to based on its internal billing rules.

**Resolution Logic:**
1. If journey has explicit `billable_entity_id` → use that
2. Else look up user's billable entity via `user_billable_entities`
3. Else look up user's company's billable entity via company membership + `company_billable_entities`
4. If no billable entity found → log error, do not send to Lago

This allows flexibility:
- User pays their own bill (individual subscription)
- Company pays for all employees (company subscription)
- Explicit override per journey (B2B scenarios)
<!-- /ANCHOR -->


---

## Lago API Endpoints for Access Control

<!-- ANCHOR: integration-lago-api-access -->
### Available APIs

Conduit calls these APIs to check if a request should be allowed:

**1. Current Usage** — "How much has this customer consumed this period?"
```
GET /customers/{external_customer_id}/current_usage?external_subscription_id=...
```
Returns: `amount_cents`, `total_amount_cents`, `charges_usage[]`
Docs: https://getlago.com/docs/api-reference/customer-usage/get-current

**2. Entitlements** — "What features/quotas does this subscription have?"
```
GET /subscriptions/{external_id}/entitlements
```
Returns: feature gates, privileges, allowances, quotas
Docs: https://docs.getlago.com/guide/entitlements

**3. Wallets** — "How many prepaid credits remain?"
```
GET /wallets?external_customer_id=...
```
Returns: `credits_balance`, `ongoing_usage_balance_cents`
Docs: https://getlago.com/docs/api-reference/wallets/get-all

### Pre-flight Check Flow

```rust
// Pseudocode - conduit pre-flight check
async fn check_access(customer_id: &str, subscription_id: &str) -> Result<AccessDecision> {
    // 1. Get current usage
    let usage = lago_client.get_current_usage(customer_id, subscription_id).await?;

    // 2. Get entitlements (cached locally for a few minutes is OK)
    let entitlements = lago_client.get_entitlements(subscription_id).await?;

    // 3. Check feature access
    if !entitlements.has_feature("llm_access") {
        return Ok(AccessDecision::Denied("Feature not enabled"));
    }

    // 4. Check quota
    if let Some(quota) = entitlements.quota("monthly_spend_cents") {
        if usage.total_amount_cents >= quota {
            return Ok(AccessDecision::Denied("Quota exceeded"));
        }
    }

    Ok(AccessDecision::Allowed)
}
```

### Claim-in-Chunks Pattern (Pre-Authorization)

To reduce Lago API calls, conduit can pre-authorize usage in chunks — similar to how credit card holds work:

```
1. Conduit checks Lago: "Does customer have 10,000 tokens available?"
2. If yes, conduit holds a local "claim" for 10,000 tokens
3. Request executes, uses 1,500 tokens
4. Claim is "collected" with actual usage (1,500)
5. Remaining 8,500 from the chunk is available for next requests
6. When chunk depleted, check Lago again for another chunk
```

**Benefits:**
- One Lago API call per N requests (where N = chunk_size / avg_usage)
- Actual usage events sent to Lago reflect real consumption
- No over-billing (claims are pre-auth, events are settlement)

**Implementation:**

```rust
// Pseudocode
struct UsageChunk {
    customer_id: String,
    remaining_tokens: i64,
    expires_at: DateTime<Utc>,  // Short TTL for safety
}

async fn check_and_claim(customer_id: &str, estimated_tokens: i64) -> Result<ClaimId> {
    let mut chunk = get_or_create_chunk(customer_id).await?;

    if chunk.remaining_tokens < estimated_tokens || chunk.is_expired() {
        // Need fresh check from Lago
        let usage = lago_client.get_current_usage(customer_id).await?;
        let available = quota - usage.total_amount_cents;

        // Claim a new chunk (e.g., 10,000 tokens or available, whichever is less)
        let chunk_size = min(10_000, available);
        chunk = create_chunk(customer_id, chunk_size);
    }

    // Deduct from local chunk
    chunk.remaining_tokens -= estimated_tokens;
    save_chunk(chunk).await?;

    Ok(create_claim(customer_id, estimated_tokens))
}

async fn collect_claim(claim_id: ClaimId, actual_tokens: i64) {
    // Record actual usage (may differ from estimated)
    // This becomes the Lago event
    insert_llm_usage(claim_id, actual_tokens).await?;

    // If actual < estimated, credit back to chunk
    let diff = claim.estimated_tokens - actual_tokens;
    if diff > 0 {
        add_to_chunk(claim.customer_id, diff).await?;
    }
}
```

**Chunk Expiry:**
- Chunks should expire quickly (5-10 minutes) to limit exposure if Lago state changes
- On expiry, discard remaining and fetch fresh from Lago
- Conservative: treat expired chunk as zero remaining

**Tables Affected:**
- `rev.resource_claims` — stores the pre-authorized amount and actual collected amount
- Could add `rev.usage_chunks` for local chunk tracking (or use Redis)
<!-- /ANCHOR -->

---

## Local Development

Lago is deployed to the local k8s cluster at:
- **UI:** https://lago.kube.dappleai.dev
- **API:** https://lago-api.kube.dappleai.dev

See `assets/k8s/registry/lago/` for Kubernetes manifests.

### Initial Setup
1. Apply k8s manifests (namespace, secrets, postgres, redis)
2. Run migrations job
3. Deploy services (api, worker, clock, pdf, front)
4. Access UI and create first organization

---

## Related Sections

- [Data Reconciliation](../monitoring/data-reconciliation.md) — Reconciliation procedures
- [Subscriptions](../domain/subscriptions.md) — Subscription domain model
- [Event Correlation](../operations/event-correlation.md) — Event tracking
