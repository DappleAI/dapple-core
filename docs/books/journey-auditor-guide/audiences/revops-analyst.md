# RevOps Analyst

Revenue Operations analysts focused on self-service reporting, KPI dashboards, and ad-hoc queries.

---

## Role Definition

<!-- ANCHOR: audience-revops-analyst -->
RevOps Analysts bridge the gap between raw data and business insights. They need:
- Direct access to revenue and usage data
- Self-service query capabilities
- Dashboard building and maintenance
- Ad-hoc analysis for stakeholder requests

### Key Decisions
- Report and dashboard design
- Data interpretation for leadership
- Anomaly identification and investigation
- Process improvement recommendations
<!-- /ANCHOR -->

---

## Audit Questions

Questions RevOps Analysts typically need answered:

### Usage & Revenue
- What's the current MRR/ARR?
- How does usage trend over time by customer segment?
- What's the revenue per Playbook or feature?
- Where are usage patterns changing?

### Billing Accuracy
- Do invoice totals match expected subscription values?
- Are credits being applied correctly?
- What's the outstanding liability balance?
- Are there billing discrepancies to investigate?

### Operational Metrics
- What's the token utilization rate?
- How often do customers hit usage limits?
- What's the subscription renewal rate?
- Which customers are at risk based on usage patterns?

---

## Self-Service Capabilities

### Direct Query Access
RevOps Analysts should have read access to:
- `rev.*` schema tables
- Pre-built views for common queries
- Dashboard data sources

### Common Query Patterns
See [Audit Queries](../templates/audit-queries.md) for SQL patterns including:
- Usage summaries by period
- Revenue recognition calculations
- Customer lifecycle metrics

### Dashboard Building
Standard dashboards should cover:
- Revenue overview (MRR, ARR, growth)
- Usage trends and limits
- Billing health checks
- Customer segmentation

---

## Escalation Paths

### To Data Engineering
- Schema changes needed
- New data sources required
- ETL pipeline issues
- Performance problems with queries

### To Finance
- Revenue recognition questions
- Compliance requirements
- Month-end close support

### To Engineering
- Data quality issues at source
- Missing event correlation
- System integration problems

---

## Related Sections

- [Audit Queries](../templates/audit-queries.md) — SQL patterns for common analyses
- [Data Reconciliation](../monitoring/data-reconciliation.md) — Verification procedures
- [Schema Reference](../templates/schema-reference.md) — Table documentation
