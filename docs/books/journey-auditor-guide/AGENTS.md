# Agent Reading Guide — Journey Auditor Guide

> **WIP. Read this banner before reading anything else.**
>
> This book is **not a how-to**. It is the implementation index for the audit / billing / compliance domain. Most of what is described in its chapters is *not yet built*. Section markers (`[DRAFT]`, `[STUB]`) indicate maturity, and the giant `<!-- ANCHOR: impl-* -->` table further down is a backlog of features tied to issue numbers, not a description of running code. If you are looking for documentation of features you can use today, you are probably in the wrong book — check the table below.

## What this book is for

A coordinated working surface for agents and humans implementing audit and billing infrastructure. Use it when you need to:
- locate the design spec for an unimplemented audit/billing table
- understand which datastore (our `rev` schema vs Lago vs Flagsmith) owns a piece of data
- follow ANCHOR markers to extract content into other guides

If you are not actively building one of those features, this book is probably not relevant to your task.

## What is actually built today

- **Lago billing has running code**, even though most chapters in this book describe pre-design state. Implementation lives in:
  - `crates/dapple-lago/` — Lago HTTP client library
  - `crates/dapple-prompt-lago-worker/` — Sidekiq-style background worker
  - `assets/k8s/registry/lago/` — Kubernetes manifests
  - `assets/nu/k8s/deploy_lago_billing.nu` — deploy script
  Treat the Lago anchors below as the authoritative *intent*, but the running pieces in code are the source of truth for what works right now.

## You probably want a different book if…

| Goal | Read instead |
|------|--------------|
| Authenticate, call API, ingest a Guide | [`../api-consumer-guide/AGENTS.md`](../api-consumer-guide/AGENTS.md) |
| Author a Guide directory | [`../journey-planner-guide/AGENTS.md`](../journey-planner-guide/AGENTS.md) |
| Extend the parser, data layer, conduit, or flow engine | [`../journey-builder-guide/AGENTS.md`](../journey-builder-guide/AGENTS.md) |
| Look up shared platform concepts and glossary | [`../_foundation/`](../_foundation/) |

## How to use this book if you *are* working on audit/billing

**For Implementing Agents:** read this file to understand the codebase context, then jump to specific `<!-- ANCHOR: impl-* -->` markers in the domain docs.

**For Authoring Agents:** see [`AGENT-AUTHORS.md`](AGENT-AUTHORS.md) for the conventions used here — adding work items, writing implementation specs, tracking issue references.

---

## Document Status

**This is a WIP (Work In Progress) guide.** Content is being developed collaboratively with RevOps stakeholders. Sections are marked with status indicators:

| Marker | Meaning |
|--------|---------|
| `[DRAFT]` | Content exists but is under review |
| `[STUB]` | Placeholder only, content pending |
| (no marker) | Considered stable |

## Reading Strategy

### For Context Gathering
Start with these files in order:
1. `preface.md` — Purpose and scope
2. `glossary.md` — Core terminology (Lago-aligned)
3. `operations/integrity.md` — Operational integrity principles
4. `domain/usage-tracking.md` — Foundation of billing model

### For Audit Query Tasks
1. `templates/audit-queries.md` — Common SQL patterns
2. `monitoring/data-audits.md` — "Things that should never happen"
3. `monitoring/data-reconciliation.md` — Internal vs external comparisons

### For Integration Understanding
1. `templates/integration-points.md` — Overview of external systems
2. Specific `integrations/*.md` files as needed

## ANCHOR Markers

This guide uses `<!-- ANCHOR: name -->` markers to identify extractable content for inclusion in other guides. When extracting content:

1. Find the ANCHOR start marker
2. Extract content up to (but not including) the next ANCHOR marker or section header
3. Preserve internal links (they assume guide context)

## Key Concepts for Agents

### Third-Party Services (Self-Hosted)

Both Lago and Flagsmith are self-hosted with the same local rules:
- **Lago** — Billing, subscriptions, usage metering, invoicing
- **Flagsmith** — Feature flags, multivariate flags for A/B test bucketing, remote config

### Billing Domain Model (Lago-Aligned Terminology)
- **Ability** = User action that may be free or billable
- **Billable Metric** = How usage events are aggregated (Lago term)
- **Usage Event** = Individual tracked action sent to Lago
- **Resource Cap** = Tier-based limit that constrains without depleting (slots, storage)
- **Debit Memo** = Invoice with service period dates (Lago: Invoice)
- **Credit Memo** = Credit applied to invoices (Lago: Credit Note)

### Feature Flags & Experiments (Flagsmith)
- **Feature Flag** = Boolean or multivariate value controlling functionality
- **Multivariate Flag** = Flag with multiple values for A/B test bucketing
- **Identity** = User/journey ID for consistent variant assignment
- **Segment** = Target group based on attributes

### Audit Chain
```
Platform Action → Internal Event → Resource Consumption → Lago Usage Event → Invoice
                       ↓                    ↓
         dispatch_task_attempt_events    Child tables (llm_usages, assets, etc.)
                                              ↓
                                       lago_event_sync (what we told Lago)

Experiment Flow:
Flagsmith (variant assignment) → experiment_outcomes (we track outcomes) → Revenue attribution
```

### Schema Focus
Primary tables are in the `rev` schema:

**Existing:**
- `rev.dispatch_task_attempt_events` — Event log for dispatch execution (parent for resource tables)
- `rev.asset_kinds` — Lookup table for asset types
- `rev.providers` — LLM provider endpoints (Anthropic, OpenAI, Mistral)
- `rev.lineages` — Model prompting paradigms
- `rev.model_releases` — Specific model configurations

**To Be Implemented (see impl-* anchors):**

*Organization:*
- `rev.companies` — Organizations
- `rev.company_parents` — Company hierarchy
- `rev.departments` — Company departments
- `rev.teams` — Company teams
- `rev.team_departments` — Team-department associations
- `rev.company_members` — User membership in companies
- `rev.department_members` — User membership in departments
- `rev.team_members` — User membership in teams
- `rev.billable_entities` — Who pays (base table, includes `lago_customer_external_id`)
- `rev.user_billable_entities` — Individual billing
- `rev.company_billable_entities` — Company billing
- `rev.experiment_outcomes` — Outcome events for Flagsmith experiments (we track outcomes, Flagsmith handles bucketing)
- `rev.user_attributions` — User marketing attribution
- `rev.user_touchpoints` — Multi-touch attribution log
- `rev.referral_codes` — Referral/affiliate codes
- `rev.referral_conversions` — Referral conversion tracking

*Resource Tracking:*
- `rev.resource_claims` — Claims made when resources consumed (before collection)
- `rev.dispatch_task_attempt_event_llm_usages` — LLM token consumption (references claim)
- `rev.dispatch_task_attempt_event_assets` — Generated assets (references claim)
- `rev.dispatch_task_attempt_event_service_calls` — Third-party API calls (references claim)
- `rev.resource_voids` — Marks resource records as invalid
- `rev.resource_amendments` — Corrected/backfilled resource data
- `rev.lago_event_sync` — Tracks what we've sent to Lago

**NOT Creating (existing systems, Lago, or Flagsmith owns this data):**
- ~~`rev.tier_model_configs`~~ — InferenceProfiles already handle model selection
- ~~`rev.billable_entity_subscriptions`~~ — Query Lago API instead
- ~~`rev.debit_memos`~~ — Invoices live in Lago
- ~~`rev.debit_memo_items`~~ — Invoice line items live in Lago
- ~~`rev.credit_memos`~~ — Credit notes live in Lago
- ~~`rev.ability_balance_cache`~~ — Use Lago API directly (claim-in-chunks pattern reduces calls)
- ~~`rev.experiments`~~ — Flagsmith handles experiment/feature flag definitions
- ~~`rev.experiment_assignments`~~ — Flagsmith handles identity-based variant bucketing
- ~~`rev.referral_codes`~~ — External referral service (Brevo or alternatives)
- ~~`rev.referral_conversions`~~ — External referral service (Brevo or alternatives)

## Implementation Tasks

When implementing features from this guide:

1. Look for `<!-- ANCHOR: impl-{feature} -->` markers in domain docs
2. These contain:
   - Table schema designs (DDL)
   - Required fields and constraints
   - Lago mapping notes
   - Issue references

3. Cross-reference with:
   - `/CLAUDE.md` for code patterns and conventions
   - `crates/dapple-data/` for model examples
   - This guide for domain context

### Implementation Anchor Index

| Anchor | Location | Description | Issue |
|--------|----------|-------------|-------|
| `impl-resource-types-lookup` | domain/usage-tracking.md | Resource types lookup table | #TODO-RESOURCE-TYPES |
| `impl-integration-services-lookup` | domain/usage-tracking.md | Integration services lookup | #TODO-INTEGRATION-SERVICES |
| `impl-resource-capture-flow` | domain/usage-tracking.md | Where/how claims are created and collected | #TODO-RESOURCE-CAPTURE |
| `impl-resource-claims-table` | domain/usage-tracking.md | Resource claims (before collection) | #TODO-RESOURCE-CLAIMS |
| `impl-llm-usage-table` | domain/usage-tracking.md | LLM token consumption tracking | #TODO-LLM-USAGE |
| `impl-asset-usage-table` | domain/usage-tracking.md | Generated asset tracking | #TODO-ASSET-USAGE |
| `impl-service-call-table` | domain/usage-tracking.md | Third-party service call tracking | #TODO-SERVICE-CALLS |
| `impl-resource-voids-table` | domain/usage-tracking.md | Marks invalid resource records | #TODO-RESOURCE-VOIDS |
| `impl-resource-amendments-table` | domain/usage-tracking.md | Corrected/backfilled data | #TODO-RESOURCE-AMENDMENTS |
| `impl-lago-sync-table` | domain/usage-tracking.md | Lago event sync status | #TODO-LAGO-SYNC |
| `integration-lago-customer-mapping` | integrations/recurring-billing.md | Billable entity → Lago customer/subscription | — |
| `integration-lago-webhooks` | integrations/recurring-billing.md | Webhook receiver (notifications only) | — |
| `integration-lago-api-access` | integrations/recurring-billing.md | Lago API for access control (usage, entitlements, wallets) | — |
| `integration-lago-architecture` | integrations/recurring-billing.md | Integration architecture overview | — |
| `impl-company-member-roles-lookup` | domain/organization.md | Company member roles lookup | #TODO-COMPANY-MEMBER-ROLES |
| `impl-companies-table` | domain/organization.md | Companies table | #TODO-COMPANIES |
| `impl-company-parents-table` | domain/organization.md | Company parent relationships | #TODO-COMPANY-PARENTS |
| `impl-departments-table` | domain/organization.md | Departments table | #TODO-DEPARTMENTS |
| `impl-teams-table` | domain/organization.md | Teams table | #TODO-TEAMS |
| `impl-team-departments-table` | domain/organization.md | Team-department associations | #TODO-TEAM-DEPARTMENTS |
| `impl-company-members-table` | domain/organization.md | Company membership | #TODO-COMPANY-MEMBERS |
| `impl-department-members-table` | domain/organization.md | Department membership | #TODO-DEPARTMENT-MEMBERS |
| `impl-team-members-table` | domain/organization.md | Team membership | #TODO-TEAM-MEMBERS |
| `impl-billable-entities-table` | domain/organization.md | Billable entities base | #TODO-BILLABLE-ENTITIES |
| `impl-user-billable-entities-table` | domain/organization.md | User billable entities | #TODO-USER-BILLABLE-ENTITIES |
| `impl-company-billable-entities-table` | domain/organization.md | Company billable entities | #TODO-COMPANY-BILLABLE-ENTITIES |
| `integrity-philosophy` | operations/integrity.md | Why operational integrity matters | — |
| `integrity-principles` | operations/integrity.md | Detectable, Trackable, Owned, Decided, Verifiable | — |
| `integrity-control-patterns` | operations/integrity.md | Continuous and period-end checks | — |
| `integrity-corrections` | operations/integrity.md | Void + Amendment correction model | — |
| `integrity-lago-sync` | operations/integrity.md | Sync states and reconciliation | — |
| `ops-conduit-routing` | operations/conduit-routing.md | Conduit routing overview | — |
| `ops-tier-model-mapping` | operations/conduit-routing.md | Model selection via InferenceProfiles | — |
| `ops-routing-flow` | operations/conduit-routing.md | Routing decision flow (includes Lago pre-flight) | — |
| `ops-fallback-behavior` | operations/conduit-routing.md | Fallback and degradation | — |
| `ops-routing-strategies` | operations/conduit-routing.md | Flagsmith routing strategies | — |
| `conduit-flagsmith-features` | operations/conduit-routing.md | Feature flags conduit accepts from Flagsmith | — |
| `domain-ability-balance` | domain/subscriptions.md | Ability balance tracking (via Lago API) | — |
| `ops-ab-testing` | operations/ab-testing.md | A/B testing overview | — |
| `ops-experiment-types` | operations/ab-testing.md | Experiment type definitions | — |
| `ops-experiment-architecture` | operations/ab-testing.md | Assignment flow | — |
| `ops-experiment-audit` | operations/ab-testing.md | Experiment audit queries | — |
| `ops-experiment-billing` | operations/ab-testing.md | Billing integration | — |
| `impl-experiment-outcomes-table` | operations/ab-testing.md | Experiment outcomes (Flagsmith handles definitions/assignments) | #TODO-EXPERIMENT-OUTCOMES |
| `integration-flagsmith` | operations/ab-testing.md | Flagsmith integration for feature flags and experiment bucketing | — |
| `ops-attribution` | operations/attribution-tracking.md | Attribution overview | — |
| `ops-attribution-models` | operations/attribution-tracking.md | Attribution model types | — |
| `ops-attribution-flow` | operations/attribution-tracking.md | Data flow and UTM capture | — |
| `ops-attribution-audit` | operations/attribution-tracking.md | Attribution audit queries | — |
| `impl-attribution-table` | operations/attribution-tracking.md | User attribution table | #TODO-USER-ATTRIBUTION |
| `impl-touchpoints-table` | operations/attribution-tracking.md | Touchpoint log table | #TODO-TOUCHPOINTS |
| `flagsmith-coordination-model` | (plan file) | Growth/Product/Engineering coordination via Flagsmith | — |
| `flagsmith-full-stack-flow` | (plan file) | dapple-ui → prompt-api → conduit flag flow | — |
| `monitoring-data-audits` | monitoring/data-audits.md | Data audit overview | — |
| `audit-critical` | monitoring/data-audits.md | Critical audit queries (daily) | — |
| `audit-warning` | monitoring/data-audits.md | Warning audit queries (weekly) | — |
| `audit-informational` | monitoring/data-audits.md | Informational audit queries (monthly) | — |
| `impl-audit-infrastructure` | monitoring/data-audits.md | Audit execution infrastructure | #TODO-AUDIT-INFRASTRUCTURE |
| `monitoring-reconciliation` | monitoring/data-reconciliation.md | Data reconciliation overview | — |
| `reconciliation-lago-events` | monitoring/data-reconciliation.md | Lago event sync reconciliation | — |
| `reconciliation-providers` | monitoring/data-reconciliation.md | Provider token reconciliation | — |
| `impl-reconciliation-infrastructure` | monitoring/data-reconciliation.md | Reconciliation infrastructure | #TODO-RECONCILIATION-INFRASTRUCTURE |

### Implementation Order

Recommended build sequence:
1. Resource consumption tables (llm_usages, assets, service_calls) ✓ designed
2. Resource capture in conduit (populate the tables)
3. Company/org hierarchy model ✓ designed
4. Customer-Lago subscription mapping ✓ designed
5. Lago event sync and send logic ✓ designed
6. FROM Lago data flow (webhooks → subscription sync)
7. Data audits and reconciliation ✓ queries designed

---

## Out of Scope for This Guide

- Infrastructure/SRE concerns (see SRE documentation)
- Application development patterns (see CLAUDE.md)
- User-facing documentation (see product guides)

## Contributing

When adding content:
1. Use appropriate status markers in SUMMARY.md
2. Add ANCHOR markers for extractable sections
3. Cross-reference related sections
4. Include audit query examples where applicable
