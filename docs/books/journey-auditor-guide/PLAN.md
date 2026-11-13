# Journey Auditors Guide - Scaffolding Plan

## Status: WIP Planning Document

This guide is intentionally structured as a working document to:
1. Define audit requirements collaboratively with RevOps stakeholders
2. Serve as a template source for extraction into production guides
3. Document what Engineering monitors vs what RevOps self-serves

**Foundation:** Based on "SaaS Best Practices" legacy document covering billing infrastructure, GAAP accounting, and subscription management. The Auditors Guide applies an auditing lens to this domain.

---

## Audiences (In Scope)

| Role | Primary Needs |
|------|---------------|
| **Growth Team** | Journey visibility, conversion metrics, revenue attribution |
| **RevOps Analyst** | Self-service reporting, KPI dashboards, ad-hoc queries |
| **RevOps Engineer** | RevOps tool configuration (HubSpot, analytics platforms) |
| **Data Engineer** | ETL pipelines, data quality, schema documentation |
| **RevTech Specialist** | Third-party system configuration (Salesforce, CRM integrations) |
| **Finance/Accounting** | Legal compliance, month-end booking, revenue recognition |

**Out of Scope:** SRE (infrastructure/reliability - handled separately)

---

## Proposed Directory Structure

```
docs/books/journey-auditor-guide/
├── book.toml                    # Minimal config, marked as draft
├── SUMMARY.md                   # TOC with status markers
├── AGENTS.md                    # Agent reading guide
├── preface.md                   # WIP disclaimer + purpose
├── glossary.md                  # Ability, Token, Slot, Memo definitions
│
├── audiences/
│   ├── growth-team.md           # Marketing + Sales
│   ├── revops-analyst.md        # Role definition + query needs
│   ├── revops-engineer.md       # RevOps tool config (HubSpot, analytics)
│   ├── data-engineer.md         # Pipeline + schema needs
│   ├── revtech-specialist.md    # Third-party systems (Salesforce, CRM)
│   └── finance-accounting.md    # Legal compliance, revenue recognition
│
├── domain/                      # Billing domain model (from SaaS Best Practices)
│   ├── usage-tracking.md        # Abilities, tokens, slots, liability
│   ├── debit-credit-memos.md    # Invoice lifecycle, credit application
│   ├── revenue-recognition.md   # GAAP daily calculation, month-end close
│   ├── subscriptions.md         # Demo/basic/paid states, renewals
│   └── pricing-discounts.md     # Price adjustments, bundling, margins
│
├── monitoring/                  # Core audit framework
│   ├── data-audits.md           # "Things that should never happen" queries
│   ├── data-reconciliation.md   # Internal vs external 1:1 comparisons
│   └── sandboxing.md            # Personal/QA environments with monitoring
│
├── integrations/                # External system audit points
│   ├── recurring-billing.md     # Maxio: events, tokens, memos
│   ├── revenue-system.md        # Zuora Revenue or equivalent
│   ├── accounting-system.md     # Debit/credit memo sync
│   ├── erp.md                   # Finance reporting, projections
│   ├── crm-cms.md               # HubSpot, Salesforce product sync
│   └── tax-compliance.md        # Avalara: per-zip rates
│
├── operations/                  # Operational audit concerns
│   ├── commission-tracking.md   # Sales commission, claw-backs
│   ├── journey-lifecycle.md     # Playbook → Journey → Dispatch states
│   ├── event-correlation.md     # Internal event ↔ external event_id
│   └── compliance.md            # Soft deletes, PII, retention
│
├── templates/                   # ANCHOR-marked extractable sections
│   ├── audit-queries.md         # Common SQL patterns for rev.* schema
│   ├── reconciliation-queries.md# Internal vs external comparison patterns
│   ├── schema-reference.md      # rev.* table documentation
│   ├── kpi-definitions.md       # Standard metrics and calculations
│   └── integration-points.md    # Where external tools connect
│
└── interview-notes/             # Legacy doc analysis
    └── .gitkeep
```

---

## Core Domain Model (from SaaS Best Practices)

### Billing Entities

| Entity | Definition | Audit Concern |
|--------|------------|---------------|
| **Ability** | Trackable user action (may be free or paid) | Usage must correlate with tokens |
| **One-Time Token** | Purchased usage unit with expiration date | Debit memo dates, liability tracking |
| **Subscription Token** | Renewed ability usage count per period | Liability limits, renewal tracking |
| **Subscription Slot** | Concurrent usage limit | Simultaneous liability cap |
| **Debit Memo** | Invoice with start/end dates | GAAP daily revenue recognition |
| **Credit Memo** | Credit applied to future invoices | Dates must match debit memo |
| **Event** | Internal log tied to external `event_id` | Audit chain to third-party systems |

### Audit Chain

```
User Action (Ability)
     ↓
Internal Event (info-level log) ←→ External event_id (Maxio)
     ↓                                    ↓
Token Consumption                   Third-party correlation
     ↓                                    ↓
Debit/Credit Memos                  Revenue Recognition
     ↓                                    ↓
GAAP Daily Calculation              Month-end close (5-day deadline)
```

### Key Audit Rules (from legacy doc)

**Data Audits** — "Things that should never happen":
- Credit memo start date after end date
- Credit memo starts before its debit memo
- Ability usage logged without corresponding token
- More tokens consumed than ability usages recorded

**Data Reconciliation** — Internal vs external comparisons:
- Tokens in Recurring Billing System ↔ documented usage
- Subscription state internally ↔ state in RBS
- Products in RBS/CMS ↔ abilities locally

### GAAP Revenue Recognition

Debit/credit memos are counted **daily** for the period:
- $300 debit memo over 30 days = $10/day
- $30 credit memo applied = net $9/day
- Month-end snapshot aggregates applicable daily values

---

## Known External Systems

| System | Category | Audit Relevance |
|--------|----------|-----------------|
| **Maxio** | Recurring Billing System | Tokens, memos, subscriptions, event correlation |
| **Lago** | Billing (internal) | Usage metering, event ingestion |
| **Zuora Revenue** | Revenue Recognition | GAAP daily calculations, month-end close |
| **Avalara** | Tax compliance | Per-zip sales tax, EU rates, Maxio integration |
| **HubSpot Content Hub** | CMS | Content analytics, journey touchpoints |
| **HubSpot Marketing Hub** | Marketing automation | Campaign attribution, conversion tracking |
| **HubSpot Sales Hub** | CRM | Lead/opportunity data, revenue attribution |
| **HubSpot Service Hub** | Support | Customer journey post-sale, churn signals |
| **Monday** | Project management | Workflow tracking, commit attribution |
| **ERP** (TBD) | Finance reporting | Projections, complex reports beyond RBS |

**Data Flow:** Internal events → Lago/Maxio (billing) → Zuora Revenue (GAAP) → Accounting/ERP

---

## Open Questions (For Content Population)

1. What specific audit queries does RevOps run today?
2. What data is missing that they wish they had?
3. What compliance requirements (SOC2, GDPR, etc.) drive audit needs?
4. How does Journey/Dispatch/LLM-call map to abilities and tokens?
5. What are the specific Maxio product configurations needed?
6. Which ERP is planned for Finance reporting?

---

## Next Steps

1. [x] User confirms structure
2. [ ] Create scaffolding files with stub content
3. [ ] Populate domain/ section from SaaS Best Practices content
4. [ ] Populate integrations/ section with system-specific audit points
5. [ ] Interview with RevOps stakeholders to fill gaps
6. [ ] Extract templates for inclusion in other guides
