# A/B Testing Framework

Experimentation infrastructure for pricing, features, and conversion optimization.

---

## Overview

<!-- ANCHOR: ops-ab-testing -->
A/B testing enables data-driven decisions for:
- **Pricing experiments** — Test price points, plan structures
- **Feature experiments** — Test new features with user subsets
- **Conversion experiments** — Optimize upgrade flows, paywalls
- **Model experiments** — Compare LLM quality and cost tradeoffs

**Architecture:**
- **Flagsmith** (self-hosted) — Handles experiment definitions and variant assignment
- **experiment_outcomes table** — We track outcomes locally for analysis and revenue attribution

**Audit Importance:**
- Experiments can affect revenue (pricing tests)
- Need clear attribution for financial reporting
- Must not corrupt billing data
- Results inform product and pricing decisions
<!-- /ANCHOR -->

---

## Experiment Types

<!-- ANCHOR: ops-experiment-types -->
### Pricing Experiments

Test different price points or plan structures.

| Aspect | Consideration |
|--------|---------------|
| **Scope** | New signups only (existing contracts unchanged) |
| **Duration** | Minimum 30 days for statistical significance |
| **Metric** | Conversion rate, revenue per user |
| **Risk** | Revenue impact if wrong price wins |

**Example:** Test $29/mo vs $39/mo for Pro plan with new signups.

### Feature Experiments

Test new features with subset of users.

| Aspect | Consideration |
|--------|---------------|
| **Scope** | % of users or specific cohorts |
| **Duration** | Until feature validated |
| **Metric** | Engagement, retention, NPS |
| **Risk** | Poor UX for test group |

**Example:** Test new prompt builder UI with 10% of users.

### Conversion Experiments

Optimize upgrade prompts and paywalls.

| Aspect | Consideration |
|--------|---------------|
| **Scope** | Free/basic tier users |
| **Duration** | 2-4 weeks |
| **Metric** | Upgrade rate, trial-to-paid |
| **Risk** | User annoyance if too aggressive |

**Example:** Test "upgrade now" modal after 5 vs 10 free prompts.

### Model Experiments

Compare LLM backends for quality and cost.

| Aspect | Consideration |
|--------|---------------|
| **Scope** | Specific task types or user segments |
| **Duration** | Until quality parity validated |
| **Metric** | User satisfaction, task success, cost |
| **Risk** | Quality degradation for test group |

**Example:** Route 10% of basic tier to Mistral instead of Haiku.
<!-- /ANCHOR -->

---

## Experiment Architecture

<!-- ANCHOR: ops-experiment-architecture -->
### Assignment Flow (via Flagsmith)

```
User Request (with identity: journey_id or user_id)
      ↓
┌─────────────────────────────────────┐
│ 1. Call Flagsmith API               │
│    flagsmith.get_identity_flags()   │
└─────────────────────────────────────┘
      ↓
┌─────────────────────────────────────┐
│ 2. Get multivariate flag value      │
│    (Flagsmith handles bucketing)    │
│    → deterministic per identity     │
└─────────────────────────────────────┘
      ↓
Apply variant behavior
      ↓
┌─────────────────────────────────────┐
│ 3. On outcome, log to our table     │
│    INSERT INTO experiment_outcomes  │
│    (flagsmith_feature_name,         │
│     flagsmith_variant_value, ...)   │
└─────────────────────────────────────┘
```

### What Flagsmith Handles
- **Experiment definitions** — Create via Flagsmith UI/API
- **Multivariate flags** — Define variants with traffic weights
- **Identity-based bucketing** — Deterministic assignment via hash
- **Segments** — Target by user/journey attributes
- **Feature flags** — Gradual rollouts, kill switches

### What We Handle
- **Outcome tracking** — `rev.experiment_outcomes` table
- **Revenue attribution** — Link outcomes to Lago billing
- **Statistical analysis** — Custom queries on outcome data

### Key Principles

1. **Deterministic assignment** — Flagsmith uses identity hash for consistent bucketing
2. **Sticky assignment** — Same identity always gets same variant
3. **Local evaluation** — Flagsmith SDK caches flags locally (reduces API calls)
4. **Outcome ownership** — We track outcomes; Flagsmith doesn't do analytics
<!-- /ANCHOR -->

---

## Implementation Specifications

<!-- ANCHOR: integration-flagsmith -->
### Flagsmith Integration

Flagsmith (self-hosted) handles experiment definitions and variant assignment. See Phase 5 in the journey-auditor-guide-plan.md for deployment details.

**Flagsmith handles:**
- Experiment/feature flag definitions (via Flagsmith UI or API)
- Multivariate flags for A/B test variants
- Identity-based deterministic bucketing
- Segments for targeting
- Local evaluation mode (SDK caches flags)

**Flagsmith does NOT handle:**
- Outcome tracking / analytics
- Revenue attribution per variant
- Statistical significance calculations

**Conduit integration (Rust SDK):**
```rust
use flagsmith::Flagsmith;

// Initialize with local evaluation (caches flags)
let flagsmith = Flagsmith::new(config);

// Get variant for user/journey
let flags = flagsmith.get_identity_flags(&journey_id).await?;
let variant = flags.get_feature_value("checkout_flow_test")?;
// variant = "control" | "variant_a" | "variant_b"

// Apply variant behavior...

// On outcome, log to our table (see experiment_outcomes below)
```

**Flagsmith URLs (self-hosted):**
- **UI:** https://flagsmith.kube.dappleai.dev
- **API:** https://flagsmith-api.kube.dappleai.dev
<!-- /ANCHOR -->

---

<!-- ANCHOR: impl-experiment-outcomes-table -->
### Experiment Outcomes Table

**Issue #TODO-EXPERIMENT-OUTCOMES** — Create rev.experiment_outcomes table

Tracks outcomes for Flagsmith experiments. Since Flagsmith handles bucketing but not analytics, we track outcomes locally for statistical analysis and revenue attribution.

**Schema:**
```sql
CREATE TABLE rev.experiment_outcomes (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),

    -- Flagsmith reference (no FK - external system)
    flagsmith_feature_name TEXT NOT NULL,  -- e.g., 'checkout_flow_test'
    flagsmith_variant_value TEXT NOT NULL, -- e.g., 'control', 'variant_a'

    -- Identity (what was bucketed)
    identity_type TEXT NOT NULL,           -- 'journey_id', 'user_id', 'anonymous_id'
    identity_value TEXT NOT NULL,          -- The actual ID value

    -- Billable entity for revenue attribution
    billable_entity_id BIGINT REFERENCES rev.billable_entities(id),

    -- Outcome data
    outcome_type TEXT NOT NULL,            -- 'conversion', 'upgrade', 'revenue', 'engagement', 'churn'
    outcome_value NUMERIC,                 -- Numeric value (e.g., revenue amount, engagement score)
    outcome_metadata JSONB,                -- Additional context

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT valid_identity_type CHECK (identity_type IN ('journey_id', 'user_id', 'anonymous_id')),
    CONSTRAINT valid_outcome_type CHECK (outcome_type IN ('conversion', 'upgrade', 'downgrade', 'revenue', 'engagement', 'churn'))
);

CREATE UNIQUE INDEX idx_outcomes_object_id ON rev.experiment_outcomes(object_id);
CREATE INDEX idx_outcomes_feature ON rev.experiment_outcomes(flagsmith_feature_name);
CREATE INDEX idx_outcomes_feature_variant ON rev.experiment_outcomes(flagsmith_feature_name, flagsmith_variant_value);
CREATE INDEX idx_outcomes_identity ON rev.experiment_outcomes(identity_type, identity_value);
CREATE INDEX idx_outcomes_billable ON rev.experiment_outcomes(billable_entity_id);
CREATE INDEX idx_outcomes_type ON rev.experiment_outcomes(outcome_type);
CREATE INDEX idx_outcomes_created ON rev.experiment_outcomes(created_at);
```

**Outcome Types:**
| Outcome | Trigger | Value | Metadata |
|---------|---------|-------|----------|
| `conversion` | Trial → paid | 1 | `{plan_code}` |
| `upgrade` | Plan upgrade | price_diff_cents | `{from_plan, to_plan}` |
| `downgrade` | Plan downgrade | price_diff_cents (negative) | `{from_plan, to_plan}` |
| `revenue` | Invoice paid | amount_cents | `{lago_invoice_id}` |
| `engagement` | Key action | action_count | `{action_name}` |
| `churn` | Subscription ended | 1 | `{reason}` |

**Notes:**
- No FK to Flagsmith (external system) — reference by feature_name + variant_value
- `identity_type` + `identity_value` matches what was sent to Flagsmith
- `billable_entity_id` enables joining with Lago for revenue attribution
- Multiple outcomes per identity allowed (e.g., engagement + later conversion)
<!-- /ANCHOR -->

---

## Audit Queries

<!-- ANCHOR: ops-experiment-audit -->
### Active Experiments

> **Note:** Experiment definitions live in Flagsmith. Query Flagsmith API for active features:
> `GET /api/v1/environments/{environment_key}/featurestates/`

### Experiment Results (from our outcome data)

```sql
-- Conversion rates by variant
SELECT
    flagsmith_feature_name as experiment,
    flagsmith_variant_value as variant,
    COUNT(DISTINCT identity_value) as identities,
    COUNT(*) FILTER (WHERE outcome_type = 'conversion') as conversions,
    ROUND(
        100.0 * COUNT(*) FILTER (WHERE outcome_type = 'conversion')
        / NULLIF(COUNT(DISTINCT identity_value), 0),
        2
    ) as conversion_rate_pct
FROM rev.experiment_outcomes
WHERE created_at >= now() - interval '30 days'
GROUP BY flagsmith_feature_name, flagsmith_variant_value
ORDER BY flagsmith_feature_name, flagsmith_variant_value;
```

### Revenue per Variant

```sql
-- Revenue attribution by experiment variant
SELECT
    flagsmith_feature_name as experiment,
    flagsmith_variant_value as variant,
    COUNT(DISTINCT identity_value) as identities,
    COUNT(*) FILTER (WHERE outcome_type = 'revenue') as revenue_events,
    SUM(outcome_value) FILTER (WHERE outcome_type = 'revenue') as total_revenue_cents,
    ROUND(
        AVG(outcome_value) FILTER (WHERE outcome_type = 'revenue'),
        2
    ) as avg_revenue_cents
FROM rev.experiment_outcomes
GROUP BY flagsmith_feature_name, flagsmith_variant_value
ORDER BY flagsmith_feature_name, flagsmith_variant_value;
```

### Revenue with Lago Customer IDs

> **Note:** For detailed invoice data, query Lago API: `GET /invoices?external_customer_id={lago_customer_external_id}`

```sql
-- Get experiment outcomes with Lago customer IDs
SELECT
    o.flagsmith_feature_name as experiment,
    o.flagsmith_variant_value as variant,
    o.identity_value,
    be.lago_customer_external_id,
    o.outcome_type,
    o.outcome_value,
    o.created_at
FROM rev.experiment_outcomes o
JOIN rev.billable_entities be ON be.id = o.billable_entity_id
WHERE o.outcome_type IN ('conversion', 'revenue')
ORDER BY o.created_at DESC;
```

### Sample Ratio Mismatch (SRM)

> **Note:** SRM detection requires comparing expected weights (from Flagsmith) with actual distribution (from outcomes).

```sql
-- Check actual variant distribution
-- Compare with expected weights from Flagsmith feature configuration
SELECT
    flagsmith_feature_name as experiment,
    flagsmith_variant_value as variant,
    COUNT(DISTINCT identity_value) as identity_count,
    ROUND(
        100.0 * COUNT(DISTINCT identity_value)
        / SUM(COUNT(DISTINCT identity_value)) OVER (PARTITION BY flagsmith_feature_name),
        1
    ) as actual_pct
FROM rev.experiment_outcomes
GROUP BY flagsmith_feature_name, flagsmith_variant_value
ORDER BY flagsmith_feature_name, flagsmith_variant_value;

-- Compare actual_pct with expected_weight from Flagsmith API:
-- GET /api/v1/features/{feature_id}/ -> multivariate_options[].percentage_allocation
```

### Outcome Volume by Day

```sql
-- Daily outcome volume for monitoring
SELECT
    date_trunc('day', created_at) as day,
    flagsmith_feature_name as experiment,
    outcome_type,
    COUNT(*) as outcome_count
FROM rev.experiment_outcomes
WHERE created_at >= now() - interval '7 days'
GROUP BY 1, 2, 3
ORDER BY 1 DESC, 2, 3;
```
<!-- /ANCHOR -->

---

## Billing Integration

<!-- ANCHOR: ops-experiment-billing -->
### Pricing Experiment Guardrails

When running pricing experiments:

1. **New signups only** — Never change pricing for existing contracts
2. **Clear attribution** — Track which price the user saw at signup
3. **Honor shown price** — If user saw $29, charge $29 (even if experiment ends)
4. **Document in contract** — Subscription references the price variant

**Implementation:**

Subscription data lives in Lago. Store experiment context via Lago's metadata:

```
# When creating subscription in Lago, include experiment context in metadata:
POST /subscriptions
{
  "external_customer_id": "...",
  "plan_code": "...",
  "metadata": [
    { "key": "experiment_id", "value": "123" },
    { "key": "variant", "value": "treatment" },
    { "key": "shown_price_cents", "value": "3900" }
  ]
}
```

### Revenue Reporting

When reporting revenue during experiments:
- Segment by experiment variant
- Note that experiment affects comparability
- Consider excluding experiment cohort from main metrics

### Experiment Conclusion

When experiment concludes:
1. Analyze results (statistical significance)
2. Document winner and rationale
3. Update default pricing/feature config
4. Existing experiment subscribers keep their variant
5. New signups get winning variant
<!-- /ANCHOR -->

---

## Related Sections

- [Subscriptions](../domain/subscriptions.md) — Pricing and plan structure
- [Data Reconciliation](../monitoring/data-reconciliation.md) — Experiment data verification
- [Attribution Tracking](attribution-tracking.md) — Conversion attribution
- [Flagsmith Docs](https://docs.flagsmith.com) — Feature flags and experiments
- [Conduit Routing](conduit-routing.md) — Model routing (may use feature flags)
