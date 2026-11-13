# Event Correlation

Internal events mapped to external system event IDs for audit trail.

---

## Overview

<!-- ANCHOR: operations-events -->
Event correlation links internal actions to external system records:
- Every billable action generates an internal event
- Internal events carry external `event_id` from billing system
- This creates an audit chain from user action to invoice

**Audit Chain:**
```
User Action → Internal Event → External Event ID → Billing Record
      ↓              ↓                ↓                   ↓
   Journey     rev.events      Maxio event_id      Invoice line
```
<!-- /ANCHOR -->

---

## Event Structure

<!-- ANCHOR: operations-event-structure -->
### Internal Event Fields

| Field | Description | Audit Use |
|-------|-------------|-----------|
| `id` | Internal event ID | Primary key |
| `event_type` | Action category | Filtering |
| `journey_id` | Associated journey | Attribution |
| `ability_id` | What action occurred | Billing lookup |
| `token_id` | Token consumed (if any) | Billing verification |
| `external_event_id` | Maxio event ID | External correlation |
| `created_at` | When event occurred | Timeline |
| `metadata` | Additional context | Investigation |

### Event Types

| Type | Description | Typically Billable |
|------|-------------|-------------------|
| `journey_created` | Journey started | No |
| `debrief_completed` | Interview finished | No |
| `dispatch_started` | Execution began | No |
| `ability_usage` | Trackable action | Depends on ability |
| `token_consumed` | Token used | Yes |
| `dispatch_completed` | Execution finished | No |
| `error_occurred` | Something failed | No |
<!-- /ANCHOR -->

---

## External Correlation

<!-- ANCHOR: operations-external-correlation -->
### When External ID is Assigned
- Token consumption triggers webhook to Maxio
- Maxio returns `event_id` in response
- Internal event updated with `external_event_id`

### Correlation Flow
```
1. User triggers ability
2. Internal event created (external_event_id = NULL)
3. Token consumption recorded
4. Webhook sent to Maxio
5. Maxio responds with event_id
6. Internal event updated with external_event_id
```

### Failure Handling
If external correlation fails:
- Event remains with `external_event_id = NULL`
- Retry mechanism attempts correlation
- Alert if correlation fails repeatedly
- Manual reconciliation may be needed
<!-- /ANCHOR -->

---

## Audit Queries

### Events Missing External Correlation
```sql
-- Billable events without external ID
SELECT
    e.id,
    e.event_type,
    e.journey_id,
    e.created_at,
    e.external_event_id
FROM rev.events e
WHERE e.event_type IN ('ability_usage', 'token_consumed')
  AND e.external_event_id IS NULL
  AND e.created_at > CURRENT_DATE - INTERVAL '7 days'
ORDER BY e.created_at DESC;
```

### Event Correlation by Journey
```sql
-- All events for a journey with correlation status
SELECT
    e.id,
    e.event_type,
    e.ability_id,
    e.token_id,
    e.external_event_id,
    CASE
        WHEN e.external_event_id IS NOT NULL THEN 'Correlated'
        WHEN e.event_type IN ('ability_usage', 'token_consumed') THEN 'MISSING'
        ELSE 'N/A'
    END AS correlation_status,
    e.created_at
FROM rev.events e
WHERE e.journey_id = $1
ORDER BY e.created_at;
```

### Correlation Success Rate
```sql
-- Correlation success rate by day
SELECT
    date_trunc('day', created_at) AS day,
    COUNT(*) AS total_billable_events,
    COUNT(external_event_id) AS correlated,
    ROUND(COUNT(external_event_id)::numeric / COUNT(*) * 100, 2) AS correlation_pct
FROM rev.events
WHERE event_type IN ('ability_usage', 'token_consumed')
  AND created_at > CURRENT_DATE - INTERVAL '30 days'
GROUP BY date_trunc('day', created_at)
ORDER BY day DESC;
```

### Orphan External Events
```sql
-- External events not matched internally (requires Maxio export)
-- This helps find events sent to Maxio but not recorded internally
SELECT
    me.event_id,
    me.event_type,
    me.created_at
FROM maxio_event_export me
LEFT JOIN rev.events e ON e.external_event_id = me.event_id
WHERE e.id IS NULL
  AND me.created_at > CURRENT_DATE - INTERVAL '7 days';
```

---

## Monitoring

### Real-Time Alerts
Alert when:
- Correlation failure rate exceeds threshold (e.g., >1%)
- Correlation latency exceeds threshold (e.g., >1 minute)
- Batch of events fails correlation

### Daily Reports
Report on:
- Total events by type
- Correlation success rate
- Events pending correlation
- Events requiring manual intervention

---

## Troubleshooting

### Common Issues

**Webhook Timeout:**
- Maxio webhook timed out
- Event created but no external ID
- Resolution: Retry webhook, manual correlation

**Duplicate Events:**
- Same action generated multiple events
- May cause double billing
- Resolution: Deduplication, idempotency keys

**Missing Events:**
- Action occurred but no event logged
- Billing gap
- Resolution: Investigate logging, backfill if needed

---

## Related Sections

- [Usage Tracking](../domain/usage-tracking.md) — Ability and token concepts
- [Data Reconciliation](../monitoring/data-reconciliation.md) — Event reconciliation
- [Recurring Billing (Maxio)](../integrations/recurring-billing.md) — External system
