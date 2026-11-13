# Pricing & Discounts

Price adjustments, bundling, discount management, and margin tracking.

> **Lago Note:** Lago handles discounts primarily through **Coupons** (percentage or fixed amount) and **Wallets** (prepaid credits). Complex bundling may require custom plan configuration.

---

## Overview

<!-- ANCHOR: domain-pricing -->
Pricing and discounts directly impact revenue recognition:
- List prices establish the baseline
- Discounts reduce recognized amounts
- Bundles may require allocation across components
- Margins must be tracked for profitability analysis

Accurate pricing audit ensures revenue integrity.
<!-- /ANCHOR -->

---

## Price Architecture

<!-- ANCHOR: domain-price-architecture -->
### Price Components

| Component | Description | Audit Point |
|-----------|-------------|-------------|
| **List Price** | Published standard price | Source of truth |
| **Contract Price** | Customer-specific negotiated price | Must be documented |
| **Promotional Price** | Time-limited offer price | Has start/end dates |
| **Bundle Price** | Combined product pricing | Requires allocation |

### Price Hierarchy
```
List Price
    └── Customer Contract (if exists)
            └── Promotional Override (if active)
                    └── Applied Discounts
                            └── Final Price
```

### Price Versioning
Prices change over time:
- New customers get current prices
- Existing subscriptions may be grandfathered
- Price increases may require notice periods
- Contract terms may lock in prices
<!-- /ANCHOR -->

---

## Discount Types

<!-- ANCHOR: domain-discounts -->
### Percentage Discounts
Reduce price by a percentage:
- Applied to line item or total
- May be conditional (volume, term commitment)
- Must track original amount for recognition

### Fixed Amount Discounts
Reduce price by specific amount:
- Coupons and promotional codes
- Credits applied to invoices
- Negotiated price reductions

### Term Discounts
Reduced rates for commitments:
- Annual vs monthly pricing
- Multi-year agreements
- Volume commitments

### Promotional Discounts
Time-limited offers:
- Have explicit start/end dates
- May be first-period only
- Should revert to standard pricing

### Audit Concerns
- Discounts must not exceed allowed limits
- Approval workflows should be documented
- Impact on margin must be tracked

### Lago-Specific Discount Mechanisms

| Mechanism | Lago Feature | Use Case |
|-----------|--------------|----------|
| **Percentage off** | Coupon (percentage) | "20% off first 3 months" |
| **Fixed amount off** | Coupon (fixed) | "$50 off first invoice" |
| **Prepaid credits** | Wallet | Customer pre-pays for usage |
| **Free trial** | Coupon (100%) | "14-day free trial" |
| **Volume discount** | Graduated charges | Lower per-unit cost at scale |
<!-- /ANCHOR -->

---

## Bundling

<!-- ANCHOR: domain-bundling -->
### Bundle Types

| Type | Description | Revenue Treatment |
|------|-------------|-------------------|
| **Fixed Bundle** | Set products at fixed price | Allocate by relative standalone price |
| **Configurable** | Choose from options | Price based on selections |
| **Usage Bundle** | Includes usage allocation | Split subscription vs usage |

### Revenue Allocation (ASC 606)
When bundles combine multiple elements:
1. Identify distinct performance obligations
2. Determine standalone selling prices
3. Allocate bundle price proportionally
4. Recognize each component per its delivery

### Example Allocation
**Bundle:** Platform + Support + Training = $1000/month

| Component | Standalone Price | Allocation |
|-----------|-----------------|------------|
| Platform | $800 | $666.67 |
| Support | $300 | $250.00 |
| Training | $100 | $83.33 |
| **Total** | $1200 | $1000.00 |

Allocation ratio: $1000 / $1200 = 83.33%
<!-- /ANCHOR -->

---

## Margin Tracking

<!-- ANCHOR: domain-margins -->
### Margin Components

| Metric | Calculation |
|--------|-------------|
| **Gross Margin** | (Revenue - COGS) / Revenue |
| **Net Discount** | Total Discounts / List Price |
| **Effective Price** | Actual Revenue / Expected Revenue |

### Margin by Segment
Track margins across:
- Product lines
- Customer segments
- Sales channels
- Geographic regions

### Margin Alerts
Monitor for:
- Deals below minimum margin threshold
- Excessive discounting patterns
- Negative margin transactions
<!-- /ANCHOR -->

---

## Audit Queries

### Discount Distribution
```sql
-- Distribution of discounts by type
SELECT
    discount_type,
    COUNT(*) AS count,
    AVG(discount_percentage) AS avg_discount,
    MAX(discount_percentage) AS max_discount,
    SUM(discount_amount) AS total_discounted
FROM rev.applied_discounts
WHERE applied_at > CURRENT_DATE - INTERVAL '30 days'
GROUP BY discount_type
ORDER BY total_discounted DESC;
```

### Excessive Discounts
```sql
-- Discounts exceeding threshold (e.g., 30%)
SELECT
    d.id,
    d.customer_id,
    d.discount_percentage,
    d.approved_by,
    d.applied_at
FROM rev.applied_discounts d
WHERE d.discount_percentage > 30
  AND d.applied_at > CURRENT_DATE - INTERVAL '30 days'
ORDER BY d.discount_percentage DESC;
```

### Bundle Allocation Verification
```sql
-- Verify bundle allocations sum correctly
SELECT
    b.bundle_id,
    b.total_price AS bundle_price,
    SUM(ba.allocated_amount) AS allocated_total,
    b.total_price - SUM(ba.allocated_amount) AS variance
FROM rev.bundles b
JOIN rev.bundle_allocations ba ON ba.bundle_id = b.id
GROUP BY b.bundle_id, b.total_price
HAVING ABS(b.total_price - SUM(ba.allocated_amount)) > 0.01;
```

### Margin Analysis
```sql
-- Margin by product
SELECT
    p.name AS product_name,
    SUM(li.net_amount) AS revenue,
    SUM(li.list_amount) AS list_revenue,
    (1 - SUM(li.net_amount) / NULLIF(SUM(li.list_amount), 0)) * 100 AS discount_pct
FROM rev.line_items li
JOIN rev.products p ON p.id = li.product_id
WHERE li.created_at > CURRENT_DATE - INTERVAL '30 days'
GROUP BY p.id, p.name
ORDER BY discount_pct DESC;
```

---

## Related Sections

- [Debit & Credit Memos](debit-credit-memos.md) — How pricing flows to invoices
- [Revenue Recognition](revenue-recognition.md) — Recognition of bundled revenue
- [Lago Integration](../integrations/recurring-billing.md) — Price and coupon configuration
- [Audit Queries](../templates/audit-queries.md) — More query patterns
