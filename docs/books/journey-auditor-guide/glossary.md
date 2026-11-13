# Glossary

> **Status:** DRAFT — Terms are being refined with RevOps stakeholders

Core terminology for billing, revenue recognition, and auditing within the Dapple platform. This glossary bridges the **Billing Domain** (revenue tracking) with the **Platform Domain** (dapple-prompt internals).

---

> **Terminology Note:** Legacy docs used "token" as a generic metering term. This glossary aligns with **Lago terminology**:
> - "Token" (legacy) → **Billable Metric** (how usage is aggregated)
> - "LLM tokens" → Specifically third-party LLM input/output (a *type* of billable metric)
> - "Service calls" → External API usage like Zapier (another billable metric type)
> - "Caps/Limits" → **Resource Caps** (tier-based constraints, not consumed)

---

## Billing Entities

<!-- ANCHOR: glossary-ability -->
### Ability

A trackable user action that may be free or paid. Abilities represent the atomic unit of usage tracking. If there's any possibility an action will be charged for in the future, it should be tracked from the start.

**Audit Concern:** Every ability usage must correlate with appropriate token consumption (if paid) or be explicitly marked as free. Mismatches indicate billing errors.

**Examples:**
- `generate-resume` — User generates a resume document
- `llm-prompt-execution` — LLM processes a prompt (incurs third-party liability)
- `send-notification-email` — System sends email on user's behalf
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-usage-event -->
### Usage Event

An individual tracked action sent to Lago (the RBS). Usage events are the raw data that billable metrics aggregate.

| Field | Description |
|-------|-------------|
| `transaction_id` | Idempotency key (prevents duplicate billing) |
| `external_subscription_id` | Links to customer subscription |
| `code` | Billable metric code this event counts toward |
| `timestamp` | When the action occurred |
| `properties` | Additional data for aggregation (e.g., `tokens: 1500`) |

**Audit Concern:** Every internal ability usage should generate a corresponding Lago usage event.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-billable-metric -->
### Billable Metric

A definition of how usage events are aggregated and counted (Lago term). This is what we called "token" in legacy docs.

#### Metric Types by Usage Pattern

| Type | Examples | Aggregation | Liability |
|------|----------|-------------|-----------|
| **LLM Token Usage** | Claude/GPT input/output tokens | Sum of `tokens` property | Immediate (third-party cost) |
| **Service Calls** | Zapier triggers, SendGrid emails | Count of events | Immediate (third-party cost) |
| **Generated Assets** | PDFs, images, audio files | Count of events | Deferred until consumed |
| **Compute Time** | Processing minutes | Sum of `duration` property | Immediate |

#### Billing Models

| Model | Description | Lago Charge Type |
|-------|-------------|------------------|
| **Pay-as-you-go** | Charged per unit consumed | `standard`, `graduated` |
| **Package** | Prepaid bundles (e.g., 1000 LLM tokens) | `package` |
| **Percentage** | % of transaction value | `percentage` |

**Audit Concern:** Billable metric definitions must match ability tracking. Aggregation method affects revenue recognition timing.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-resource-cap -->
### Resource Cap

An ongoing limit based on subscription tier, not consumed by usage. Unlike billable metrics, caps don't deplete—they constrain.

| Cap Type | Examples | Enforcement |
|----------|----------|-------------|
| **Storage** | 10GB, 100GB, unlimited | Reject uploads when exceeded |
| **Concurrent Slots** | 3 active journeys, 10 team members | Queue or reject new requests |
| **Feature Gates** | Premium playbooks, advanced analytics | Hide or disable features |

**Audit Concern:** Cap enforcement should be logged. Upgrades should immediately expand caps.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-wallet -->
### Wallet (Prepaid Credits)

A prepaid balance customers can draw against for usage charges. Lago manages wallet balances automatically.

| Field | Description |
|-------|-------------|
| `balance` | Current remaining credits |
| `consumed_credits` | Total spent from this wallet |
| `expiration_date` | When unused credits expire |

**Audit Concern:** Wallet transactions must reconcile with usage events. Expiring credits require credit memos.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-slot -->
### Slot

A concurrent usage limit for subscription-based access. Slots are a type of **Resource Cap**—they constrain simultaneous usage rather than depleting like billable metrics.

**Example:** "5 simultaneous journey executions", "3 team members", "10 active projects"

**Audit Concern:** Slot limits cap simultaneous liability exposure. Peak usage should be tracked for capacity planning. Slot violations (attempts beyond limit) should be logged for upgrade funnel analysis.

**See also:** [Resource Cap](#resource-cap)
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-memo -->
### Debit Memo

An invoice document with start and end dates. The date range is critical for GAAP revenue recognition—revenue is calculated daily over the memo period.

**Example:** A $300 debit memo over 30 days = $10/day recognized revenue.

**Audit Concern:**
- Start date must be before or equal to end date
- Date range drives daily revenue calculation
- Must correlate with subscription or token purchase
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-credit-memo -->
### Credit Memo

A credit applied against future invoices, with its own start and end dates.

**Audit Concerns:**
- Start date must not be after end date
- Credit memo must not start before its associated debit memo
- Daily credit calculation reduces net recognized revenue
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-event -->
### Event

An internal log entry tied to an external `event_id` from third-party systems (e.g., Lago).

| Field | Description |
|-------|-------------|
| `internal_id` | Our system's event identifier |
| `external_id` | Third-party system's event identifier (Lago, Maxio) |
| `ability` | Which ability was used |
| `token_id` | Which token was consumed (if applicable) |
| `timestamp` | When the event occurred |

**Audit Concern:** Events provide the audit chain between internal actions and external billing systems. Every billable action should have a correlated external event. The `internal_id ↔ external_id` correlation is the foundation of billing auditability.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-subscription -->
### Subscription

A periodic contractual purchase of abilities that renews on a specific interval.

#### Subscription States

| State | Description | Audit Relevance |
|-------|-------------|-----------------|
| `demo` | Signed up, never entered payment | Track conversion funnel |
| `basic` | Lapsed from trial/paid, minimal free abilities | Resubscribe campaigns |
| `trial` | Time-limited full access before payment | Trial-to-paid conversion |
| `active` | Paid and current | Revenue recognition |
| `lapsed` | Failed to renew, in grace period | Churn risk, dunning |
| `canceled` | Explicitly terminated | Churn analysis |

**Audit Concern:** State transitions must be tracked. Demo/basic subscriptions should still renew (for token refresh) even if not billed—this maintains consistent analytics.
<!-- /ANCHOR -->

---

## Workflow Execution

<!-- ANCHOR: glossary-task -->
### Task

A single unit of work implemented as a small script. Tasks have clear inputs, outputs, and exit behavior.

| Field | Description |
|-------|-------------|
| `function` | The operation type (e.g., `nu_pipeline`, `llm_call`) |
| `function_input` | JSON configuration for the function |

**Audit Concern:** Task execution triggers ability usage events. LLM tasks incur third-party costs.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-step -->
### Step (Setup Step)

An ordered collection of Tasks that run together to accomplish a subgoal. Steps guarantee ordering and deterministic artifact passing.

**Audit Concern:** Step completion may mark ability consumption milestones.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-stage -->
### Stage

A higher-level grouping of Steps used to model major phases of a Playbook (e.g., data collection, formatting, review).

**Audit Concern:** Stage transitions may correlate with billing checkpoints.
<!-- /ANCHOR -->

---

## Journey Entities

<!-- ANCHOR: glossary-journey -->
### Journey

A user's active session with a workflow (Playbook). Journeys are the central runtime concept—all user activity flows through them.

**Audit Concern:** Journey state transitions (created → active → completed/abandoned) should correlate with ability usage and potential billing events.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-playbook -->
### Playbook

A workflow template that users select to create Journeys. Think of it as a "recipe" the user picks.

**Audit Concern:** Playbook selection may drive pricing tier or feature availability.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-dispatch -->
### Dispatch

Background execution of workflow steps within a Journey.

**Audit Concern:** Dispatch execution may consume tokens (especially for LLM calls). Each dispatch step should have traceable ability usage.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-debrief -->
### Debrief

The data-gathering interview phase of a Journey, where users provide input via Briefs.

**Audit Concern:** Debrief completion may trigger ability consumption or unlock paid features.
<!-- /ANCHOR -->

---

## Briefs & Packs

<!-- ANCHOR: glossary-brief -->
### Brief

A versioned container for debrief UI elements (BriefUnits). Briefs define the structure of data-gathering interviews.

| Field | Description |
|-------|-------------|
| `name` | Identifier for the brief |
| `title` | Display title |
| `path_object_id` | Stable UUID across versions |

**Audit Concern:** Brief versions should be tracked—changes to interview structure may affect data quality.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-brief-unit -->
### BriefUnit

An individual UI element within a Brief. Owned dependent of Brief.

| Type | Description |
|------|-------------|
| `output` | Displays computed values from ValueSlots |
| `markdown` | Renders formatted text content |
| `notice` | Shows alerts, warnings, or info messages |
| `input` | Captures user input (text, select, radio, etc.) |
| `media` | Displays images, video, or audio |

**Audit Concern:** Input units capture user-provided data—critical for proof of service.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-brief-group -->
### BriefGroup

A logical grouping of Briefs that must be completed atomically. Users cannot partially complete a BriefGroup.

**Audit Concern:** Atomic completion rule enforces data completeness for billing purposes.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-brief-pack -->
### BriefPack

A distributable package of BriefGroups. Assigned to Journeys at creation time.

**Audit Concern:** BriefPack assignment may be tied to subscription tier—premium packs for higher tiers.
<!-- /ANCHOR -->

---

## Revenue Recognition

<!-- ANCHOR: glossary-gaap -->
### GAAP Revenue Recognition

Generally Accepted Accounting Principles for when revenue can be recognized. For subscription and usage-based billing:

- Revenue is recognized **daily** over the service period
- Debit memos define the recognition period
- Credit memos reduce daily recognized revenue
- Month-end close aggregates applicable daily values

**5-Day Close Rule:** Month-end books typically close within 5 business days.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-liability -->
### Liability (Deferred Revenue)

Prepaid amounts not yet recognized as revenue. When a customer pays upfront:
1. Full amount is recorded as liability
2. Daily recognition reduces liability, increases revenue
3. At period end, liability should be zero (or refundable)

**Audit Concern:** Liability balances must reconcile between internal tracking and accounting systems.
<!-- /ANCHOR -->

---

## System Terms

<!-- ANCHOR: glossary-rbs -->
### Recurring Billing System (RBS)

The external system managing subscriptions, invoicing, usage metering, and payment collection. Currently: **Lago** (self-hosted, open-source).

**Important:** Lago is treated as third-party software even when self-hosted. We do not modify its source code—feature requests go through Lago's open source channels.

**Scalability:** The minimal Lago stack is appropriate for **up to ~50,000 customers**. Beyond this scale, consider adding Kafka/ClickHouse.

**Audit Concern:** Internal subscription state must reconcile with RBS state.
<!-- /ANCHOR -->

<!-- ANCHOR: glossary-reconciliation -->
### Reconciliation

The process of comparing internal data against external system data to identify discrepancies.

**Types:**
- **Token reconciliation:** Internal token counts vs. RBS records
- **Subscription reconciliation:** Internal state vs. RBS state
- **Product reconciliation:** Internal products vs. RBS/CMS products
- **Event reconciliation:** Internal events vs. external `event_id`
<!-- /ANCHOR -->

---

## Cross-Domain Mappings

<!-- ANCHOR: cross-domain-mappings -->
How platform concepts map to Lago billing concepts:

| Platform Event | Lago Concept | Billable Metric Type | Notes |
|----------------|--------------|---------------------|-------|
| Journey created | Usage Event | `journey_created` | May be free tier limited |
| Dispatch executed | Usage Event | `dispatch_executed` | Each step may trigger additional events |
| LLM prompt executed | Usage Event | `llm_tokens` (sum) | Properties include `input_tokens`, `output_tokens` |
| External API call | Usage Event | `service_calls` (count) | Zapier, SendGrid, etc. |
| Asset generated | Usage Event | `assets_generated` (count) | PDF, image, audio |
| Storage consumed | Resource Cap | N/A (plan limit) | Check against tier limit |
| Concurrent slots used | Resource Cap | N/A (plan limit) | Check against tier limit |
| BriefPack assigned | Feature Gate | N/A (plan feature) | Premium packs require higher tier |

**Liability Timing:**
- **Immediate:** LLM tokens, external API calls (third-party costs incurred at execution)
- **Deferred:** Generated assets (liability until service period ends)
- **None:** Resource caps (no per-unit liability, just enforcement)

**Event Flow:**
```
Platform Action → Internal Event Log → Lago Usage Event → Billable Metric Aggregation
                         ↓                    ↓
                   Audit trail         Invoice line item
```
<!-- /ANCHOR -->

---

## Acronyms

<!-- ANCHOR: acronyms -->
| Acronym | Meaning |
|---------|---------|
| **RBS** | Recurring Billing System (Lago, Maxio) |
| **RRS** | Revenue Recognition System (Zuora Revenue) |
| **CMS** | Customer Management System or Content Management System (context-dependent) |
| **CRM** | Customer Relationship Management (HubSpot, Salesforce) |
| **ERP** | Enterprise Resource Planner |
| **GAAP** | Generally Accepted Accounting Principles |
| **PII** | Personally Identifiable Information |
| **PAT** | Personal Access Token |
| **JWT** | JSON Web Token |
| **ACH** | Automated Clearing House (bank transfers) |
<!-- /ANCHOR -->

---

## See Also

- [Usage Tracking](domain/usage-tracking.md) — Deep dive into abilities and tokens
- [Debit & Credit Memos](domain/debit-credit-memos.md) — Invoice lifecycle
- [Revenue Recognition](domain/revenue-recognition.md) — GAAP daily calculations
