# Subscriptions

Subscription states, lifecycle management, and renewal tracking.

> **Terminology:** This section uses Lago-aligned terminology. See [Glossary](../glossary.md) for the terminology mapping from legacy docs.

---

## Overview

<!-- ANCHOR: domain-subscriptions -->
Subscriptions (Lago Plans) provide ongoing access to platform features:
- Define what abilities a customer can use
- Include billable metric allocations (packages) per period
- May include resource caps (slots, storage) for tier-based limits
- Renew automatically or require manual action

Understanding subscription states is critical for revenue auditing.
<!-- /ANCHOR -->

---

## Subscription States

<!-- ANCHOR: domain-subscription-states -->
### State Definitions

| State | Description | Billing | Access |
|-------|-------------|---------|--------|
| **Demo/Trial** | Evaluation period | None | Limited |
| **Basic** | Entry tier | May be free | Restricted features |
| **Paid** | Active subscription | Recurring | Full access |
| **Past Due** | Payment failed | Pending | Grace period |
| **Suspended** | Payment issues | Blocked | Restricted |
| **Canceled** | Ended by customer | None | Until period end |
| **Expired** | Period ended | None | None |

### State Transitions
```
Demo → Basic → Paid → (Renewal) → Paid
         ↓              ↓
      Expired      Past Due → Suspended → Canceled
                        ↓
                    (Payment) → Paid
```

### Audit Concerns
- State must match billing system state
- Access should reflect current state
- Transitions should be logged with timestamps
<!-- /ANCHOR -->

---

## Subscription Components

<!-- ANCHOR: domain-subscription-components -->
### Core Elements (Lago Terminology)

| Component | Lago Term | Description | Audit Point |
|-----------|-----------|-------------|-------------|
| **Plan** | Plan | Product configuration | Must exist in Lago |
| **Period** | Billing interval | Monthly, annual, etc. | Drives recognition timing |
| **Price** | Plan amount | Recurring charge | Must match Lago plan |
| **Package allocations** | Charges (package type) | Included usage bundles | Resets each period |
| **Resource caps** | (Plan metadata) | Slots, storage limits | Enforced by platform |

### Optional Elements
- **Add-ons:** Additional features or capacity (Lago add-ons)
- **Coupons:** Discounts with terms (Lago coupons)
- **Overages:** Usage beyond package allocation (graduated charges)
- **Wallets:** Prepaid credit balances (Lago wallets)
<!-- /ANCHOR -->

---

## Renewal Processing

<!-- ANCHOR: domain-renewals -->
### Auto-Renewal (Default)
1. Approaching period end (typically 7-30 days)
2. System creates renewal invoice
3. Payment processed
4. New period begins
5. Token allocation resets

### Manual Renewal
1. Customer notified of upcoming expiration
2. Customer confirms renewal
3. Invoice issued
4. Payment received
5. Subscription extended

### Renewal Failures
When payment fails:
1. Subscription enters "Past Due" state
2. Retry attempts scheduled
3. Customer notified
4. If not resolved → "Suspended" state
5. Eventually → "Canceled"

### Audit Concerns
- Renewal dates must align with period boundaries
- Pricing should match contract terms
- Failed renewals need proper state management
<!-- /ANCHOR -->

---

## Lifecycle Events

### Creation Events
- Subscription created
- Initial payment processed
- Access granted

### Modification Events
- Plan upgraded/downgraded
- Add-on added/removed
- Pricing changed

### Renewal Events
- Renewal scheduled
- Invoice generated
- Payment processed (or failed)
- Period activated

### Termination Events
- Cancellation requested
- End of period reached
- Access revoked

---

## Audit Queries

> **Note:** Subscription data lives in Lago. These audits are performed via Lago API, not SQL queries against local tables.

### State Reconciliation

**Lago API:** `GET /subscriptions`

Compare subscription status in Lago with expected business rules:
- Subscriptions past `ending_at` should be `terminated`
- Subscriptions with failed payments should be `pending` or have dunning status

### Upcoming Renewals

**Lago API:** `GET /subscriptions?status[]=active`

Filter by `current_billing_period_ending_at` to find subscriptions renewing soon.

### Failed Payments

**Lago API:** `GET /invoices?payment_status=failed`

Lago tracks invoice payment status. Query for failed invoices to identify customers needing attention.

### Usage Verification

**Lago API:** `GET /customers/{external_id}/current_usage`

Compare internal event counts with Lago's aggregated usage to verify sync accuracy:

```sql
-- Count events we sent to Lago in current period
SELECT
    be.billable_entity_id,
    be.lago_customer_external_id,
    COUNT(*) as events_sent
FROM rev.lago_event_sync les
JOIN rev.billable_entities be ON be.id = les.billable_entity_id
WHERE les.synced_at >= date_trunc('month', CURRENT_DATE)
  AND les.sync_status = 'synced'
GROUP BY be.billable_entity_id, be.lago_customer_external_id;
-- Cross-reference with Lago API: GET /customers/{external_id}/current_usage
```

---

## Ability Balance Tracking

<!-- ANCHOR: domain-ability-balance -->
### Balance Types

**Pay-as-you-go (Postpaid):**
- No balance to track locally
- Usage billed at period end
- Query via Lago `GET /current_usage` API

**Package Allocations (Included Usage):**
- Included units per period (e.g., 10,000 tokens/month)
- Resets at period boundary
- Query via Lago `GET /entitlements` API

**Prepaid Credits (Wallets):**
- Customer purchases credits upfront
- Deducted as usage occurs
- Query via Lago `GET /wallets` API

### Where Balance Lives

**Lago is authoritative for ALL balance data:**
- Wallet balances (prepaid credits)
- Package allocation remaining (within period)
- Subscription status
- Entitlements (feature access)

**We do NOT cache locally.** Conduit calls Lago API directly.

### Conduit Pre-flight Check

When conduit receives a request, it calls Lago to check:
1. `GET /current_usage` — How much has been consumed?
2. `GET /entitlements` — What features/quotas are available?
3. `GET /wallets` — Any prepaid credit balance? (if applicable)

If Lago unavailable: **Deny** (safe default)

**Optimization:** Claim-in-chunks pattern reduces API calls by pre-authorizing usage in batches. See [Lago Integration](../integrations/recurring-billing.md).

See [Conduit Routing](../operations/conduit-routing.md) for routing flow details.
<!-- /ANCHOR -->

---

## Related Sections

- [Usage Tracking](usage-tracking.md) — Billable metrics and ability management
- [Revenue Recognition](revenue-recognition.md) — Subscription revenue timing
- [Lago Integration](../integrations/recurring-billing.md) — Recurring billing system
- [Conduit Routing](../operations/conduit-routing.md) — Model selection and routing
- [Data Reconciliation](../monitoring/data-reconciliation.md) — State verification
