# Data Audits

"Things that should never happen" — queries to detect data integrity violations.

---

## Overview

<!-- ANCHOR: monitoring-data-audits -->
Data audits identify integrity violations that indicate bugs, data corruption, or process failures. These are conditions that should **never occur** in a healthy system.

Running these queries regularly (daily or more frequently) helps catch problems before they impact billing or reporting.

**Tables referenced** (from Phases 1-6):
- `rev.resource_claims` — Claims made when resources consumed
- `rev.dispatch_task_attempt_event_llm_usages` — LLM token consumption
- `rev.dispatch_task_attempt_event_assets` — Generated assets
- `rev.dispatch_task_attempt_event_service_calls` — Third-party API calls
- `rev.resource_voids` — Invalid resource records
- `rev.resource_amendments` — Corrected/backfilled data
- `rev.lago_event_sync` — What we've sent to Lago
- `rev.billable_entities` — Who pays
- `rev.dispatch_task_attempt_events` — Parent event table

> **Note:** Subscription and invoice data lives in Lago. Audits for that data use Lago API, not SQL queries.
<!-- /ANCHOR -->

---

## Audit Categories

### Critical (Run Daily)
Violations that directly impact billing or revenue recognition.

### Warning (Run Weekly)
Anomalies that may indicate process issues.

### Informational (Run Monthly)
Patterns that warrant investigation.

---

## Critical Audits

<!-- ANCHOR: audit-critical -->
### Orphan Resource Records

**Rule:** Every resource record must have a valid parent dispatch event.

```sql
-- LLM usages without parent event
SELECT
    u.id,
    u.object_id,
    u.dispatch_task_attempt_event_id,
    u.created_at
FROM rev.dispatch_task_attempt_event_llm_usages u
LEFT JOIN rev.dispatch_task_attempt_events e
    ON e.id = u.dispatch_task_attempt_event_id
WHERE e.id IS NULL;

-- Assets without parent event
SELECT
    a.id,
    a.object_id,
    a.dispatch_task_attempt_event_id,
    a.created_at
FROM rev.dispatch_task_attempt_event_assets a
LEFT JOIN rev.dispatch_task_attempt_events e
    ON e.id = a.dispatch_task_attempt_event_id
WHERE e.id IS NULL;

-- Service calls without parent event
SELECT
    s.id,
    s.object_id,
    s.dispatch_task_attempt_event_id,
    s.created_at
FROM rev.dispatch_task_attempt_event_service_calls s
LEFT JOIN rev.dispatch_task_attempt_events e
    ON e.id = s.dispatch_task_attempt_event_id
WHERE e.id IS NULL;
```

**Action if found:** Immediate investigation required. Orphan records indicate broken event capture.

---

### Claims Without Collection

**Rule:** Every claim should have corresponding resource records (llm_usages, assets, or service_calls).

```sql
-- Claims with no collected resources
SELECT
    c.id,
    c.object_id,
    c.dispatch_task_attempt_event_id,
    rt.code as resource_type,
    c.created_at
FROM rev.resource_claims c
JOIN rev.resource_types rt ON rt.id = c.resource_type_id
LEFT JOIN rev.dispatch_task_attempt_event_llm_usages u
    ON u.claim_id = c.id
LEFT JOIN rev.dispatch_task_attempt_event_assets a
    ON a.claim_id = c.id
LEFT JOIN rev.dispatch_task_attempt_event_service_calls s
    ON s.claim_id = c.id
WHERE u.id IS NULL
  AND a.id IS NULL
  AND s.id IS NULL
  AND c.created_at < NOW() - INTERVAL '1 hour';  -- Allow time for collection
```

**Action if found:** Check dispatch completion logic. Claims should be collected when dispatch completes.

---

### Lago Sync Backlog

**Rule:** All resource records should sync to Lago within 1 hour.

```sql
-- Pending syncs older than 1 hour
SELECT
    les.id,
    les.resource_object_id,
    rt.code as resource_type,
    les.sync_status,
    les.created_at,
    NOW() - les.created_at as age
FROM rev.lago_event_sync les
JOIN rev.resource_types rt ON rt.id = les.resource_type_id
WHERE les.sync_status = 'pending'
  AND les.created_at < NOW() - INTERVAL '1 hour'
ORDER BY les.created_at;
```

**Action if found:** Check Lago service connectivity. Investigate failed_at messages for retries.

---

### Failed Lago Syncs

**Rule:** Failed syncs should be investigated and retried.

```sql
-- Failed syncs not yet retried
SELECT
    les.id,
    les.resource_object_id,
    rt.code as resource_type,
    les.sync_status,
    les.failed_at,
    les.error_message,
    les.retry_count
FROM rev.lago_event_sync les
JOIN rev.resource_types rt ON rt.id = les.resource_type_id
WHERE les.sync_status = 'failed'
  AND les.retry_count < 3
ORDER BY les.failed_at;
```

**Action if found:** Review error messages. May need manual intervention or Lago support.

---

### Token Count Discrepancies

**Rule:** Our token counts should approximately match provider counts.

```sql
-- LLM usages where our count differs significantly from provider count
SELECT
    u.id,
    u.object_id,
    p.name as provider,
    mr.name as model,
    u.our_token_count,
    u.provider_token_count,
    ABS(u.our_token_count - u.provider_token_count) as variance,
    ROUND(100.0 * ABS(u.our_token_count - u.provider_token_count)
        / NULLIF(GREATEST(u.our_token_count, u.provider_token_count), 0), 2) as variance_pct
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.providers p ON p.id = u.provider_id
JOIN rev.model_releases mr ON mr.id = u.model_release_id
WHERE u.provider_token_count IS NOT NULL
  AND ABS(u.our_token_count - u.provider_token_count) > 100  -- Threshold
  AND u.created_at > NOW() - INTERVAL '24 hours'
ORDER BY variance DESC
LIMIT 50;
```

**Action if found:** Investigate tokenization logic. Small variances are expected; large ones indicate bugs.

---

### Billable Entity Without Lago Customer ID

**Rule:** All billable entities should have a `lago_customer_external_id` after creation.

```sql
-- Billable entities missing Lago customer mapping
SELECT
    be.id,
    be.object_id,
    be.created_at
FROM rev.billable_entities be
WHERE be.lago_customer_external_id IS NULL
  AND be.created_at < NOW() - INTERVAL '1 day';  -- Allow time for Lago sync
```

**Action if found:** Check Lago customer creation flow. May indicate API failure during signup.
<!-- /ANCHOR -->

---

## Warning Audits

<!-- ANCHOR: audit-warning -->
### Void Without Amendment

**Rule:** Voided resource records should have corresponding amendments (unless intentionally voided without replacement).

```sql
-- Voids without amendments in last 7 days
SELECT
    rv.id,
    rv.object_id,
    rv.resource_object_id,
    rt.code as resource_type,
    rv.void_reason,
    rv.voided_at
FROM rev.resource_voids rv
JOIN rev.resource_types rt ON rt.id = rv.resource_type_id
LEFT JOIN rev.resource_amendments ra
    ON ra.resource_object_id = rv.resource_object_id
    AND ra.resource_type_id = rv.resource_type_id
WHERE ra.id IS NULL
  AND rv.voided_at > NOW() - INTERVAL '7 days'
  AND rv.void_reason NOT IN ('intentional_void', 'test_data');
```

**Action if found:** Verify if amendment is pending or if void was intentional.

---

### Stale Subscription States

> **Note:** Subscription data lives in Lago. Use Lago API to check for stale subscription states.

**Lago API:** `GET /subscriptions?status[]=pending`

Check for subscriptions stuck in transitional states using Lago dashboard or API.

**Action if found:** Check Lago subscription status. May indicate webhook or payment processing issues.

---

### Orphan Membership Records

**Rule:** Membership records should have valid parent entities.

```sql
-- Company members without valid company
SELECT cm.id, cm.user_id, cm.company_id
FROM rev.company_members cm
LEFT JOIN rev.companies c ON c.id = cm.company_id
WHERE c.id IS NULL;

-- Company members without valid user
SELECT cm.id, cm.user_id, cm.company_id
FROM rev.company_members cm
LEFT JOIN rev.user_details ud ON ud.user_id = cm.user_id
WHERE ud.user_id IS NULL;
```

**Action if found:** Data integrity issue. Check cascade delete settings.

---

### Duplicate Resource Records

**Rule:** Each dispatch event should have at most one resource record per claim.

```sql
-- Duplicate LLM usage records for same claim
SELECT
    claim_id,
    COUNT(*) as record_count
FROM rev.dispatch_task_attempt_event_llm_usages
GROUP BY claim_id
HAVING COUNT(*) > 1;
```

**Action if found:** Investigate collection logic. May indicate retry without idempotency.
<!-- /ANCHOR -->

---

## Informational Audits

<!-- ANCHOR: audit-informational -->
### Usage Patterns by Provider

**Rule:** Monitor token consumption distribution across providers.

```sql
-- Token usage by provider in last 30 days
SELECT
    p.name as provider,
    COUNT(DISTINCT u.dispatch_task_attempt_event_id) as dispatch_count,
    SUM(u.our_token_count) as total_tokens,
    AVG(u.our_token_count)::int as avg_tokens_per_call
FROM rev.dispatch_task_attempt_event_llm_usages u
JOIN rev.providers p ON p.id = u.provider_id
WHERE u.created_at > NOW() - INTERVAL '30 days'
GROUP BY p.id, p.name
ORDER BY total_tokens DESC;
```

---

### Revenue Concentration

**Rule:** Monitor revenue concentration risk.

> **Note:** Invoice data lives in Lago. Use Lago API or dashboard for revenue analysis.

**Lago API:** `GET /invoices` with filters

For proxy analysis, use usage event sync data:

```sql
-- Top 10 billable entities by usage events (last 90 days)
SELECT
    be.id,
    be.object_id,
    be.lago_customer_external_id,
    COUNT(les.id) as event_count
FROM rev.billable_entities be
JOIN rev.lago_event_sync les ON les.billable_entity_id = be.id
WHERE les.synced_at > NOW() - INTERVAL '90 days'
  AND les.sync_status = 'synced'
GROUP BY be.id, be.object_id, be.lago_customer_external_id
ORDER BY event_count DESC
LIMIT 10;
```

---

### Resource Type Distribution

**Rule:** Monitor resource consumption by type.

```sql
-- Resource claims by type in last 7 days
SELECT
    rt.code as resource_type,
    COUNT(*) as claim_count,
    COUNT(DISTINCT c.dispatch_task_attempt_event_id) as unique_events
FROM rev.resource_claims c
JOIN rev.resource_types rt ON rt.id = c.resource_type_id
WHERE c.created_at > NOW() - INTERVAL '7 days'
GROUP BY rt.id, rt.code
ORDER BY claim_count DESC;
```

---

### Lago Sync Success Rate

**Rule:** Monitor sync reliability.

```sql
-- Sync status distribution in last 24 hours
SELECT
    sync_status,
    COUNT(*) as count,
    ROUND(100.0 * COUNT(*) / SUM(COUNT(*)) OVER (), 2) as percentage
FROM rev.lago_event_sync
WHERE created_at > NOW() - INTERVAL '24 hours'
GROUP BY sync_status
ORDER BY count DESC;
```
<!-- /ANCHOR -->

---

## Running Audits

<!-- ANCHOR: impl-audit-infrastructure -->
### Implementation Specification

**Issue #TODO-AUDIT-INFRASTRUCTURE** — Audit execution infrastructure

**Status:** Not Started
**Dependencies:** Phases 1-6 complete

**Components:**

| Component | Description |
|-----------|-------------|
| Audit queries | SQL files in `assets/sql/audits/` |
| Cron jobs | K8s CronJob resources |
| Alert webhook | Notification on critical findings |
| Dashboard | Grafana or similar visualization |

**Automation:**
- Schedule critical audits to run daily via CronJob
- Alert on any non-zero results for critical queries
- Include query results in alert payload for context
- Store audit results in `rev.audit_results` for trending

**Dashboard Integration:**
- Create audit summary dashboard
- Track audit failure trends over time
- Set up escalation procedures

**Acceptance Criteria:**
- [ ] Critical audits run automatically every 24 hours
- [ ] Alerts fire when critical audits return rows
- [ ] Audit results stored for historical analysis
- [ ] Dashboard shows 30-day trend of audit findings
<!-- /ANCHOR -->

---

## Related Sections

- [Data Reconciliation](data-reconciliation.md) — Comparing internal vs external data
- [Audit Queries](../templates/audit-queries.md) — Common query patterns
- [Operational Integrity](../operations/integrity.md) — Integrity principles
