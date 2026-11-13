# Data Reconciliation

Comparing internal data against external systems for consistency verification.

---

## Overview

<!-- ANCHOR: monitoring-reconciliation -->
Data reconciliation compares internal records with external system data to identify discrepancies. Unlike data audits (which check internal integrity), reconciliation verifies that systems agree.

**Primary reconciliation points:**
- Internal ↔ Lago (usage events, subscriptions, invoices)
- Internal ↔ LLM Providers (token counts)

**Tables referenced:**
- `rev.lago_event_sync` — What we've sent to Lago
- `rev.dispatch_task_attempt_event_llm_usages` — Token consumption
- `rev.billable_entities` — Customer mapping

> **Note:** Subscription and invoice data lives in Lago. Reconciliation for that data uses Lago API exports, not local tables.
<!-- /ANCHOR -->

---

## Reconciliation Types

### Count Reconciliation
Do record counts match?
- Usage event counts (internal vs Lago)
- Subscription counts
- Invoice counts

### Balance Reconciliation
Do monetary totals match?
- Invoice totals
- Credit totals
- Token consumption totals

### State Reconciliation
Do statuses align?
- Subscription states
- Sync statuses

### Identity Reconciliation
Can records be matched?
- `lago_customer_id` present on billable entities
- `lago_subscription_id` present on subscriptions
- `lago_id` present on invoices

---

## Lago Usage Event Reconciliation

<!-- ANCHOR: reconciliation-lago-events -->
### Sync Status Summary

**Purpose:** Verify all usage events have been synced to Lago.

```sql
-- Sync status distribution by date
SELECT
    DATE(les.created_at) as sync_date,
    rt.code as resource_type,
    les.sync_status,
    COUNT(*) as count
FROM rev.lago_event_sync les
JOIN rev.resource_types rt ON rt.id = les.resource_type_id
WHERE les.created_at > NOW() - INTERVAL '7 days'
GROUP BY DATE(les.created_at), rt.code, les.sync_status
ORDER BY sync_date DESC, rt.code, les.sync_status;
```

### Events Pending Too Long

```sql
-- Events that should have synced but haven't
SELECT
    les.id,
    les.resource_object_id,
    rt.code as resource_type,
    les.sync_status,
    les.created_at,
    les.retry_count,
    NOW() - les.created_at as pending_duration
FROM rev.lago_event_sync les
JOIN rev.resource_types rt ON rt.id = les.resource_type_id
WHERE les.sync_status IN ('pending', 'failed')
  AND les.created_at < NOW() - INTERVAL '4 hours'
ORDER BY les.created_at;
```

### Compare With Lago Export

**Procedure:**
1. Export events from Lago API for date range
2. Load into temp table `temp.lago_events_export`
3. Run comparison query

```sql
-- Events we sent but Lago doesn't have
SELECT
    les.resource_object_id,
    les.lago_transaction_id,
    les.synced_at
FROM rev.lago_event_sync les
LEFT JOIN temp.lago_events_export le
    ON le.transaction_id = les.lago_transaction_id
WHERE les.sync_status = 'synced'
  AND les.synced_at > NOW() - INTERVAL '24 hours'
  AND le.transaction_id IS NULL;

-- Events Lago has that we don't have sync records for
SELECT
    le.transaction_id,
    le.external_customer_id,
    le.timestamp
FROM temp.lago_events_export le
LEFT JOIN rev.lago_event_sync les
    ON les.lago_transaction_id = le.transaction_id
WHERE les.id IS NULL
  AND le.timestamp > NOW() - INTERVAL '24 hours';
```

**Frequency:** Daily

**Action on Variance:** Check sync process, investigate timing issues.
<!-- /ANCHOR -->

---

## Subscription Reconciliation

<!-- ANCHOR: reconciliation-subscriptions -->
> **Note:** Subscription data lives in Lago. We do not store subscription state locally.

### Subscription Verification

**Purpose:** Verify billable entities have valid Lago subscriptions.

**Lago API:** `GET /subscriptions?external_customer_id={id}`

**Procedure:**
1. Query billable entities with `lago_customer_external_id`
2. Call Lago API to verify subscription exists
3. Check subscription status is as expected

```sql
-- Get billable entities to verify against Lago
SELECT
    be.id,
    be.object_id,
    be.lago_customer_external_id
FROM rev.billable_entities be
WHERE be.lago_customer_external_id IS NOT NULL;
-- Then call Lago API: GET /subscriptions?external_customer_id={lago_customer_external_id}
```

**Frequency:** Weekly (spot check) or on-demand

**Action on Variance:** Check Lago dashboard, investigate customer setup flow.
<!-- /ANCHOR -->

---

## Invoice Reconciliation

<!-- ANCHOR: reconciliation-invoices -->
> **Note:** Invoice data lives in Lago. We do not store invoices locally.

### Invoice Verification

**Purpose:** Verify usage events resulted in expected invoices.

**Lago API:** `GET /invoices?external_customer_id={id}`

**Procedure:**
1. Count usage events synced per billable entity per period
2. Call Lago API to retrieve invoices for same period
3. Verify invoice amounts reflect expected usage

```sql
-- Usage events synced per billable entity per month
SELECT
    be.lago_customer_external_id,
    DATE_TRUNC('month', les.synced_at) AS month,
    COUNT(*) AS events_synced
FROM rev.lago_event_sync les
JOIN rev.billable_entities be ON be.id = les.billable_entity_id
WHERE les.sync_status = 'synced'
  AND les.synced_at > DATE_TRUNC('month', NOW() - INTERVAL '3 months')
GROUP BY be.lago_customer_external_id, DATE_TRUNC('month', les.synced_at)
ORDER BY month DESC, events_synced DESC;
-- Then call Lago API: GET /invoices?external_customer_id={id}&issuing_date_from=...
```

**Frequency:** Monthly (during close)

**Action on Variance:** Check Lago dashboard, verify event-to-invoice mapping.
<!-- /ANCHOR -->

---

## Provider Token Reconciliation

<!-- ANCHOR: reconciliation-providers -->
### Token Count Comparison

**Purpose:** Compare our token counts with LLM provider invoices.

```sql
-- Monthly token usage by provider
SELECT
    DATE_TRUNC('month', u.created_at) AS month,
    p.name AS provider,
    SUM(u.our_token_count) AS our_tokens,
    SUM(u.provider_token_count) AS provider_tokens,
    SUM(u.provider_token_count) - SUM(u.our_token_count) AS variance,
    ROUND(100.0 * (SUM(u.provider_token_count) - SUM(u.our_token_count))
        / NULLIF(SUM(u.provider_token_count), 0), 2) AS variance_pct
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.providers p ON p.id = u.provider_id
WHERE u.created_at > DATE_TRUNC('month', NOW() - INTERVAL '3 months')
GROUP BY DATE_TRUNC('month', u.created_at), p.id, p.name
ORDER BY month DESC, provider;
```

### Token Usage by Model

```sql
-- Detailed breakdown by model for variance investigation
SELECT
    p.name AS provider,
    mr.name AS model,
    COUNT(*) AS call_count,
    SUM(u.our_token_count) AS our_tokens,
    SUM(u.provider_token_count) AS provider_tokens,
    AVG(ABS(u.our_token_count - u.provider_token_count))::int AS avg_variance
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.providers p ON p.id = u.provider_id
JOIN rev.model_releases mr ON mr.id = u.model_release_id
WHERE u.created_at > NOW() - INTERVAL '7 days'
  AND u.provider_token_count IS NOT NULL
GROUP BY p.id, p.name, mr.id, mr.name
HAVING AVG(ABS(u.our_token_count - u.provider_token_count)) > 10  -- Threshold
ORDER BY avg_variance DESC;
```

### Provider Invoice Comparison

**Procedure:**
1. Download provider usage reports (Anthropic, OpenAI)
2. Load into temp table `temp.provider_usage_export`
3. Run comparison query

```sql
-- Compare internal totals with provider report
WITH internal_totals AS (
    SELECT
        p.name AS provider,
        DATE_TRUNC('day', u.created_at) AS usage_date,
        SUM(u.our_token_count) AS our_tokens
    FROM rev.dispatch_task_attempt_event_llm_usages u
    JOIN rev.providers p ON p.id = u.provider_id
    WHERE u.created_at > NOW() - INTERVAL '30 days'
    GROUP BY p.name, DATE_TRUNC('day', u.created_at)
)
SELECT
    it.provider,
    it.usage_date,
    it.our_tokens,
    pe.reported_tokens,
    pe.reported_tokens - it.our_tokens AS variance
FROM internal_totals it
LEFT JOIN temp.provider_usage_export pe
    ON pe.provider = it.provider
    AND pe.date = it.usage_date
WHERE ABS(pe.reported_tokens - it.our_tokens) > 1000  -- Threshold
ORDER BY ABS(variance) DESC;
```

**Frequency:** Monthly (with provider billing cycle)

**Action on Variance:** Investigate tokenization differences, check for untracked usage.
<!-- /ANCHOR -->

---

## Customer/Billable Entity Reconciliation

<!-- ANCHOR: reconciliation-customers -->
### Lago Customer Mapping

**Purpose:** Verify all billable entities have corresponding Lago customers.

```sql
-- Billable entities without Lago customer ID
SELECT
    be.id,
    be.object_id,
    be.created_at,
    CASE
        WHEN ube.id IS NOT NULL THEN 'user'
        WHEN cbe.id IS NOT NULL THEN 'company'
    END AS entity_type
FROM rev.billable_entities be
LEFT JOIN rev.user_billable_entities ube ON ube.billable_entity_id = be.id
LEFT JOIN rev.company_billable_entities cbe ON cbe.billable_entity_id = be.id
WHERE be.lago_customer_id IS NULL
  AND be.created_at < NOW() - INTERVAL '1 hour';
```

### Customer Count Match

```sql
-- Count of billable entities by type
SELECT
    CASE
        WHEN ube.id IS NOT NULL THEN 'user'
        WHEN cbe.id IS NOT NULL THEN 'company'
        ELSE 'unknown'
    END AS entity_type,
    COUNT(*) AS count
FROM rev.billable_entities be
LEFT JOIN rev.user_billable_entities ube ON ube.billable_entity_id = be.id
LEFT JOIN rev.company_billable_entities cbe ON cbe.billable_entity_id = be.id
GROUP BY 1
ORDER BY count DESC;

-- Compare with Lago customer count
```

**Frequency:** Weekly

**Action on Variance:** Check customer creation flow, investigate API failures.
<!-- /ANCHOR -->

---

## Reconciliation Procedures

### Daily Reconciliation Checklist

1. **Lago Event Sync**
   - Run sync status query
   - Check for pending > 4 hours
   - Check for failed syncs

2. **Subscription State**
   - Compare with Lago export
   - Check for missing IDs

3. **Invoice Sync**
   - Verify recent invoices have Lago IDs
   - Check webhook delivery logs

### Weekly Reconciliation Checklist

1. **Customer Mapping**
   - All billable entities have `lago_customer_id`
   - Count matches Lago dashboard

2. **Invoice Totals**
   - Sum matches Lago invoice report
   - Outstanding amounts reconcile

3. **Token Variance Analysis**
   - Review provider token discrepancies
   - Investigate models with high variance

### Monthly Close Reconciliation

1. **Full Lago Export**
   - Export all events, subscriptions, invoices for period
   - Load into temp tables
   - Run full comparison queries

2. **Provider Reconciliation**
   - Download Anthropic, OpenAI usage reports
   - Compare with internal totals
   - Document variances

3. **Financial Tie-out**
   - Invoice totals → Revenue system
   - Credit totals → Credit tracking
   - Subscription MRR → Financial planning

---

## Variance Resolution

### Investigation Steps

1. Identify timing of divergence
2. Check system sync logs
3. Review webhook delivery status
4. Verify data transformations
5. Check for manual Lago adjustments

### Common Causes

| Variance Type | Common Cause | Resolution |
|---------------|--------------|------------|
| Missing events in Lago | Sync failure, API timeout | Retry sync, check logs |
| State mismatch | Webhook delivery delay | Wait and re-check |
| Token count difference | Tokenizer version | Document as expected |
| Invoice missing | Webhook failed | Manual webhook replay |

### Resolution Actions

- Correct data in appropriate system
- Document root cause in audit log
- Implement prevention measures
- Update monitoring thresholds if needed

---

## Implementation Specification

<!-- ANCHOR: impl-reconciliation-infrastructure -->
**Issue #TODO-RECONCILIATION-INFRASTRUCTURE** — Reconciliation infrastructure

**Status:** Not Started
**Dependencies:** Phases 1-6 complete

**Components:**

| Component | Description |
|-----------|-------------|
| Lago export script | Nushell script to pull Lago data via API |
| Temp table loader | Load exports into `temp.*` tables |
| Reconciliation queries | SQL files in `assets/sql/reconciliation/` |
| Variance report | Summary output for review |

**Acceptance Criteria:**
- [ ] Daily reconciliation runs automatically
- [ ] Weekly reconciliation includes Lago export comparison
- [ ] Monthly close includes full variance analysis
- [ ] Variances logged with timestamp and resolution
<!-- /ANCHOR -->

---

## Related Sections

- [Data Audits](data-audits.md) — Internal integrity checks
- [Recurring Billing (Lago)](../integrations/recurring-billing.md) — Lago integration details
- [Usage Tracking](../domain/usage-tracking.md) — Resource consumption tables
- [Operational Integrity](../operations/integrity.md) — Integrity principles
