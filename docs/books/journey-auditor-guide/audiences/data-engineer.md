# Data Engineer

Data Engineers focused on ETL pipelines, data quality, and schema documentation.

---

## Role Definition

<!-- ANCHOR: audience-data-engineer -->
Data Engineers build and maintain the data infrastructure that powers RevOps:
- Design and maintain ETL/ELT pipelines
- Ensure data quality and consistency
- Document schemas and data lineage
- Optimize query performance

### Key Decisions
- Pipeline architecture and scheduling
- Data modeling and schema design
- Quality gate implementation
- Performance optimization strategies
<!-- /ANCHOR -->

---

## Audit Questions

Questions Data Engineers typically need answered:

### Pipeline Health
- Are all pipelines running on schedule?
- What's the failure rate and recovery time?
- Are there data gaps or missing periods?
- What's the end-to-end latency?

### Data Quality
- What percentage of records pass validation?
- Where are schema violations occurring?
- Are there referential integrity issues?
- What's the duplicate detection rate?

### Schema Integrity
- Are all expected columns populated?
- What's the null rate for required fields?
- Are data types consistent?
- Are foreign key relationships intact?

---

## Schema Responsibilities

### Core Schemas

#### `rev` Schema
Primary audit tables:
- Abilities, tokens, slots
- Debit/credit memos
- Events with external correlation
- Subscription states

#### `web` Schema
User-facing data:
- Journeys, playbooks
- User sessions and actions
- Feature usage

#### `auth` Schema
Authentication data:
- User accounts
- API tokens
- Session management

---

## Pipeline Patterns

### Ingest Pipelines
- External system data (Maxio, HubSpot)
- Event stream processing
- Batch file imports

### Transform Pipelines
- Revenue recognition calculations
- Usage aggregations
- Attribution modeling

### Quality Pipelines
- Schema validation
- Referential integrity checks
- Anomaly detection

---

## Documentation Requirements

### Schema Documentation
- Table purpose and ownership
- Column definitions and types
- Relationship diagrams
- Sample queries

### Pipeline Documentation
- Data flow diagrams
- Transformation logic
- Schedule and dependencies
- Error handling

---

## Related Sections

- [Schema Reference](../templates/schema-reference.md) — Table documentation
- [Data Audits](../monitoring/data-audits.md) — Quality checks
- [Integration Points](../templates/integration-points.md) — External data sources
