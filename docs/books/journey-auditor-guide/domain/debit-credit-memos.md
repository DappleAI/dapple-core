# Debit & Credit Memos

Invoice lifecycle, credit application, and the foundation of revenue recognition.

> **Lago Note:** In Lago, debit memos are **Invoices** and credit memos are **Credit Notes**. The concepts are equivalent—we use traditional accounting terminology in this guide.

> **Important:** Invoice and credit note data lives in Lago. We do NOT store this data locally. This document explains the concepts for auditing purposes; query Lago API for actual data.

---

## Overview

<!-- ANCHOR: domain-memos -->
Memos are the core billing documents:
- **Debit Memos** represent invoices for services (Lago: Invoices)
- **Credit Memos** represent credits against future invoices (Lago: Credit Notes)

Both have date ranges that drive GAAP revenue recognition calculations.

**Data Location:** Lago is the source of truth. Query via:
- `GET /invoices` — List invoices
- `GET /invoices/{lago_id}` — Get invoice details
- `GET /credit_notes` — List credit notes
- `GET /credit_notes/{lago_id}` — Get credit note details
<!-- /ANCHOR -->

---

## Debit Memos

<!-- ANCHOR: domain-debit-memos -->
A **Debit Memo** is an invoice with start and end dates defining the service period.

### Key Fields
| Field | Description | Audit Importance |
|-------|-------------|------------------|
| `amount` | Total invoice amount | Must match line items |
| `start_date` | Service period begins | Drives recognition start |
| `end_date` | Service period ends | Drives recognition end |
| `issued_at` | When invoice was created | Audit trail |
| `customer_id` | Who is billed | Reconciliation key |

### Creation Triggers
- Subscription renewal
- One-time token purchase
- Usage overage
- Manual adjustment

### Lifecycle
```
Created → Issued → Paid → Recognized (daily)
            ↓
    (optional) Partially Credited
            ↓
    (optional) Fully Credited/Voided
```

### Audit Concerns
- Start date must be before or equal to end date
- Amount must match subscription/purchase terms
- Date range must align with billing period
- All debit memos must have audit trail
<!-- /ANCHOR -->

---

## Credit Memos

<!-- ANCHOR: domain-credit-memos -->
A **Credit Memo** reduces amounts owed, with its own date range for recognition.

### Key Fields
| Field | Description | Audit Importance |
|-------|-------------|------------------|
| `amount` | Credit amount | Must not exceed debit |
| `start_date` | Credit period begins | Must not precede debit start |
| `end_date` | Credit period ends | Recognition calculation |
| `debit_memo_id` | Associated invoice | Required linkage |
| `reason` | Why credit was issued | Audit trail |

### Creation Triggers
- Service credit (SLA violation)
- Pricing adjustment
- Partial refund
- Promotional discount

### Audit Rules (Critical)
These are "things that should never happen":

1. **Credit start after end**: `start_date > end_date`
2. **Credit before debit**: Credit `start_date` before debit memo's `start_date`
3. **Credit exceeds debit**: Credit amount greater than remaining debit balance
4. **Orphan credit**: Credit memo without associated debit memo

### Lifecycle
```
Created → Linked to Debit Memo → Applied to Balance → Recognition Adjusted
```
<!-- /ANCHOR -->

---

## Date Range Calculations

<!-- ANCHOR: domain-memo-dates -->
The date range on memos drives daily revenue recognition.

### Days in Period
```
days_in_period = end_date - start_date + 1
```

### Daily Value
```
daily_value = amount / days_in_period
```

### Example
- Debit memo: $300, Jan 1 - Jan 30 (30 days)
- Daily recognition: $10/day
- Credit memo: $30, Jan 1 - Jan 30
- Daily credit: $1/day
- Net daily: $9/day

### Partial Period Handling
When memos span month boundaries:
- Calculate days in each month
- Apportion amount by days
- Book appropriate amount per month
<!-- /ANCHOR -->

---

## Memo Relationships

```
Customer
    └── Debit Memos (invoices)
            └── Line Items
            └── Credit Memos (applied)
                    └── Recognition adjustments
```

### Line Items
Each debit memo has line items:
- Product/service description
- Quantity and unit price
- Tax calculations
- Subtotals

### Credit Application
Credits can be:
- Fully applied to one debit memo
- Split across multiple debit memos
- Held as account credit for future invoices

---

## Audit Queries

> **Note:** Invoice and credit note data lives in Lago. Use Lago API or export data to perform these audits.

### Audit via Lago API

**Invalid Credit Dates:**
- `GET /credit_notes` and filter where `credit_note.issuing_date` relationships are invalid

**Credit Before Invoice:**
- Compare `credit_note.invoice.issuing_date` with credit note date

**Over-Credited Invoices:**
- Check `invoice.credit_notes_amount_cents` vs `invoice.total_amount_cents`

### Audit via Lago Export

For bulk audits, export Lago data and load into temp tables:

```sql
-- Load Lago invoice export into temp table
CREATE TEMP TABLE lago_invoices (
    lago_id TEXT,
    external_customer_id TEXT,
    total_amount_cents BIGINT,
    credit_notes_amount_cents BIGINT,
    issuing_date DATE,
    status TEXT
);

-- Check for over-credited invoices
SELECT *
FROM lago_invoices
WHERE credit_notes_amount_cents > total_amount_cents;
```

---

## Implementation Notes

**We do NOT store invoice/credit note data locally.**

Lago is the source of truth for:
- Invoice amounts and line items
- Credit note amounts and reasons
- Payment status
- Revenue recognition dates

For revenue reporting, export from Lago or use Lago's built-in reports.

---

## Related Sections

- [Revenue Recognition](revenue-recognition.md) — GAAP daily calculations
- [Lago Integration](../integrations/recurring-billing.md) — Invoice/credit note webhooks
- [Glossary: Debit Memo](../glossary.md#debit-memo)
- [Glossary: Credit Memo](../glossary.md#credit-memo)
- [Data Audits](../monitoring/data-audits.md) — Audit procedures
