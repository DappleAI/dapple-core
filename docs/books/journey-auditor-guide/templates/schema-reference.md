# Schema Reference

Documentation for `rev.*` audit tables.

---

## Coming Soon

Detailed column-level schema documentation is under development.

**What this section will cover:**
- Complete column definitions for all `rev.*` tables
- Data type specifications and constraints
- Index documentation and query optimization hints
- Foreign key relationships and join patterns
- Example queries for common access patterns

---

## Current Tables

### Resource Tracking

| Table | Purpose | Status |
|-------|---------|--------|
| `rev.resource_claims` | Claims made when resources consumed | Planned |
| `rev.dispatch_task_attempt_event_llm_usages` | LLM token consumption | Planned |
| `rev.dispatch_task_attempt_event_assets` | Generated assets | Planned |
| `rev.dispatch_task_attempt_event_service_calls` | Third-party API calls | Planned |
| `rev.resource_voids` | Invalid resource records | Planned |
| `rev.resource_amendments` | Corrected/backfilled data | Planned |

### Lago Integration

| Table | Purpose | Status |
|-------|---------|--------|
| `rev.lago_event_sync` | Tracks usage events sent to Lago | Planned |

### Organization

| Table | Purpose | Status |
|-------|---------|--------|
| `rev.billable_entities` | Who pays (includes `lago_customer_external_id`) | Planned |
| `rev.user_billable_entities` | Individual billing | Planned |
| `rev.company_billable_entities` | Company billing | Planned |
| `rev.companies` | Organizations | Planned |
| `rev.company_members` | User membership in companies | Planned |

### Existing Lookup Tables

| Table | Purpose |
|-------|---------|
| `rev.providers` | LLM provider endpoints (Anthropic, OpenAI, Mistral) |
| `rev.lineages` | Model prompting paradigms |
| `rev.model_releases` | Specific model configurations |
| `rev.asset_kinds` | Asset type classification |
| `rev.value_kinds` | Type system for value slots |

---

## Data in Lago (NOT Local)

The following data lives in Lago and should be queried via Lago API:

| Concept | Lago API |
|---------|----------|
| Subscriptions | `GET /subscriptions` |
| Invoices | `GET /invoices` |
| Credit Notes | `GET /credit_notes` |
| Plans/Products | `GET /plans` |
| Usage/Entitlements | `GET /customers/{id}/current_usage` |
| Wallets | `GET /wallets` |

**In the meantime:**
- For implementation specs, see the domain docs (e.g., [Organization Model](../domain/organization.md), [Usage Tracking](../domain/usage-tracking.md))
- For query patterns, see [Audit Queries](audit-queries.md)

---

## Related Sections

- [Audit Queries](audit-queries.md) — Query patterns
- [Data Engineer](../audiences/data-engineer.md) — Schema responsibility
- [Organization Model](../domain/organization.md) — Table designs
- [Usage Tracking](../domain/usage-tracking.md) — Resource table designs
