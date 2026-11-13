# Revenue Recognition

GAAP-compliant daily revenue recognition and month-end close procedures.

> **Note:** Lago provides usage aggregation and invoicing. Revenue recognition (GAAP compliance) may require additional systems like Zuora Revenue or manual accounting processes.

---

## Overview

<!-- ANCHOR: domain-revenue-recognition -->
Revenue recognition follows GAAP principles:
- Revenue is recognized **daily** over the service period
- Debit memos define the recognition period
- Credit memos reduce daily recognized amounts
- Month-end aggregates applicable daily values

This ensures revenue is recognized when earned, not when billed.
<!-- /ANCHOR -->

---

## Daily Recognition

<!-- ANCHOR: domain-daily-recognition -->
Revenue is calculated and recognized on a daily basis.

### Formula
```
Daily Revenue = (Debit Amount - Credit Amount) / Days in Service Period
```

### Components
| Component | Source | Calculation |
|-----------|--------|-------------|
| Debit Amount | Debit memo | Invoice total |
| Credit Amount | Credit memos | Sum of applied credits |
| Days in Period | Memo dates | end_date - start_date + 1 |

### Example Calculation
**Scenario:** $300 subscription, 30-day period, $30 credit applied

```
Gross Daily = $300 / 30 = $10.00
Credit Daily = $30 / 30 = $1.00
Net Daily = $10.00 - $1.00 = $9.00
```

**Monthly Recognition (30 days):** $9.00 × 30 = $270.00
<!-- /ANCHOR -->

---

## Recognition Timing

<!-- ANCHOR: domain-recognition-timing -->
### When Revenue is Recognized
- **Subscription services:** Daily over the subscription period
- **One-time tokens:** When token is consumed (ability usage)
- **Usage-based:** When usage occurs
- **Professional services:** Per delivery milestones or time-based

### When Revenue is NOT Recognized
- On invoice issuance (creates liability instead)
- On payment receipt (payment ≠ recognition)
- Before service period begins
- After service period ends

### Deferred Revenue (Liability)
```
Deferred Revenue = Total Invoiced - Total Recognized
```

This liability represents:
- Future service obligations
- Prepaid amounts not yet earned
- At-risk revenue if customer cancels
<!-- /ANCHOR -->

---

## Month-End Close

<!-- ANCHOR: domain-month-end -->
### 5-Day Close Timeline
Month-end books typically close within 5 business days:

| Day | Activity |
|-----|----------|
| Day 1 | Final transactions posted, period locked |
| Day 2 | Preliminary recognition calculated |
| Day 3 | Reconciliation and adjustments |
| Day 4 | Management review |
| Day 5 | Books closed, reports issued |

### Close Procedures
1. **Lock Period:** Prevent new transactions in closed month
2. **Calculate Recognition:** Aggregate daily values
3. **Reconcile:** Compare to external systems
4. **Adjust:** Book any corrections
5. **Report:** Generate financial statements

### Partial Period Handling
Memos spanning month boundaries:
```
Month 1 Days = days in Month 1 that fall within service period
Month 2 Days = days in Month 2 that fall within service period

Month 1 Revenue = Daily Rate × Month 1 Days
Month 2 Revenue = Daily Rate × Month 2 Days
```
<!-- /ANCHOR -->

---

## Recognition by Product Type

### Subscriptions
- **Annual Prepaid:** Recognize 1/365 (or 1/366) daily
- **Monthly:** Recognize over calendar month
- **Quarterly:** Recognize over 90-day period

### Token Purchases
- **Immediate Recognition:** When token is consumed
- **Deferred:** Held as liability until consumption or expiration

### Hybrid Products
- Split into subscription and usage components
- Each component follows its own recognition rules

---

## Revenue Recognition Data

> **Note:** Invoice and credit note data lives in Lago. Revenue recognition calculations must either:
> 1. Query Lago API and compute in application code
> 2. Export Lago data and compute in a separate revenue recognition system (e.g., Zuora Revenue)
> 3. Use Lago's built-in analytics and reports

### Lago API Endpoints for Revenue Data

| Data Needed | Lago API |
|-------------|----------|
| Invoices | `GET /invoices` |
| Invoice details | `GET /invoices/{lago_id}` |
| Credit notes | `GET /credit_notes` |
| Subscription periods | `GET /subscriptions` |

### Recognition Calculation Pattern

For a revenue recognition system consuming Lago data:

```
1. Fetch invoices: GET /invoices?issuing_date_from=...&issuing_date_to=...
2. For each invoice:
   - Get service period from invoice.charges[].from_date / to_date
   - Calculate daily rate: total_amount / days_in_period
   - Apply to each day in period
3. Fetch credit_notes: GET /credit_notes
4. For each credit note:
   - Reduce daily recognition by credit daily rate
5. Aggregate by month for reporting
```

### Deferred Revenue

Deferred revenue (liability) represents future service obligations:
- Total invoiced but not yet recognized
- Query Lago for invoice amounts and compute based on service periods

---

## Related Sections

- [Debit & Credit Memos](debit-credit-memos.md) — Memo structure and rules
- [Finance & Accounting](../audiences/finance-accounting.md) — Role responsibilities
- [Revenue System (Zuora)](../integrations/revenue-system.md) — System details
- [Audit Queries](../templates/audit-queries.md) — More query patterns
