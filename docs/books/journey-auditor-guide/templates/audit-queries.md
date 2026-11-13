# Audit Queries

Common SQL patterns for auditing the `rev.*` schema.

---

## Overview

<!-- ANCHOR: templates-audit-queries -->
This section provides reusable SQL query patterns for auditing billing and revenue data.

**Important:** Subscription, invoice, and credit note data lives in Lago. For those audits, use Lago API or export data. This document focuses on tables we store locally.

**Tables we audit locally:**
- `rev.lago_event_sync` — Usage events sent to Lago
- `rev.dispatch_task_attempt_event_llm_usages` — LLM token consumption
- `rev.dispatch_task_attempt_event_assets` — Generated assets
- `rev.dispatch_task_attempt_event_service_calls` — Third-party API calls
- `rev.billable_entities` — Customer to Lago mapping
- `rev.resource_claims` — Resource consumption claims
<!-- /ANCHOR -->

---

## Lago Event Sync

<!-- ANCHOR: audit-queries-lago-sync -->
### Sync Status Distribution
```sql
-- Sync status distribution by date
SELECT
    DATE(created_at) AS sync_date,
    sync_status,
    COUNT(*) AS count
FROM rev.lago_event_sync
WHERE created_at > NOW() - INTERVAL '7 days'
GROUP BY DATE(created_at), sync_status
ORDER BY sync_date DESC, sync_status;
```

### Failed Syncs
```sql
-- Failed syncs needing investigation
SELECT
    id,
    resource_object_id,
    sync_status,
    error_message,
    retry_count,
    failed_at
FROM rev.lago_event_sync
WHERE sync_status = 'failed'
  AND created_at > NOW() - INTERVAL '7 days'
ORDER BY failed_at DESC;
```

### Pending Syncs Too Long
```sql
-- Events that should have synced but haven't
SELECT
    id,
    resource_object_id,
    sync_status,
    created_at,
    NOW() - created_at AS pending_duration
FROM rev.lago_event_sync
WHERE sync_status = 'pending'
  AND created_at < NOW() - INTERVAL '4 hours'
ORDER BY created_at;
```

### Sync Success Rate
```sql
-- Sync success rate by day
SELECT
    DATE(created_at) AS sync_date,
    COUNT(*) AS total,
    COUNT(*) FILTER (WHERE sync_status = 'synced') AS synced,
    ROUND(100.0 * COUNT(*) FILTER (WHERE sync_status = 'synced') / COUNT(*), 2) AS success_rate_pct
FROM rev.lago_event_sync
WHERE created_at > NOW() - INTERVAL '30 days'
GROUP BY DATE(created_at)
ORDER BY sync_date DESC;
```
<!-- /ANCHOR -->

---

## LLM Token Usage

<!-- ANCHOR: audit-queries-llm-usage -->
### Token Usage by Provider
```sql
-- Token consumption by provider
SELECT
    p.name AS provider,
    COUNT(*) AS request_count,
    SUM(u.our_token_count) AS total_tokens,
    AVG(u.our_token_count)::int AS avg_tokens_per_call
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.providers p ON p.id = u.provider_id
WHERE u.created_at > NOW() - INTERVAL '30 days'
GROUP BY p.id, p.name
ORDER BY total_tokens DESC;
```

### Token Count Variance
```sql
-- Requests where our count differs significantly from provider count
SELECT
    u.id,
    u.object_id,
    p.name AS provider,
    mr.name AS model,
    u.our_token_count,
    u.provider_token_count,
    ABS(u.our_token_count - u.provider_token_count) AS variance
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.providers p ON p.id = u.provider_id
JOIN rev.model_releases mr ON mr.id = u.model_release_id
WHERE u.provider_token_count IS NOT NULL
  AND ABS(u.our_token_count - u.provider_token_count) > 100
  AND u.created_at > NOW() - INTERVAL '7 days'
ORDER BY variance DESC
LIMIT 50;
```

### Usage by Model Tier
```sql
-- Token usage by model tier (haiku, sonnet, opus)
SELECT
    mr.tier,
    COUNT(*) AS request_count,
    SUM(u.our_token_count) AS total_tokens,
    AVG(u.our_token_count)::int AS avg_tokens
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.model_releases mr ON mr.id = u.model_release_id
WHERE u.created_at > NOW() - INTERVAL '30 days'
GROUP BY mr.tier
ORDER BY total_tokens DESC;
```

### Daily Token Consumption
```sql
-- Token consumption over time
SELECT
    DATE(created_at) AS day,
    COUNT(*) AS request_count,
    SUM(our_token_count) AS total_tokens
FROM rev.dispatch_task_attempt_event_llm_usages
WHERE created_at > NOW() - INTERVAL '30 days'
GROUP BY DATE(created_at)
ORDER BY day DESC;
```
<!-- /ANCHOR -->

---

## Billable Entity Health

<!-- ANCHOR: audit-queries-billable-entities -->
### Mapping Status
```sql
-- Billable entities with/without Lago customer mapping
SELECT
    CASE
        WHEN lago_customer_external_id IS NOT NULL THEN 'mapped'
        ELSE 'unmapped'
    END AS status,
    COUNT(*) AS count
FROM rev.billable_entities
GROUP BY 1;
```

### Unmapped Entities
```sql
-- Billable entities missing Lago customer ID
SELECT
    id,
    object_id,
    created_at
FROM rev.billable_entities
WHERE lago_customer_external_id IS NULL
  AND created_at < NOW() - INTERVAL '1 day'
ORDER BY created_at;
```

### Usage by Billable Entity
```sql
-- Top billable entities by event count
SELECT
    be.id,
    be.object_id,
    be.lago_customer_external_id,
    COUNT(les.id) AS event_count
FROM rev.billable_entities be
LEFT JOIN rev.lago_event_sync les ON les.billable_entity_id = be.id
WHERE les.created_at > NOW() - INTERVAL '30 days'
GROUP BY be.id, be.object_id, be.lago_customer_external_id
ORDER BY event_count DESC
LIMIT 20;
```
<!-- /ANCHOR -->

---

## Resource Claims

<!-- ANCHOR: audit-queries-claims -->
### Orphan Claims
```sql
-- Claims with no collected resources
SELECT
    c.id,
    c.object_id,
    c.dispatch_task_attempt_event_id,
    c.created_at
FROM rev.resource_claims c
LEFT JOIN rev.dispatch_task_attempt_event_llm_usages u ON u.claim_id = c.id
LEFT JOIN rev.dispatch_task_attempt_event_assets a ON a.claim_id = c.id
LEFT JOIN rev.dispatch_task_attempt_event_service_calls s ON s.claim_id = c.id
WHERE u.id IS NULL
  AND a.id IS NULL
  AND s.id IS NULL
  AND c.created_at < NOW() - INTERVAL '1 hour';
```

### Claims by Type
```sql
-- Resource claims by type
SELECT
    rt.code AS resource_type,
    COUNT(*) AS claim_count
FROM rev.resource_claims c
JOIN rev.resource_types rt ON rt.id = c.resource_type_id
WHERE c.created_at > NOW() - INTERVAL '7 days'
GROUP BY rt.code
ORDER BY claim_count DESC;
```
<!-- /ANCHOR -->

---

## Billing/Revenue Queries (Lago)

> **Note:** Invoice, subscription, and revenue recognition data lives in Lago.

For these audits, use:
- **Lago API:** `GET /invoices`, `GET /subscriptions`, `GET /credit_notes`
- **Lago Dashboard:** Built-in reports and analytics
- **Lago Export:** Export data and load into temp tables for SQL analysis

See [Reconciliation Queries](reconciliation-queries.md) for patterns to compare internal data with Lago exports.

---

## Related Sections

- [Data Audits](../monitoring/data-audits.md) — Integrity checks
- [Schema Reference](schema-reference.md) — Table documentation
- [Reconciliation Queries](reconciliation-queries.md) — Comparison patterns
