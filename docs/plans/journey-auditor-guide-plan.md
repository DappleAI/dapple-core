# Journey Auditor Guide — Implementation Plan

**Status:** In Progress
**Last Updated:** 2026-02-02
**Location:** `docs/guides/journey-auditor-guide/`

---

## Overview

The Journey Auditor Guide provides comprehensive documentation for RevOps, Finance, and Data teams to understand, audit, and reconcile billing data in the dapple-prompt system. It covers the full flow from user actions through Lago (recurring billing) to financial reporting.

### Goals

1. **3-Day Month-End Close** — Systems and controls that enable fast, accurate closes
2. **Auditable Data** — Every billing event traceable from user action to invoice
3. **Lago Integration** — Seamless two-way data flow with Lago (usage events out, invoices in)
4. **Self-Service Analytics** — RevOps can answer questions without engineering help

---

## Architecture Summary

### Data Flow

```
User Action (Journey/Dispatch)
        ↓
Platform Event (dispatch_task_attempt_events)
        ↓
Resource Capture (conduit creates claims → dispatch collects)
        ↓
Resource Tables (llm_usages, assets, service_calls)
        ↓
Lago Event Sync (send usage events to Lago)
        ↓
Lago Processing (aggregation, metering, invoicing)

--- Access Control (on each request) ---

Conduit Request
        ↓
Lago API Call (usage, entitlements, wallets)
        ↓
Access Decision (allow/deny/degrade)
```

### Key Entities

```
User (person with account)
    ├── company_members ──→ Company
    ├── department_members ──→ Department
    ├── team_members ──→ Team
    └── user_billable_entities ──→ BillableEntity → Lago Customer

Company
    ├── company_parents ──→ Company (parent)
    ├── providers (if type = 'provider')
    └── company_billable_entities ──→ BillableEntity → Lago Customer

BillableEntity (who pays)
    └── billable_entity_subscriptions → Lago Subscription
```

---

## Implementation Status

### Completed (Documentation)

| Section | File | Status |
|---------|------|--------|
| Guide structure | SUMMARY.md, AGENTS.md | ✓ Complete |
| Glossary | glossary.md | ✓ Draft |
| Operational Integrity | operations/integrity.md | ✓ Draft |
| Usage Tracking | domain/usage-tracking.md | ✓ Draft |
| Organization Model | domain/organization.md | ✓ Draft |
| Recurring Billing (Lago) | integrations/recurring-billing.md | ✓ Draft |
| Conduit Routing | operations/conduit-routing.md | ✓ Draft |
| A/B Testing | operations/ab-testing.md | ✓ Draft |
| Attribution Tracking | operations/attribution-tracking.md | ✓ Draft |
| Subscriptions | domain/subscriptions.md | ✓ Draft |

### Pending (Documentation)

| Section | File | Status | Notes |
|---------|------|--------|-------|
| Revenue Recognition | domain/revenue-recognition.md | Stub | Needs ASC 606 mapping |
| Pricing & Discounts | domain/pricing-discounts.md | Stub | Needs Lago plan structure |
| Commission Tracking | operations/commission-tracking.md | Stub | Needs commission model design |
| Compliance | operations/compliance.md | Stub | SOC 2, GDPR considerations |
| Sandboxing | monitoring/sandboxing.md | Stub | Lago sandbox environment |

### Pending (Implementation)

See AGENTS.md `impl-*` anchors for detailed specifications.

---

## Implementation Phases

### Phase 1: Organization Model (Foundation)

**Dependencies:** None
**Migration Strategy:** Individual migrations per table (easier to review, incremental rollout)
**Tables to create:** 12 tables (1 lookup + 11 domain)

| Table | Issue | Description | API |
|-------|-------|-------------|-----|
| `rev.company_member_roles` | #TODO-COMPANY-MEMBER-ROLES | Lookup: owner, admin, member | — |
| `rev.companies` | #TODO-COMPANIES | Organizations | CRUD |
| `rev.company_parents` | #TODO-COMPANY-PARENTS | Company hierarchy | via company |
| `rev.departments` | #TODO-DEPARTMENTS | Company departments | CRUD |
| `rev.teams` | #TODO-TEAMS | Company teams | CRUD |
| `rev.team_departments` | #TODO-TEAM-DEPARTMENTS | Team-department associations | via team |
| `rev.company_members` | #TODO-COMPANY-MEMBERS | User membership in companies | CRUD |
| `rev.department_members` | #TODO-DEPARTMENT-MEMBERS | User membership in departments | CRUD |
| `rev.team_members` | #TODO-TEAM-MEMBERS | User membership in teams | CRUD |
| `rev.billable_entities` | #TODO-BILLABLE-ENTITIES | Who pays (base table) | CRUD |
| `rev.user_billable_entities` | #TODO-USER-BILLABLE-ENTITIES | Individual billing | via BE |
| `rev.company_billable_entities` | #TODO-COMPANY-BILLABLE-ENTITIES | Company billing | via BE |

**Key design decisions:**
- User IS Person (no separate Person table)
- Billable entity uses class table inheritance (no nullable FKs)
- Flexible membership (user can be in company without team/department)
- Company parents in separate table (append-only pattern)
- **No company types** — `rev.providers` (model taxonomy) handles LLM providers
- **No TEXT+CHECK** — Use lookup tables for enum-like values
- **Reference `rev.user_details`** — Never touch `auth` schema directly
- **All Principal objects get CRUD APIs** — companies, departments, teams, billable_entities, membership tables

**Migration order:**
1. Lookup: `company_member_roles`
2. `companies` → `company_parents`
3. `departments`, `teams` → `team_departments`
4. `company_members`, `department_members`, `team_members`
5. `billable_entities` → `user_billable_entities`, `company_billable_entities`

### Phase 2: Resource Consumption Tables

**Dependencies:** Phase 1 (billable_entities), existing `rev.providers`, `rev.model_releases`, `rev.asset_kinds`
**Migration Strategy:** Individual migrations per table
**Tables to create:** 9 tables (2 lookups + 7 domain)

| Table | Issue | Description |
|-------|-------|-------------|
| `rev.resource_types` | #TODO-RESOURCE-TYPES | Lookup: llm_usage, asset, service_call |
| `rev.integration_services` | #TODO-INTEGRATION-SERVICES | Lookup: zapier, sendgrid, twilio |
| `rev.resource_claims` | #TODO-RESOURCE-CLAIMS | Claims made when resources consumed |
| `rev.dispatch_task_attempt_event_llm_usages` | #TODO-LLM-USAGE | LLM token consumption |
| `rev.dispatch_task_attempt_event_assets` | #TODO-ASSET-USAGE | Generated assets |
| `rev.dispatch_task_attempt_event_service_calls` | #TODO-SERVICE-CALLS | Third-party API calls |
| `rev.resource_voids` | #TODO-RESOURCE-VOIDS | Marks invalid records |
| `rev.resource_amendments` | #TODO-RESOURCE-AMENDMENTS | Corrected/backfilled data |
| `rev.lago_event_sync` | #TODO-LAGO-SYNC | Tracks what we've sent to Lago |

**Key design decisions:**
- Claims capture consumption immediately (in conduit)
- Collection creates resource records when dispatch completes
- Void + Amendment pattern for corrections (no updates)
- Both our counts and provider counts stored for reconciliation
- **No TEXT+CHECK** — Use lookup tables (`resource_types`, `integration_services`)
- **FK to existing taxonomy** — `llm_usages.provider_id` → `rev.providers`, `model_release_id` → `rev.model_releases`
- **Normal FK pattern** — `claim_id BIGINT` not `claim_object_id UUID`

**Migration order:**
1. Lookups: `resource_types`, `integration_services`
2. `resource_claims`
3. `llm_usages`, `asset_usages`, `service_calls` (reference claims)
4. `resource_voids`, `resource_amendments`
5. `lago_event_sync`

### Phase 3+4: Lago Integration (Combined)

**Status:** Design finalized — Direct API approach

**What we DON'T want (let Lago handle):**
- `rev.debit_memos` / `rev.debit_memo_items` — invoices live in Lago
- `rev.credit_memos` — credits live in Lago
- `rev.billable_entity_subscriptions` (full history) — subscription history lives in Lago

**What we ACTUALLY need:**
1. Know if user can perform an action (has abilities/quota)
2. Know if account is in good standing (not suspended)

---

#### Lago APIs Available

**1. Current Usage API** — "How much has this customer consumed?"
```
GET /customers/{external_customer_id}/current_usage?external_subscription_id=...
```
Returns: `amount_cents`, `total_amount_cents`, `charges_usage[]` with per-metric breakdown
Docs: https://getlago.com/docs/api-reference/customer-usage/get-current

**2. Entitlements API** — "Can this customer access feature X?"
```
GET /subscriptions/{external_id}/entitlements
```
Returns: feature gates (boolean), privileges (granular), allowances (included amounts), quotas (limits)
Triggers `subscription.updated` webhook on changes
Docs: https://docs.getlago.com/guide/entitlements

**3. Wallets API** — "How many prepaid credits remain?"
```
GET /wallets?external_customer_id=...
```
Returns: `credits_balance`, `ongoing_usage_balance_cents`, `consumed_credits`
Docs: https://getlago.com/docs/api-reference/wallets/get-all

---

#### Architecture Decision: Direct API

**Decision:** Call Lago API directly from conduit on each request. Defer caching until we verify latency is actually an issue.

| Option | Latency | Staleness | Complexity | Status |
|--------|---------|-----------|------------|--------|
| **Direct API** | +50-200ms | None | Low | ✅ Chosen |
| Cache + Webhooks | ~0ms | Seconds-minutes | Medium | Deferred |
| Periodic Sync | ~0ms | Bounded (N min) | Medium | Deferred |
| Kafka | ~0ms | Milliseconds | High | Deferred |

**Rationale:** Staleness is a serious concern (user consumes when suspended = we eat cost; user blocked when paid = bad UX). Direct API eliminates staleness entirely. We'll measure latency impact and add caching only if needed.

**Optimization: Claim-in-Chunks Pattern**

To reduce Lago API calls, conduit pre-authorizes usage in chunks (like credit card holds):

1. Check Lago: "Does customer have 10,000 tokens available?"
2. If yes, hold a local "claim" for 10,000 tokens
3. Request executes, uses 1,500 tokens
4. Claim is "collected" with actual usage (1,500)
5. Remaining 8,500 available for next requests without API call
6. When chunk depleted or expires (5-10 min TTL), check Lago again

This gives us: one Lago API call per N requests (where N ≈ chunk_size / avg_usage).

See `integration-lago-api-access` anchor in recurring-billing.md for implementation details.

**Fallback options if latency still an issue:**
- Kafka `events_enriched` topic for real-time state (requires enabling Kafka infra)
- Local cache with webhook refresh + periodic sync as backup
- Subscription Refresh Service only flags subscriptions, doesn't share usage data — not useful for external consumption

---

#### Phase 3: Outbound (TO Lago)

**Tables:**
- `rev.lago_event_sync` (already in Phase 2) — tracks what we've sent

**Components:**
| Component | Description |
|-----------|-------------|
| Lago Event Service | HTTP client to POST usage events to Lago |
| External ID patterns | `dapple_be_{uuid}` for customers, `dapple_sub_{uuid}` for subscriptions |

**Flow:**
```
Resource collected (llm_usage, asset, service_call)
    ↓
Insert lago_event_sync record (status: pending)
    ↓
Lago Event Service POSTs to Lago Events API
    ↓
Update lago_event_sync (status: synced, lago_transaction_id)
```

---

#### Phase 4: Inbound (FROM Lago)

**Approach:** Direct API calls, no local storage of Lago state

**Conduit pre-flight check:**
```rust
// Before processing request
let usage = lago_client.get_current_usage(customer_id, subscription_id).await?;
let entitlements = lago_client.get_entitlements(subscription_id).await?;

if !entitlements.has_feature("llm_access") {
    return Err(AccessDenied::FeatureNotEnabled);
}
if usage.total_amount_cents > entitlements.quota_cents("monthly_spend") {
    return Err(AccessDenied::QuotaExceeded);
}
```

**Webhook receiver (for notifications only, not state):**
- `subscription.terminated` → Log for analytics, maybe notify user
- `invoice.payment_failed` → Log for analytics
- No local state updates needed if we're always calling API

**recurring-billing.md cleanup needed:**
- Remove debit_memos, credit_memos, subscription history table specs
- Simplify to just outbound sync and API client docs

### Phase 5: Conduit Enhancements + Flagsmith ✅

**Status:** Design complete, ready for implementation
**Dependencies:** Phase 3+4 (Lago API client)
**Tables to create:** None

**Existing Infrastructure:**
- `InferenceProfiles` already handle model selection per Journey
- `model_release.tier` already defines model tiers (haiku, sonnet, opus)
- Conduit already picks from InferenceProfile options based on availability

**Components to build:**

| Component | Issue | Description |
|-----------|-------|-------------|
| Lago API Client | #TODO-LAGO-CLIENT | HTTP client for usage/entitlements/wallets APIs |
| Pre-flight Check | #TODO-PREFLIGHT | Call Lago before executing prompt |
| Flagsmith K8s Manifests | #TODO-FLAGSMITH-K8S | K8s deployment manifests in `assets/k8s/registry/flagsmith/` |
| Flagsmith CLI Module | #TODO-FLAGSMITH-CLI | `dapple flagsmith setup/status/logs` commands |
| Flagsmith Rust Client | #TODO-FLAGSMITH-CLIENT | Integration with `flagsmith` crate in conduit |

**Key design decisions:**
- Call Lago API directly (no local cache of subscription state)
- Deny if Lago unavailable (safe default)
- Claim-in-chunks pattern to reduce API calls (optional optimization, can defer)
- InferenceProfiles define WHAT models are available; Lago check determines IF user can use them
- Flagsmith handles feature flags and experiment bucketing; we handle outcome tracking

---

#### Flagsmith Evaluation (✅ Complete)

**Decision:** Adopt Flagsmith for feature flags and A/B test bucketing.

**Deployment:**
| Aspect | Finding |
|--------|---------|
| Architecture | Django API + PostgreSQL + optional task processor + Redis |
| Resources | ~1 vCPU, 2GB RAM per pod (similar to Lago) |
| Helm Charts | Available via external repo, supports HA |
| K8s Manifests | Will create at `assets/k8s/registry/flagsmith/` |

**CLI Integration:**
```
dapple flagsmith setup      # Deploy Flagsmith to cluster
dapple flagsmith status     # Show pod status
dapple flagsmith logs       # View service logs
```

**Conduit Integration (Rust SDK):**
```rust
// flagsmith crate supports local evaluation mode
let flags = flagsmith.get_identity_flags(&journey_id).await?;

// Feature rollouts
if flags.is_feature_enabled("use_opus_for_coding") {
    // Route to Opus
}

// A/B test bucketing
let variant = flags.get_feature_value("checkout_flow_test")?;
// variant = "control" | "variant_a" | "variant_b"
```

**Key capabilities:**
- **Feature flags** — Gradual rollouts, kill switches
- **Multivariate flags** — A/B test bucketing (replaces `rev.experiments`, `rev.experiment_assignments`)
- **Segments** — Target by user/journey attributes
- **Local evaluation** — Caches flags locally, reduces API calls
- **Offline mode** — Custom handlers when Flagsmith unavailable

**Flagsmith vs Lago (complementary):**
| Concern | Flagsmith | Lago |
|---------|-----------|------|
| "Can user access feature X?" | Feature flags | Entitlements via subscription |
| "Has user consumed quota?" | N/A | Wallet balance checks |
| Gradual rollouts | Percentage targeting | N/A |
| A/B test bucketing | Multivariate flags | N/A |
| Billing | N/A | Full billing stack |

**What Flagsmith does NOT do:**
- Analytics / outcome tracking — requires external platform (Amplitude, Mixpanel) or our own tables
- Revenue attribution per variant — we need to track this ourselves

---

#### Flagsmith Feature Design (Conduit Routing)

Features to configure in Flagsmith for conduit model routing:

**Core Features:**

| Feature | Type | Default | Description |
|---------|------|---------|-------------|
| `routing_strategy` | Multivariate | `cost_optimized` | How to score/select models |
| `allow_overage` | Boolean | `false` | Allow usage beyond quota |
| `opus_access_override` | Boolean | `false` | Override Lago entitlement for opus |
| `sonnet_access_override` | Boolean | `false` | Override Lago entitlement for sonnet |

**Routing Strategy Values:**

| Value | Scoring Weights | Use Case |
|-------|-----------------|----------|
| `cost_optimized` | quality=0.2, cost=0.5 | Free tier, cost-conscious |
| `quality_first` | quality=0.5, cost=0.2 | Premium plans |
| `quality_experiment` | Varies by variant | A/B testing quality impact |

**Experiment Features:**

| Feature | Type | Description |
|---------|------|-------------|
| `quality_experiment_variant` | Multivariate | `control` (cost) vs `treatment` (quality) |

**Segments to Create:**

| Segment | Rule | Used For |
|---------|------|----------|
| Pro Plan Users | `plan_tier = "pro"` | `routing_strategy` → `quality_first` |
| Enterprise Users | `plan_tier = "enterprise"` | `routing_strategy` → `quality_first`, `allow_overage` → true |
| Beta Testers | `beta_tester = true` | Model access overrides |
| Quality Experiment | `in_quality_experiment = true` | 50/50 split on `quality_experiment_variant` |

**Identity & Traits:**

- **Identity:** `billable_entity_id` (string)
- **Traits synced on request:**
  - `plan_tier` — From Lago subscription/entitlements
  - `beta_tester` — Manual flag or from user metadata
  - `in_quality_experiment` — Assigned by business logic

**Reference:** https://docs.flagsmith.com/deployment-self-hosting

### Phase 6: Growth & Analytics ✅

**Status:** Design complete, ready for implementation
**Dependencies:** Phase 1 (companies, users), Phase 5 (Flagsmith)

**Tables to create:**

| Table | Issue | Description |
|-------|-------|-------------|
| `rev.experiment_outcomes` | #TODO-EXPERIMENT-OUTCOMES | Outcome events for Flagsmith experiments |
| `rev.user_attributions` | #TODO-USER-ATTRIBUTION | First/last touch marketing attribution |
| `rev.user_touchpoints` | #TODO-TOUCHPOINTS | Multi-touch attribution log |

**NOT creating (external services or Flagsmith handle):**
- ~~`rev.experiments`~~ → Flagsmith feature flags with multivariate values
- ~~`rev.experiment_assignments`~~ → Flagsmith identity-based bucketing
- ~~`rev.referral_codes`~~ → External referral service (Brevo or alternatives)
- ~~`rev.referral_conversions`~~ → External referral service
- ~~`rev.path_recommendations`~~ → Flagsmith handles Journey assembly decisions directly

**Key design decisions:**
- Experiment definitions and bucketing live in Flagsmith (self-hosted)
- We track outcomes locally for statistical analysis and revenue attribution
- Outcome events reference Flagsmith `feature_name` + `variant_value`
- Multi-touch attribution with configurable windows
- **Journey assembly**: `POST /journeys { playbook_id }` — API calls Flagsmith to determine BriefPack and InferenceProfile, no custom scoring/recommendation system
- **Identity alignment**: dapple-ui, dapple-prompt-api, and dapple-prompt-conduit all use same Flagsmith identity (`billable_entity_{uuid}`) for consistent flag evaluation
- **Growth/Product self-service**: Can run experiments via Flagsmith UI without engineering for variant splits, segments, and mapping to existing UUIDs

---

#### Referral Program (External Service)

**Decision:** Use an external referral/affiliate platform rather than building our own.

**Candidate services:**
- Brevo (formerly Sendinblue) — Marketing platform with referral capabilities
- Other options TBD

**What the external service handles:**
- Referral code generation and management
- Affiliate tracking and attribution
- Commission calculation and payout
- Referral program UI/dashboards

**What we store (minimal):**
- Reference to external referral ID on billable_entity (for linking)
- No local referral_codes or referral_conversions tables

**Integration pattern:** Similar to Lago (billing) and Flagsmith (feature flags) — API integration with external service, we don't replicate their data locally.

**Status:** TBD — Needs further evaluation of service options before implementation.

---

#### Flagsmith Coordination Model (Growth, Product, Engineering)

How dapple-ui, dapple-prompt-api, and Growth/Product teams coordinate via Flagsmith.

**Key Principle:** Flagsmith is the single source of truth for feature flags and experiments. Both frontend (dapple-ui) and backend (dapple-prompt-api) read from the same Flagsmith environment using the same identity.

---

##### Identity Alignment

Both dapple-ui and dapple-prompt-api use the same Flagsmith identity:

```
Identity: billable_entity_{uuid}
Traits:
  - plan_tier: "pro"
  - beta_tester: true
  - signup_date: "2026-01-15"
```

This ensures a user gets the same flag values regardless of whether the flag is evaluated client-side or server-side.

---

##### dapple-ui Integration

**Setup:**
```javascript
// dapple-ui Flagsmith initialization
import flagsmith from 'flagsmith';

flagsmith.init({
  environmentID: 'xxx',
  api: 'https://flagsmith-api.kube.dappleai.dev/api/v1/',
  identity: `billable_entity_${billableEntityId}`,
  traits: {
    plan_tier: user.planTier,
    beta_tester: user.isBetaTester
  }
});

// Get flags
const briefPackVariant = flagsmith.getValue('journey_brief_pack');
const showNewOnboarding = flagsmith.hasFeature('new_onboarding_flow');
```

**UI-side flags:**
| Flag | Type | UI Usage |
|------|------|----------|
| `new_onboarding_flow` | Boolean | Show redesigned onboarding UI |
| `show_model_picker` | Boolean | Let user see/choose inference profile |
| `journey_brief_pack` | Multivariate | Which BriefPack UX to present |

---

##### dapple-prompt-api Integration

**Setup:** (Already documented in Phase 5)

```rust
// Same identity as UI
let flags = flagsmith.get_identity_flags_with_traits(
    &format!("billable_entity_{}", be_id),
    traits,
).await?;
```

**API-side flags:**
| Flag | Type | API Usage |
|------|------|----------|
| `journey_brief_pack` | Multivariate | Which BriefPack to assign |
| `journey_inference_profile` | Multivariate | Which InferenceProfile to use |
| `routing_strategy` | Multivariate | How to score models in conduit |

---

##### Experiment Types by Owner

| Experiment Type | Flagsmith Config | Code Changes | Owner |
|-----------------|------------------|--------------|-------|
| **UI-only** | Flag controls UI variant | dapple-ui only | Product |
| **Backend-only** | Flag controls routing/models | dapple-prompt-api only | Engineering |
| **Full-stack** | Flag coordinates both | Both repos | Product + Engineering |
| **No-code** | Flag controls existing variants | None | Growth |

**No-code experiments:** Growth can run experiments without engineering if the flag values map to existing BriefPack/InferenceProfile UUIDs.

---

##### Growth/Product Workflow

**1. Creating an Experiment (Flagsmith UI)**

```
1. Log into Flagsmith: https://flagsmith.kube.dappleai.dev
2. Create Feature:
   - Name: checkout_flow_v2
   - Type: Multivariate
   - Variants: control (50%), treatment (50%)
3. Create Segment (if targeting):
   - Name: pro_plan_users
   - Rule: plan_tier = "pro"
4. Enable for Environment: production
```

**2. Coordinating with Engineering**

| Scenario | Process |
|----------|---------|
| New flag, existing code paths | Growth creates flag, maps to existing UUIDs |
| New flag, needs new code | Growth creates ticket, Engineering adds flag handling |
| New segment | Growth creates in Flagsmith, no code needed |
| New traits | Engineering adds trait to identity calls |

**3. Monitoring Results**

- Flagsmith: See flag evaluations, segment membership
- dapple: Query `experiment_outcomes` for conversion/revenue data
- Combined: Join Flagsmith feature_name with our outcome data

---

##### Journey Creation Flow (End-to-End)

```
┌─────────────────────────────────────────────────────────────┐
│ 1. User lands in dapple-ui                                  │
│    → UI calls Flagsmith with identity                       │
│    → Gets: journey_brief_pack = "streamlined"               │
│    → UI shows streamlined onboarding experience             │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 2. User picks a Playbook, clicks "Start"                    │
│    → UI calls POST /journeys { playbook_id }                │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 3. dapple-prompt-api receives request                       │
│    → API calls Flagsmith with same identity                 │
│    → Gets: journey_brief_pack = "streamlined" (same!)       │
│    → Gets: journey_inference_profile = "quality_first"      │
│    → Assembles Journey with those components                │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 4. Journey created, user proceeds with debrief              │
│    → experiment_outcomes logged for analysis                │
└─────────────────────────────────────────────────────────────┘
```

---

##### Outcome Tracking

When Journey is created, log to `experiment_outcomes`:

```sql
INSERT INTO rev.experiment_outcomes (
    flagsmith_feature_name,
    flagsmith_variant_value,
    identity_type,
    identity_value,
    billable_entity_id,
    outcome_type,
    outcome_metadata
) VALUES (
    'journey_brief_pack',
    'streamlined',
    'billable_entity_id',
    'be_123',
    123,
    'journey_created',
    '{"playbook_id": "...", "journey_id": "..."}'
);
```

Later outcomes (completion, satisfaction) reference same identity for attribution.

---

##### Flags Reference (Journey Assembly)

| Flag | Type | Default | Controls |
|------|------|---------|----------|
| `journey_brief_pack` | Multivariate (UUID) | default pack | Which BriefPack assigned to Journey |
| `journey_inference_profile` | Multivariate (UUID) | default profile | Which InferenceProfile assigned |
| `force_brief_pack` | Boolean | false | If true, ignore pack selection logic |
| `force_inference_profile` | Boolean | false | If true, ignore profile selection logic |

Segments can override to force specific values for cohorts (beta testers, enterprise, experiments).

---

##### Full Stack Flag Flow (dapple-ui → prompt-api → conduit)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              DAPPLE-UI                                       │
│  Flagsmith JS SDK                                                           │
│  Identity: billable_entity_123                                              │
│                                                                             │
│  Flags evaluated:                                                           │
│  ├── new_onboarding_flow: true          → Show new UI                       │
│  ├── journey_brief_pack: "streamlined"  → Display streamlined UX            │
│  └── show_model_picker: false           → Hide model selection              │
│                                                                             │
│  User action: "Start Journey" → POST /journeys { playbook_id }              │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           DAPPLE-PROMPT-API                                  │
│  Flagsmith Rust SDK                                                         │
│  Identity: billable_entity_123 (same as UI)                                 │
│                                                                             │
│  On POST /journeys:                                                         │
│  1. Call Flagsmith → get flags for this identity                            │
│  2. Flags evaluated:                                                        │
│     ├── journey_brief_pack: "streamlined"    → Resolve to BriefPack UUID    │
│     └── journey_inference_profile: "quality" → Resolve to InferenceProfile  │
│  3. Assemble Journey = Playbook + BriefPack + InferenceProfile              │
│  4. Log to experiment_outcomes                                              │
│  5. Return Journey to UI                                                    │
│                                                                             │
│  On POST /dispatches (when user submits debrief):                           │
│  → Journey already has InferenceProfile attached                            │
│  → Dispatch created, tasks queued                                           │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          DAPPLE-PROMPT-CONDUIT                               │
│  Flagsmith Rust SDK                                                         │
│  Identity: billable_entity_123 (same identity, consistent flags)            │
│                                                                             │
│  On prompt execution:                                                       │
│  1. Get InferenceProfile from Journey (already assigned)                    │
│  2. Call Flagsmith → get runtime routing flags                              │
│  3. Flags evaluated:                                                        │
│     ├── routing_strategy: "quality_first"    → Score models for quality     │
│     ├── opus_access_override: false          → Use Lago entitlements        │
│     ├── allow_overage: false                 → Deny if over quota           │
│     ├── max_context_tokens: 32000            → Context limit                │
│     └── quality_experiment_variant: "treatment" → In A/B test               │
│  4. Call Lago → check entitlements, usage, quota                            │
│  5. Score available models (InferenceProfile × routing_strategy)            │
│  6. Execute prompt with selected model                                      │
│  7. Log resource claim, outcome                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

##### Flag Ownership by Service

| Service | Flag Category | Examples |
|---------|---------------|----------|
| **dapple-ui** | UI/UX experiments | `new_onboarding_flow`, `show_model_picker`, `checkout_cta_variant` |
| **dapple-prompt-api** | Journey assembly | `journey_brief_pack`, `journey_inference_profile` |
| **dapple-prompt-conduit** | Model routing | `routing_strategy`, `allow_overage`, `opus_access_override` |
| **Shared** | Identity traits | `plan_tier`, `beta_tester`, `signup_date` |

---

##### Flag Consistency Guarantee

Because all three services use the **same identity** (`billable_entity_{uuid}`) with the **same traits**, Flagsmith returns consistent flag values across the stack:

```
UI sees:        journey_brief_pack = "streamlined"
API sees:       journey_brief_pack = "streamlined"  ✓ Same
Conduit sees:   routing_strategy = "quality_first"  (different flag, same identity)
```

If Growth changes a flag in Flagsmith, all services pick up the change on next evaluation. No cross-service coordination needed for flag value changes.

---

##### When Engineering Involvement is Needed

| Change | Flagsmith Only | Needs Code |
|--------|----------------|------------|
| Change variant percentages (50/50 → 70/30) | ✓ | |
| Add new segment (enterprise_beta) | ✓ | |
| Map flag to existing BriefPack UUID | ✓ | |
| Add new BriefPack and use in experiment | | ✓ Create BriefPack first |
| Add new trait (e.g., `company_size`) | | ✓ Add to identity calls |
| New flag category (e.g., `pricing_*`) | | ✓ Add flag handling |

---

##### Experiment Lifecycle

```
┌─────────────────────────────────────────────────────────────┐
│ 1. PLAN (Growth/Product)                                    │
│    - Define hypothesis                                       │
│    - Identify flag type (UI/API/conduit/full-stack)         │
│    - Create ticket if code needed                           │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 2. CONFIGURE (Growth + Engineering if needed)               │
│    - Create feature flag in Flagsmith                       │
│    - Define segments/targeting                              │
│    - Set variant percentages                                │
│    - Engineering adds flag handling if new flag category    │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 3. LAUNCH                                                   │
│    - Enable flag in production environment                  │
│    - Users start getting bucketed                           │
│    - Outcomes logged to experiment_outcomes                 │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 4. ANALYZE                                                  │
│    - Query experiment_outcomes for results                  │
│    - Compare variants on key metrics                        │
│    - Statistical significance check                         │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 5. DECIDE                                                   │
│    - Winner → roll out to 100%                              │
│    - No winner → extend or kill                             │
│    - Update Flagsmith, archive feature flag                 │
└─────────────────────────────────────────────────────────────┘
```

**Experiment Outcomes Schema (conceptual):**
```sql
CREATE TABLE rev.experiment_outcomes (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),

    -- Flagsmith reference (no FK - external system)
    flagsmith_feature_name TEXT NOT NULL,
    flagsmith_variant_value TEXT NOT NULL,

    -- Who and when
    identity_id TEXT NOT NULL,  -- Flagsmith identity (journey_id, user_id, etc.)
    billable_entity_id BIGINT REFERENCES rev.billable_entities(id),

    -- Outcome data
    outcome_type TEXT NOT NULL,  -- 'conversion', 'revenue', 'engagement', etc.
    outcome_value NUMERIC,       -- e.g., revenue amount, engagement score
    outcome_metadata JSONB,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

**Analysis pattern:**
```sql
-- Revenue per variant
SELECT
    flagsmith_variant_value,
    COUNT(*) as conversions,
    SUM(outcome_value) as total_revenue,
    AVG(outcome_value) as avg_revenue
FROM rev.experiment_outcomes
WHERE flagsmith_feature_name = 'checkout_flow_test'
  AND outcome_type = 'revenue'
GROUP BY flagsmith_variant_value;
```

### Phase 7: Data Audits, Reconciliation & Experiment Analysis ✅

**Status:** Design complete

**Dependencies:** Phases 1-6 (table designs must be final first)

---

#### Quality Experiment Design

Design the `quality_experiment` A/B test that compares `cost_optimized` vs `quality_first` routing strategies.

**Research Questions:**
1. Does using higher-tier models (quality_first) produce measurably better outcomes?
2. What's the cost difference per outcome between strategies?
3. At what task complexity does quality_first become worth the cost?

**Outcomes to Track:**

| Outcome | Source | How to Measure |
|---------|--------|----------------|
| Task success | Conduit | Did task complete without error? |
| User satisfaction | App | Thumbs up/down, regenerate requests |
| Output quality | LLM-as-judge | Automated quality scoring |
| Cost per request | Lago | Token cost from usage events |
| Time to complete | Conduit | Request duration |

**Experiment Structure:**
```
Flagsmith Feature: quality_experiment_variant
Type: Multivariate
Segment: Users in quality experiment cohort

Variants:
  - control (50%): routing_strategy = cost_optimized
  - treatment (50%): routing_strategy = quality_first
```

**Analysis Queries (conceptual):**
```sql
-- Success rate by variant
SELECT
    flagsmith_variant_value,
    COUNT(*) FILTER (WHERE outcome_type = 'task_success' AND outcome_value = 1) as successes,
    COUNT(*) FILTER (WHERE outcome_type = 'task_success') as attempts,
    ROUND(100.0 *
        COUNT(*) FILTER (WHERE outcome_type = 'task_success' AND outcome_value = 1) /
        NULLIF(COUNT(*) FILTER (WHERE outcome_type = 'task_success'), 0),
    2) as success_rate_pct
FROM rev.experiment_outcomes
WHERE flagsmith_feature_name = 'quality_experiment_variant'
GROUP BY flagsmith_variant_value;

-- Cost per successful outcome
SELECT
    flagsmith_variant_value,
    SUM(outcome_value) FILTER (WHERE outcome_type = 'cost_cents') as total_cost,
    COUNT(*) FILTER (WHERE outcome_type = 'task_success' AND outcome_value = 1) as successes,
    ROUND(
        SUM(outcome_value) FILTER (WHERE outcome_type = 'cost_cents') /
        NULLIF(COUNT(*) FILTER (WHERE outcome_type = 'task_success' AND outcome_value = 1), 0),
    2) as cost_per_success_cents
FROM rev.experiment_outcomes
WHERE flagsmith_feature_name = 'quality_experiment_variant'
GROUP BY flagsmith_variant_value;
```

**Statistical Significance:**
- Minimum sample size: TBD (depends on expected effect size)
- Duration: Run until significance reached or 30 days max
- Primary metric: Cost per successful outcome

---

#### Data Audits & Reconciliation

**Problem:** `monitoring/data-audits.md` and `monitoring/data-reconciliation.md` were updated prematurely with queries referencing tables that may not exist (debit_memos, credit_memos, billable_entity_subscriptions). These files need cleanup once Phase 3+4 design is finalized.

**What will remain valid:**
- Orphan resource checks (llm_usages without parent event)
- Lago sync backlog (pending syncs in `lago_event_sync`)
- Claim/collection mismatch
- Provider token reconciliation (`our_token_count` vs `provider_token_count`)

**What needs rework:**
- Any queries referencing debit_memos, credit_memos
- Any queries referencing billable_entity_subscriptions (if we don't create it)
- Reconciliation procedures depend on what data we actually store

---

## Database Schema Summary

### New Tables by Schema

**rev schema:**

```
-- Organization (Phase 1)
rev.company_member_roles  -- lookup
rev.companies
rev.company_parents
rev.departments
rev.teams
rev.team_departments
rev.company_members
rev.department_members
rev.team_members
rev.billable_entities
rev.user_billable_entities
rev.company_billable_entities

-- Resource Tracking (Phase 2)
rev.resource_types  -- lookup
rev.integration_services  -- lookup
rev.resource_claims
rev.dispatch_task_attempt_event_llm_usages
rev.dispatch_task_attempt_event_assets
rev.dispatch_task_attempt_event_service_calls
rev.resource_voids
rev.resource_amendments
rev.lago_event_sync

-- Lago Integration (Phase 3+4) - DIRECT API
-- NOT CREATING: debit_memos, credit_memos, billable_entity_subscriptions
-- Conduit calls Lago API directly for usage/entitlements (no local cache)

-- Conduit + Flagsmith (Phase 5)
-- No new tables - uses Lago API and Flagsmith API directly
-- InferenceProfiles (existing) handle model selection
-- Flagsmith (self-hosted) handles feature flags and experiment bucketing

-- Growth (Phase 6)
-- NOT CREATING: experiments, experiment_assignments (Flagsmith handles)
rev.experiment_outcomes  -- outcome tracking for Flagsmith experiments
rev.user_attributions
rev.user_touchpoints
rev.referral_codes
rev.referral_conversions
```

### Key Relationships

```
auth.users ──→ rev.company_members ──→ rev.companies
                                            │
                                            ├──→ rev.departments ──→ rev.department_members
                                            ├──→ rev.teams ──→ rev.team_members
                                            ├──→ rev.company_parents
                                            └──→ rev.providers

auth.users ──→ rev.user_billable_entities ──→ rev.billable_entities
rev.companies ──→ rev.company_billable_entities ──→ rev.billable_entities
                                                         │
                                                         └──→ Lago (external_customer_id)

rev.dispatch_task_attempt_events
    ├──→ rev.dispatch_task_attempt_event_llm_usages ──→ rev.resource_claims
    ├──→ rev.dispatch_task_attempt_event_assets ──→ rev.resource_claims
    └──→ rev.dispatch_task_attempt_event_service_calls ──→ rev.resource_claims

rev.resource_* ──→ rev.lago_event_sync ──→ Lago Events API

Conduit ──→ Lago API (usage, entitlements, wallets) ──→ Access decision
```

---

## API Endpoints

### Webhook Receiver

```
POST /webhooks/lago
```
- Signature verification (HMAC-SHA256)
- Idempotency via `lago_id`
- Routes to handlers by `webhook_type`

### Internal APIs (to be designed)

```
POST /api/billable-entities
GET /api/billable-entities/{id}

POST /api/companies
GET /api/companies/{id}
GET /api/companies/{id}/members
POST /api/companies/{id}/members

GET /api/users/{id}/billable-entity
```

Note: Subscription/invoice data accessed via Lago API, not stored locally.

---

## Reconciliation Strategy

### Continuous Checks

| Check | Query | Expected | Frequency |
|-------|-------|----------|-----------|
| Orphan resources | Resources without parent event | 0 rows | Continuous |
| Lago sync backlog | Pending syncs > 1 hour | 0 rows | Hourly |
| Void without amendment | Voids not yet corrected | 0 rows | Daily |
| Claims without collection | Claims older than 1hr with no resource record | 0 rows | Hourly |

### Monthly Reconciliation

1. Export Lago events via API for the period
2. Compare against `lago_event_sync` records (what we sent vs what Lago received)
3. Investigate discrepancies (missing events, duplicates)
4. Document in audit log

### Provider Reconciliation

Compare our LLM token counts with provider invoices:
- Anthropic API usage report
- OpenAI usage dashboard
- Variance analysis (`our_token_count` vs `provider_token_count`)

---

## Configuration

### Environment Variables

```bash
# Lago (billing)
LAGO_API_URL=https://lago-api.kube.dappleai.dev
LAGO_API_KEY=<api-key>
LAGO_WEBHOOK_SECRET=<webhook-secret>
LAGO_ORGANIZATION_ID=<org-id>

# Lago feature flags
LAGO_SYNC_ENABLED=true
LAGO_WEBHOOKS_ENABLED=true

# Flagsmith (feature flags & experiments)
FLAGSMITH_API_URL=https://flagsmith-api.kube.dappleai.dev/api/v1
FLAGSMITH_SERVER_KEY=<server-side-environment-key>
FLAGSMITH_ENVIRONMENT_ID=<environment-id>
FLAGSMITH_LOCAL_EVAL=true  # Enable local evaluation mode
```

### Third-Party Services (Self-Hosted)

Both Lago and Flagsmith are self-hosted under the same local rules:
- Deployed to our K8s cluster
- Data stays within our infrastructure
- We control upgrades and availability

**Lago (Billing):**
- **UI:** https://lago.kube.dappleai.dev
- **API:** https://lago-api.kube.dappleai.dev
- **K8s manifests:** `assets/k8s/registry/lago/`
- **CLI:** `dapple lago setup`

**Flagsmith (Feature Flags):**
- **UI:** https://flagsmith.kube.dappleai.dev
- **API:** https://flagsmith-api.kube.dappleai.dev
- **K8s manifests:** `assets/k8s/registry/flagsmith/` (to create)
- **CLI:** `dapple flagsmith setup` (to create)

---

## Open Questions

1. **Revenue Recognition:** How to map Lago invoices to ASC 606 requirements?
2. **Commission Model:** Flat vs tiered vs hybrid commissions?
3. **Multi-currency:** Support for non-USD billing?
4. **Free Tier Tracking:** How to track free tier usage for upgrade funnel?
5. **Usage Alerts:** Real-time notifications when approaching limits?

---

## Migration Notes

### From Current State

- No existing Lago integration
- `dispatch_task_attempt_events` exists as parent table
- `asset_kinds` lookup table exists
- Auth via NextAuth.js (auth.users)

### Backward Compatibility

- New tables are additive (no breaking changes)
- Existing dispatches won't have resource records (acceptable)
- Backfill only if needed for specific date ranges

---

## References

### Guide Files

- `docs/guides/journey-auditor-guide/SUMMARY.md` — Table of contents
- `docs/guides/journey-auditor-guide/AGENTS.md` — Agent navigation guide (for implementing agents)
- `docs/guides/journey-auditor-guide/AGENT-AUTHORS.md` — Authoring guide (for agents adding content)
- `docs/guides/journey-auditor-guide/glossary.md` — Terminology
- `docs/guides/journey-auditor-guide/preface.md` — Purpose and scope

### Implementation Anchors

All `impl-*` anchors are indexed in AGENTS.md with:
- Location (file path)
- Description
- Issue reference

### Related Docs

- `CLAUDE.md` — Code patterns and conventions
- `docs/plans/dispatch-intent-binding-items-uuidv7-migration.md` — UUIDv7 migration patterns

---

## Authoring Workflow

When discovering new work items during implementation:

1. **Document in guide** — Add content to appropriate section with `<!-- ANCHOR: impl-* -->` markers
2. **Register anchor** — Add to AGENTS.md Implementation Anchor Index table
3. **Update SUMMARY.md** — Change `[STUB]` to `[DRAFT]` if adding real content
4. **Update this plan** — Add to relevant phase table if it's a new table/component

See `AGENT-AUTHORS.md` for detailed instructions on writing implementation specs.

### Spec Quality Bar

Every implementation spec MUST include:
- Issue reference (`#TODO-{NAME}`)
- Full DDL with constraints and indexes
- Dependencies on other features
- Acceptance criteria (checkboxes)

This ensures implementing agents can work autonomously without guessing.
