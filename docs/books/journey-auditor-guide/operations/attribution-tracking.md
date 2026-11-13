# Attribution Tracking

Marketing channel attribution, conversion tracking, and revenue allocation.

---

## Overview

<!-- ANCHOR: ops-attribution -->
Attribution tracking answers:
- **Where did this customer come from?** (Channel attribution)
- **What touchpoints led to conversion?** (Funnel tracking)
- **How much revenue came from each channel?** (Revenue attribution)
- **Who deserves commission?** (Referral/partner attribution)

**Why this matters for RevOps:**
- Calculate customer acquisition cost (CAC) by channel
- Measure marketing ROI
- Track referral commissions
- Understand conversion funnels
<!-- /ANCHOR -->

---

## Attribution Models

<!-- ANCHOR: ops-attribution-models -->
### First-Touch Attribution

Credits the first interaction before conversion.

| Pros | Cons |
|------|------|
| Simple to implement | Ignores nurture touchpoints |
| Clear attribution | May over-credit top-of-funnel |

**Use when:** Understanding which channels drive awareness.

### Last-Touch Attribution

Credits the final interaction before conversion.

| Pros | Cons |
|------|------|
| Simple to implement | Ignores awareness touchpoints |
| Clear attribution | May over-credit bottom-of-funnel |

**Use when:** Understanding which channels close deals.

### Multi-Touch Attribution

Distributes credit across multiple touchpoints.

| Model | Distribution |
|-------|-------------|
| **Linear** | Equal credit to all touchpoints |
| **Time decay** | More credit to recent touchpoints |
| **Position-based** | 40% first, 40% last, 20% middle |

**Use when:** Understanding full customer journey.

### Recommendation

Start with **first-touch and last-touch** (easy to implement). Add multi-touch later if needed.
<!-- /ANCHOR -->

---

## Attribution Data Flow

<!-- ANCHOR: ops-attribution-flow -->
```
Marketing Campaign
      ↓
Landing Page (UTM params captured)
      ↓
┌─────────────────────────────────────┐
│ Anonymous Session                   │
│ - UTM parameters stored             │
│ - Referrer captured                 │
│ - Landing page recorded             │
└─────────────────────────────────────┘
      ↓
User Signs Up
      ↓
┌─────────────────────────────────────┐
│ Attribution Record Created          │
│ - First-touch attribution set       │
│ - Session data linked to user       │
└─────────────────────────────────────┘
      ↓
User Converts (Trial → Paid)
      ↓
┌─────────────────────────────────────┐
│ Conversion Recorded                 │
│ - Last-touch attribution set        │
│ - Revenue attributed                │
└─────────────────────────────────────┘
```

### UTM Parameters

Standard UTM params to capture:

| Parameter | Purpose | Example |
|-----------|---------|---------|
| `utm_source` | Traffic source | `google`, `linkedin`, `partner_acme` |
| `utm_medium` | Marketing medium | `cpc`, `email`, `referral` |
| `utm_campaign` | Campaign name | `q1_launch`, `black_friday` |
| `utm_content` | Ad/link variant | `hero_cta`, `sidebar_ad` |
| `utm_term` | Paid search keyword | `ai_assistant` |

### Referrer Data

In addition to UTM:
- HTTP referrer header
- Referral code (if partner/affiliate)
- Landing page URL
<!-- /ANCHOR -->

---

## Implementation Specifications

<!-- ANCHOR: impl-attribution-table -->
### User Attribution Table

**Issue #TODO-USER-ATTRIBUTION** — Create rev.user_attributions table

Stores attribution data for each user.

**Schema:**
```sql
CREATE TABLE rev.user_attributions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    user_id BIGINT NOT NULL,  -- References auth.users

    -- First-touch attribution (at signup)
    first_touch_source TEXT,
    first_touch_medium TEXT,
    first_touch_campaign TEXT,
    first_touch_content TEXT,
    first_touch_term TEXT,
    first_touch_referrer TEXT,
    first_touch_landing_page TEXT,
    first_touch_at TIMESTAMPTZ NOT NULL,

    -- Last-touch attribution (at conversion, updated on each conversion)
    last_touch_source TEXT,
    last_touch_medium TEXT,
    last_touch_campaign TEXT,
    last_touch_content TEXT,
    last_touch_term TEXT,
    last_touch_referrer TEXT,
    last_touch_at TIMESTAMPTZ,

    -- Referral attribution
    referral_code TEXT,
    referred_by_user_id BIGINT,  -- If user referred by another user

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_user_attr_user ON rev.user_attributions(user_id);
CREATE INDEX idx_user_attr_first_source ON rev.user_attributions(first_touch_source);
CREATE INDEX idx_user_attr_first_campaign ON rev.user_attributions(first_touch_campaign);
CREATE INDEX idx_user_attr_referral ON rev.user_attributions(referral_code) WHERE referral_code IS NOT NULL;
CREATE INDEX idx_user_attr_referred_by ON rev.user_attributions(referred_by_user_id) WHERE referred_by_user_id IS NOT NULL;
```

**Notes:**
- One row per user (unique on user_id)
- First-touch set at signup, never changes
- Last-touch updated on each conversion event
- Referral attribution for partner/affiliate tracking
<!-- /ANCHOR -->

---

<!-- ANCHOR: impl-touchpoints-table -->
### Touchpoint Log Table

**Issue #TODO-TOUCHPOINTS** — Create rev.user_touchpoints table

Logs all marketing touchpoints for multi-touch attribution.

**Schema:**
```sql
CREATE TABLE rev.user_touchpoints (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    user_id BIGINT NOT NULL,  -- References auth.users

    -- Attribution data
    source TEXT,
    medium TEXT,
    campaign TEXT,
    content TEXT,
    term TEXT,
    referrer TEXT,
    page_url TEXT,

    -- Context
    session_id UUID,  -- Groups touchpoints in same session
    touchpoint_type TEXT NOT NULL,  -- 'visit', 'signup', 'trial_start', 'conversion'

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_touchpoints_user ON rev.user_touchpoints(user_id);
CREATE INDEX idx_touchpoints_session ON rev.user_touchpoints(session_id);
CREATE INDEX idx_touchpoints_source ON rev.user_touchpoints(source);
CREATE INDEX idx_touchpoints_campaign ON rev.user_touchpoints(campaign);
CREATE INDEX idx_touchpoints_created ON rev.user_touchpoints(created_at);
```

**Touchpoint Types:**
| Type | When Recorded |
|------|---------------|
| `visit` | Any page visit with attribution params |
| `signup` | User registration |
| `trial_start` | Trial subscription started |
| `conversion` | Paid subscription started |
| `upgrade` | Plan upgrade |

**Notes:**
- Append-only log of all touchpoints
- Enables multi-touch attribution analysis
- Session ID groups touchpoints from same visit
<!-- /ANCHOR -->

---

---

## Referral Program (External Service)

> **Note:** We are NOT building custom referral tables. The referral/affiliate program will be handled by an external service (Brevo or alternatives TBD).

**What the external service handles:**
- Referral code generation and management
- Affiliate tracking and attribution
- Commission calculation and payout
- Referral program UI/dashboards

**What we store (minimal):**
- Reference to external referral ID on user_attributions (for linking)
- `referral_code` and `referred_by_user_id` fields capture the referral relationship

**Integration pattern:** Similar to Lago (billing) and Flagsmith (feature flags) — API integration with external service, we don't replicate their data locally.

---

## Audit Queries

<!-- ANCHOR: ops-attribution-audit -->
### Channel Performance

> **Note:** Invoice/revenue data lives in Lago. For revenue attribution:
> 1. Query attribution data from `rev.user_attributions`
> 2. Get Lago customer IDs via `rev.billable_entities`
> 3. Query Lago API for invoice data
> 4. Join in application code or analytics system

```sql
-- Get attribution data with Lago customer IDs
SELECT
    ua.first_touch_source as channel,
    ua.first_touch_medium as medium,
    ua.user_id,
    be.lago_customer_external_id
FROM rev.user_attributions ua
JOIN rev.user_billable_entities ube ON ube.user_id = ua.user_id
JOIN rev.billable_entities be ON be.id = ube.billable_entity_id
WHERE be.lago_customer_external_id IS NOT NULL;
-- Then query Lago API: GET /invoices?external_customer_id={id}&payment_status=succeeded
```

### Campaign Signups

```sql
-- Signups by campaign (revenue requires Lago API join)
SELECT
    ua.first_touch_campaign as campaign,
    COUNT(DISTINCT ua.user_id) as signups,
    COUNT(DISTINCT CASE WHEN be.lago_customer_external_id IS NOT NULL THEN ua.user_id END) as converted_to_customer
FROM rev.user_attributions ua
LEFT JOIN rev.user_billable_entities ube ON ube.user_id = ua.user_id
LEFT JOIN rev.billable_entities be ON be.id = ube.billable_entity_id
WHERE ua.first_touch_campaign IS NOT NULL
GROUP BY ua.first_touch_campaign
ORDER BY signups DESC;
```

### Referral Tracking

> **Note:** Referral program data lives in external service (Brevo or alternatives).
> Query the external service API for referral performance and commission data.
>
> We can correlate referrals with our data via `user_attributions.referral_code`:

```sql
-- Users who came through referral codes
SELECT
    ua.referral_code,
    COUNT(DISTINCT ua.user_id) as referred_users,
    COUNT(DISTINCT be.id) as converted_to_customer
FROM rev.user_attributions ua
LEFT JOIN rev.user_billable_entities ube ON ube.user_id = ua.user_id
LEFT JOIN rev.billable_entities be ON be.id = ube.billable_entity_id
WHERE ua.referral_code IS NOT NULL
GROUP BY ua.referral_code
ORDER BY referred_users DESC;
```

### Missing Attribution

```sql
-- Users without attribution data (data quality check)
SELECT
    u.id as user_id,
    u.created_at as signup_date
FROM auth.users u
LEFT JOIN rev.user_attributions ua ON ua.user_id = u.id
WHERE ua.id IS NULL
  AND u.created_at > now() - interval '90 days';
```
<!-- /ANCHOR -->

---

## Related Sections

- [A/B Testing](ab-testing.md) — Experiment attribution via Flagsmith
- [Data Reconciliation](../monitoring/data-reconciliation.md) — Attribution data verification
- External: Brevo or alternatives for referral/commission tracking
