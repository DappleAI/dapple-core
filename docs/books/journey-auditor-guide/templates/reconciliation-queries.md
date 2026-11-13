# Reconciliation Queries

SQL patterns for comparing internal data with external systems.

---

## Overview

<!-- ANCHOR: templates-reconciliation-queries -->
Reconciliation queries compare internal data against external system exports. These patterns assume you have access to external data via:
- Imported CSV/JSON files in temp tables
- Direct API queries
- Scheduled data exports

**Important:** Subscription and invoice data lives in Lago. We do not store that data locally. Reconciliation for subscriptions/invoices is done via Lago API, not SQL queries.

**What we DO store locally:**
- `rev.lago_event_sync` — Usage events we've sent to Lago
- `rev.dispatch_task_attempt_event_llm_usages` — LLM token consumption
- `rev.billable_entities` — Customer to Lago customer mapping

Adapt these patterns to your specific data access method.
<!-- /ANCHOR -->

---

## Setup Pattern

<!-- ANCHOR: reconciliation-setup -->
### Creating Temp Tables for External Data

When comparing with Lago exports or provider reports, load into temp tables:

```sql
-- Create temp table for Lago event export (from GET /events)
CREATE TEMP TABLE lago_events_export (
    transaction_id TEXT,
    external_customer_id TEXT,
    code TEXT,
    timestamp TIMESTAMPTZ,
    properties JSONB
);

-- Create temp table for provider usage export
CREATE TEMP TABLE provider_usage_export (
    provider TEXT,
    date DATE,
    reported_tokens BIGINT
);

-- Load from CSV (example)
-- COPY lago_events_export FROM '/path/to/export.csv' WITH CSV HEADER;

-- Or insert programmatically from API response
INSERT INTO lago_events_export VALUES
    ('tx_abc123', 'cust_456', 'llm_tokens', '2024-02-15 10:00:00+00', '{"tokens": 1000}');
```
<!-- /ANCHOR -->

---

## Subscription Reconciliation

<!-- ANCHOR: reconciliation-subscriptions-queries -->
> **Note:** Subscription data lives in Lago. We do not store subscription state locally.

### Billable Entity ↔ Lago Customer Match

Verify all billable entities have corresponding Lago customers:

```sql
-- Billable entities to verify against Lago
SELECT
    be.id,
    be.object_id,
    be.lago_customer_external_id,
    be.created_at
FROM rev.billable_entities be
WHERE be.lago_customer_external_id IS NOT NULL;
-- Then call Lago API: GET /customers/{external_id}
-- Verify customer exists and has expected subscriptions
```

### Missing Lago Mapping

```sql
-- Billable entities without Lago customer ID
SELECT
    be.id,
    be.object_id,
    be.created_at
FROM rev.billable_entities be
WHERE be.lago_customer_external_id IS NULL
  AND be.created_at < NOW() - INTERVAL '1 day';
```

For subscription state verification, use Lago API:
- `GET /subscriptions?external_customer_id={id}` — List customer's subscriptions
- `GET /subscriptions/{external_id}` — Get specific subscription details
<!-- /ANCHOR -->

---

## Invoice Reconciliation

<!-- ANCHOR: reconciliation-invoices-queries -->
> **Note:** Invoice data lives in Lago. We do not store invoices locally.

### Usage Events → Invoice Verification

Compare usage events we sent to Lago with resulting invoices:

```sql
-- Monthly usage events synced to Lago by customer
SELECT
    be.lago_customer_external_id,
    DATE_TRUNC('month', les.synced_at) AS month,
    COUNT(*) AS events_synced,
    SUM(CASE WHEN rt.code = 'llm_tokens' THEN 1 ELSE 0 END) AS llm_events,
    SUM(CASE WHEN rt.code = 'asset_generation' THEN 1 ELSE 0 END) AS asset_events
FROM rev.lago_event_sync les
JOIN rev.billable_entities be ON be.id = les.billable_entity_id
JOIN rev.resource_types rt ON rt.id = les.resource_type_id
WHERE les.sync_status = 'synced'
  AND les.synced_at > DATE_TRUNC('month', NOW() - INTERVAL '3 months')
GROUP BY be.lago_customer_external_id, DATE_TRUNC('month', les.synced_at)
ORDER BY month DESC, events_synced DESC;
-- Then call Lago API: GET /invoices?external_customer_id={id}&issuing_date_from=...
-- Verify invoice amounts reflect expected usage
```

For invoice details, use Lago API:
- `GET /invoices?external_customer_id={id}` — List customer's invoices
- `GET /invoices/{lago_id}` — Get specific invoice details
<!-- /ANCHOR -->

---

## LLM Token Reconciliation

<!-- ANCHOR: reconciliation-tokens-queries -->
### Our Count vs Provider Count

Compare our token counts with what providers report:

```sql
-- Token count variance by provider
SELECT
    p.name AS provider,
    DATE_TRUNC('month', u.created_at) AS month,
    SUM(u.our_token_count) AS our_tokens,
    SUM(u.provider_token_count) AS provider_tokens,
    SUM(u.provider_token_count) - SUM(u.our_token_count) AS variance,
    ROUND(100.0 * (SUM(u.provider_token_count) - SUM(u.our_token_count))
        / NULLIF(SUM(u.provider_token_count), 0), 2) AS variance_pct
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.providers p ON p.id = u.provider_id
WHERE u.created_at > DATE_TRUNC('month', NOW() - INTERVAL '3 months')
  AND u.provider_token_count IS NOT NULL
GROUP BY p.name, DATE_TRUNC('month', u.created_at)
ORDER BY month DESC, provider;
```

### High Variance Models

```sql
-- Models with significant token count variance
SELECT
    p.name AS provider,
    mr.name AS model,
    COUNT(*) AS call_count,
    AVG(ABS(u.our_token_count - u.provider_token_count))::int AS avg_variance
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.providers p ON p.id = u.provider_id
JOIN rev.model_releases mr ON mr.id = u.model_release_id
WHERE u.created_at > NOW() - INTERVAL '7 days'
  AND u.provider_token_count IS NOT NULL
GROUP BY p.name, mr.name
HAVING AVG(ABS(u.our_token_count - u.provider_token_count)) > 10
ORDER BY avg_variance DESC;
```
<!-- /ANCHOR -->

---

## Plan/Product Reconciliation

<!-- ANCHOR: reconciliation-products-queries -->
> **Note:** Plan and product data lives in Lago. We do not store product catalogs locally.

### Plan Verification

Plans (products) are configured in Lago. Verify via:
- `GET /plans` — List all plans
- `GET /plans/{code}` — Get specific plan details

For reconciliation, compare Lago plans with your expected product configuration (spreadsheet, documentation, etc.).
<!-- /ANCHOR -->

---

## Customer Reconciliation

<!-- ANCHOR: reconciliation-customers-queries -->
### Billable Entity ↔ Lago Customer Match

```sql
-- Count of billable entities with/without Lago mapping
SELECT
    CASE
        WHEN be.lago_customer_external_id IS NOT NULL THEN 'mapped'
        ELSE 'unmapped'
    END AS status,
    COUNT(*) AS count
FROM rev.billable_entities be
GROUP BY 1;
```

### Verify Lago Customers Exist

```sql
-- Get billable entities to verify in Lago
SELECT
    be.id,
    be.object_id,
    be.lago_customer_external_id
FROM rev.billable_entities be
WHERE be.lago_customer_external_id IS NOT NULL;
-- Then call Lago API: GET /customers/{external_id}
-- Verify customer exists
```

For customer details, use Lago API:
- `GET /customers` — List all customers
- `GET /customers/{external_id}` — Get specific customer
<!-- /ANCHOR -->

---

## Reconciliation Summary

<!-- ANCHOR: reconciliation-summary -->
### Generate Reconciliation Report

```sql
-- Summary reconciliation report for data we track locally
WITH lago_sync_check AS (
    SELECT
        'Lago Event Sync' AS category,
        COUNT(*) AS total_checked,
        COUNT(*) FILTER (WHERE sync_status = 'failed') AS issues
    FROM rev.lago_event_sync
    WHERE created_at > NOW() - INTERVAL '7 days'
),
customer_mapping_check AS (
    SELECT
        'Customer Mapping' AS category,
        COUNT(*) AS total_checked,
        COUNT(*) FILTER (WHERE lago_customer_external_id IS NULL) AS issues
    FROM rev.billable_entities
    WHERE created_at < NOW() - INTERVAL '1 day'
),
token_variance_check AS (
    SELECT
        'Token Variance' AS category,
        COUNT(*) AS total_checked,
        COUNT(*) FILTER (WHERE ABS(our_token_count - provider_token_count) > 100) AS issues
    FROM rev.dispatch_task_attempt_event_llm_usages
    WHERE created_at > NOW() - INTERVAL '7 days'
      AND provider_token_count IS NOT NULL
)
SELECT
    category,
    total_checked,
    issues,
    ROUND((total_checked - issues)::numeric / NULLIF(total_checked, 0) * 100, 2) AS success_rate_pct
FROM (
    SELECT * FROM lago_sync_check
    UNION ALL SELECT * FROM customer_mapping_check
    UNION ALL SELECT * FROM token_variance_check
) combined;
```

> **Note:** For subscription and invoice reconciliation, use Lago API exports and compare manually or via scripted checks.
<!-- /ANCHOR -->

---

## Related Sections

- [Data Reconciliation](../monitoring/data-reconciliation.md) — Procedures
- [Audit Queries](audit-queries.md) — Internal audit patterns
- [Integration Points](integration-points.md) — External systems
