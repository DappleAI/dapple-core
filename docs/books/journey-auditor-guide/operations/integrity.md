# Operational Integrity

Controls and behaviors that enable accurate, auditable billing and a 3-day month-end close.

---

## Philosophy

<!-- ANCHOR: integrity-philosophy -->
Operational integrity is not about compliance theater. It's about building systems and behaviors that:

- Surface problems immediately, not at month-end
- Prove that problems were handled, by whom, and how
- Make the 3-day close possible through automation and trust in data
- Give RevOps and Product a framework for thinking about data quality

**The goal:** When the books close on day 3, there are no surprises because issues were detected and resolved in real-time throughout the month.
<!-- /ANCHOR -->

---

## Principles

<!-- ANCHOR: integrity-principles -->
### 1. Detectable

Problems surface automatically via audit queries. Every control is a query that should return zero rows.

```sql
-- Example: Orphan LLM calls (usage without resource record)
SELECT ... WHERE NOT EXISTS (resource record)
-- Expected: 0 rows
```

### 2. Trackable

Every problem gets a ticket. No silent fixes.

| Bad | Good |
|-----|------|
| "I fixed it" | "Fixed in Issue #142" |
| Manual DB update | Amendment record with `issue_reference` |

### 3. Owned

Someone specific is responsible for resolution. Not "the team" — a name.

### 4. Decided

The resolution approach is documented. Why this fix? What was considered?

### 5. Verifiable

After resolution, the original query returns zero rows. Close the loop.
<!-- /ANCHOR -->

---

## Control Patterns

<!-- ANCHOR: integrity-control-patterns -->
### Continuous Checks

These run automatically and flag issues immediately:

| Control | Query | Expected | Frequency |
|---------|-------|----------|-----------|
| No orphan LLM calls | Events without resource records | 0 rows | Continuous |
| Lago sync current | Pending syncs > 1 hour old | 0 rows | Hourly |
| Resource-event linkage | Resources without parent event | 0 rows | Continuous |
| Void/amendment integrity | Amendments without issue_reference | 0 rows | Continuous |

### Period-End Checks

These run at month-end to verify close readiness:

| Control | Query | Expected |
|---------|-------|----------|
| All usage synced to Lago | Pending or failed syncs | 0 rows |
| No unresolved voids | Voids without corresponding amendments | 0 rows |
| Reconciliation complete | Internal totals vs Lago totals | Variance < threshold |

### Example Flow

```
Day N:    Audit query returns 3 rows (orphan LLM calls)
          ↓
Day N:    Issue #142 created automatically or by alert
          ↓
Day N+1:  @engineer assigned, investigates
          ↓
Day N+1:  Root cause: race condition in conduit
          Decision: backfill via amendments, fix code
          ↓
Day N+2:  Amendments created (issue_reference = 'Issue #142')
          Code fix deployed
          ↓
Day N+2:  Query returns 0 rows
          Issue #142 closed with verification link
```
<!-- /ANCHOR -->

---

## Data Correction Model

<!-- ANCHOR: integrity-corrections -->
### Separation of Concerns

| Domain | Owner | Correction Mechanism |
|--------|-------|---------------------|
| Business adjustments | Lago | Credit notes, refunds |
| Usage facts (data errors) | Platform | Voids + Amendments |

Lago handles billing adjustments (customer credits, pricing errors, refunds). We handle data errors (wrong values recorded, missing records).

### Correction Flow

**For data errors (wrong value recorded):**

```
Original record (wrong: 5000 tokens)
        ↓
Void record created
  - References original
  - Requires issue_reference
  - Requires voided_by
        ↓
Amendment record created
  - References void
  - Contains correct data (500 tokens)
  - Requires issue_reference
  - Requires amended_by
```

**For missing data (backfill):**

```
No original record exists
        ↓
Amendment record created
  - No void reference (NULL)
  - Contains backfilled data
  - Requires issue_reference
  - Requires amended_by
```

### Effective Usage Calculation

```
Effective usage = Original records WHERE NOT voided
                + Amendment records
```

Original records are never modified or deleted. Voids mark them invalid. Amendments provide correct values.
<!-- /ANCHOR -->

---

## Lago Sync Integrity

<!-- ANCHOR: integrity-lago-sync -->
### Sync States

| State | Meaning | Action |
|-------|---------|--------|
| Pending | Not yet sent | Will be picked up by sync job |
| Sent | Lago accepted | None |
| Failed | Lago rejected | Investigate, retry or void |
| Retrying | Scheduled for retry | Monitor |

### Reconciliation

Regular comparison between:
1. **Our resource tables** — What we recorded happened
2. **Our sync table** — What we told Lago
3. **Lago's records** — What Lago has

Discrepancies trigger alerts and tickets.

### Amendment Sync

When amendments are created:
1. If original was already synced to Lago → send correcting event
2. If original was not yet synced → void the pending sync, send amendment instead
3. Backfills (no original) → send as new event
<!-- /ANCHOR -->

---

## Audit Queries

### Orphan Resources

```sql
-- LLM usage without parent event (should never happen)
SELECT u.object_id, u.created_at
FROM rev.dispatch_task_attempt_event_llm_usages u
LEFT JOIN rev.dispatch_task_attempt_events e ON e.id = u.event_id
WHERE e.id IS NULL;
```

### Voids Without Amendments

```sql
-- Voided records that were never corrected
SELECT v.object_id, v.original_object_id, v.issue_reference
FROM rev.resource_voids v
LEFT JOIN rev.resource_amendments a ON a.void_object_id = v.object_id
WHERE a.id IS NULL
  AND v.created_at < now() - interval '24 hours';
```

### Amendments Without Tickets

```sql
-- Should return 0 rows (constraint enforces this, but verify)
SELECT object_id, created_at, amended_by
FROM rev.resource_amendments
WHERE issue_reference IS NULL OR issue_reference = '';
```

### Lago Sync Backlog

```sql
-- Syncs pending too long
SELECT resource_type, resource_object_id, created_at
FROM rev.lago_event_sync
WHERE sent_at IS NULL
  AND created_at < now() - interval '1 hour';
```

---

## Related Sections

- [Data Audits](../monitoring/data-audits.md) — Audit query patterns
- [Data Reconciliation](../monitoring/data-reconciliation.md) — Cross-system checks
- [Usage Tracking](../domain/usage-tracking.md) — Resource table implementations
- [Lago Integration](../integrations/recurring-billing.md) — Sync details
