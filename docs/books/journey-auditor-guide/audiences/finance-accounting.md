# Finance & Accounting

Finance and Accounting stakeholders focused on legal compliance, month-end booking, and revenue recognition.

---

## Role Definition

<!-- ANCHOR: audience-finance-accounting -->
Finance and Accounting teams ensure financial accuracy and compliance:
- GAAP-compliant revenue recognition
- Month-end close and booking
- Audit preparation and support
- Financial reporting and projections

### Key Decisions
- Revenue recognition policies
- Accounting treatment of new products
- Credit and refund policies
- Financial statement presentation
<!-- /ANCHOR -->

---

## Audit Questions

Questions Finance & Accounting typically need answered:

### Revenue Recognition
- What's the daily recognized revenue?
- How much deferred revenue (liability) remains?
- Are recognition periods correct for all memos?
- Do calculations match GAAP requirements?

### Month-End Close
- What's the total bookable revenue?
- Are all invoices accounted for?
- Are credits applied correctly?
- What adjustments are needed?

### Compliance
- Are all transactions properly documented?
- Is the audit trail complete?
- Are soft deletes preserved correctly?
- Is PII handled according to policy?

---

## GAAP Revenue Recognition

### Daily Calculation
Revenue is recognized daily over the service period:

```
Daily Revenue = (Debit Memo Amount - Credit Memo Amount) / Days in Period
```

**Example:**
- $300 debit memo over 30 days = $10/day
- $30 credit applied over same period = $1/day reduction
- Net daily recognition = $9/day

### Month-End Aggregation
- Sum all applicable daily values for the month
- Partial months prorate based on calendar days
- Deferred revenue = Unrecognized portion of prepaid amounts

### 5-Day Close Rule
Month-end books typically close within 5 business days. This requires:
- All transactions posted by month end
- Reconciliation completed promptly
- Adjusting entries identified and booked

---

## Liability Tracking

### Deferred Revenue
When customers prepay:
1. Full amount recorded as liability
2. Daily recognition reduces liability
3. Corresponding increase in recognized revenue
4. At period end, liability should be zero (or represent future periods)

### Token Liability
One-time tokens create liability until:
- Consumed by ability usage
- Expired (expiration date passed)
- Refunded (credit memo issued)

---

## Reconciliation Requirements

### Internal vs. External
- Billing system totals vs. internal calculations
- Bank deposits vs. recorded payments
- Subscription counts vs. recognized revenue

### Month-End Checklist
- [ ] All invoices issued and recorded
- [ ] Credits applied correctly
- [ ] Usage data complete
- [ ] Revenue recognition calculated
- [ ] Liability balances verified
- [ ] Reconciliation documented

---

## Related Sections

- [Revenue Recognition](../domain/revenue-recognition.md) — Detailed GAAP procedures
- [Debit & Credit Memos](../domain/debit-credit-memos.md) — Invoice lifecycle
- [Revenue System (Zuora)](../integrations/revenue-system.md) — Recognition system
- [Audit Queries](../templates/audit-queries.md) — SQL patterns for verification
