# AGENT-AUTHORS.md — Guide for Contributing Agents

This document provides instructions for AI agents that are **authoring or extending** the Journey Auditors Guide. If you're an agent implementing features described in this guide, see `AGENTS.md` instead.

---

## Purpose

The Journey Auditors Guide serves two audiences:
1. **Human readers** (RevOps, Finance, Data teams) — learning how the billing system works
2. **Implementing agents** — building the features described in the guide

As an authoring agent, your job is to ensure both audiences are well-served.

---

## Adding New Work Items

When you identify work that needs to be done (new tables, services, integrations), you MUST document it in the guide so future implementing agents can find and execute it.

### Step 1: Write the Domain Documentation

Add content to the appropriate section in the guide:
- `domain/` — Data models, table schemas, business logic
- `operations/` — Workflows, processes, operational concerns
- `integrations/` — External system connections
- `monitoring/` — Audits, reconciliation, observability
- `templates/` — Reusable queries, reference materials

### Step 2: Add Implementation Anchors

For any implementable feature, add an HTML anchor marker:

```markdown
<!-- ANCHOR: impl-{feature-name} -->

### {Feature Name}

**Issue:** #TODO-{FEATURE-NAME}
**Status:** Not Started | In Progress | Complete
**Dependencies:** List any prerequisite features

#### Description

[What this feature does and why it's needed]

#### Schema / DDL

```sql
CREATE TABLE rev.{table_name} (
    -- Full DDL specification
);
```

#### Implementation Notes

- [Key design decisions]
- [Edge cases to handle]
- [Testing requirements]

#### Acceptance Criteria

- [ ] Table created with all constraints
- [ ] Indexes for common query patterns
- [ ] Model and query functions in dapple-data
- [ ] API endpoints if user-facing
- [ ] Integration tests passing

<!-- /ANCHOR: impl-{feature-name} -->
```

### Step 3: Register in AGENTS.md

Add your anchor to the **Implementation Anchor Index** table in `AGENTS.md`:

```markdown
| `impl-{feature-name}` | {location}.md | {description} | #TODO-{FEATURE-NAME} |
```

### Step 4: Update SUMMARY.md Status

If you created a new file or changed a stub to a draft:
- `[STUB]` — Placeholder only, no real content
- `[DRAFT]` — Content exists but under review
- (no marker) — Considered stable

---

## Implementation Spec Format

When writing specs for implementing agents, be explicit and complete:

### Required Elements

1. **Issue Reference** — `#TODO-{NAME}` placeholder (will become GitHub issue)
2. **Dependencies** — What must exist first (other tables, services)
3. **Full DDL** — Complete CREATE TABLE with all constraints, indexes, comments
4. **Rust Types** — Expected model names (`{Table}Model`, `Flat{Table}Model`, etc.)
5. **Query Patterns** — What queries will be needed (insert, select_by_*, etc.)
6. **API Surface** — Endpoints if any, request/response schemas
7. **Acceptance Criteria** — Checkboxes an implementing agent can verify

### Example: Good Spec

```markdown
<!-- ANCHOR: impl-resource-claims-table -->

### Resource Claims Table

**Issue:** #TODO-RESOURCE-CLAIMS
**Status:** Not Started
**Dependencies:** None (foundational table)

#### Description

Resource claims capture the moment a resource is consumed, before collection.
Created by conduit during LLM calls, collected by dispatch when task completes.

#### Schema

```sql
CREATE TABLE rev.resource_claims (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),

    -- What was claimed
    resource_type TEXT NOT NULL CHECK (resource_type IN ('llm', 'asset', 'service_call')),

    -- Who claimed it
    billable_entity_id BIGINT NOT NULL REFERENCES rev.billable_entities(id),

    -- When
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    collected_at TIMESTAMPTZ,  -- NULL until collected

    -- Correlation
    dispatch_task_attempt_id BIGINT REFERENCES rev.dispatch_task_attempts(id),
    conduit_request_id UUID,  -- For tracing back to conduit logs

    CONSTRAINT resource_claims_object_id_unique UNIQUE (object_id)
);

CREATE INDEX idx_resource_claims_billable_entity ON rev.resource_claims(billable_entity_id);
CREATE INDEX idx_resource_claims_dispatch ON rev.resource_claims(dispatch_task_attempt_id);
CREATE INDEX idx_resource_claims_uncollected ON rev.resource_claims(claimed_at)
    WHERE collected_at IS NULL;
```

#### Implementation Notes

- Claims are created optimistically (may never be collected if request fails)
- Uncollected claims older than 24h should be investigated
- `object_id` becomes the Lago `transaction_id` for idempotency

#### Rust Types

- `ResourceClaimModel` — Full model with internal IDs
- `InsertResourceClaimArgs` — Column-wise insert args
- `InsertedResourceClaimModel` — INSERT...RETURNING result

#### Queries Needed

- `insert_resource_claims.sql` — Batch insert
- `select_uncollected_claims.sql` — For collection job
- `mark_claims_collected.sql` — Update collected_at

#### Acceptance Criteria

- [ ] Migration creates table with all constraints
- [ ] Model in `dapple-data/src/model/resource_claims.rs`
- [ ] Queries in `dapple-data/assets/sql/resource_claims/`
- [ ] Query functions in `dapple-data/src/query/resource_claims.rs`
- [ ] Unit tests for insert/select round-trip

<!-- /ANCHOR: impl-resource-claims-table -->
```

### Example: Bad Spec (Don't Do This)

```markdown
### Resource Claims

We need a table to track resource claims. It should have an ID and some fields
for tracking what was claimed. The conduit will create these.
```

Why it's bad:
- No anchor marker
- No issue reference
- No DDL
- No acceptance criteria
- Implementing agent has to guess everything

---

## Updating Existing Specs

When modifying an existing implementation spec:

1. **Preserve the anchor** — Don't change anchor names; other docs may reference them
2. **Update status** — Change from "Not Started" to "In Progress" or "Complete"
3. **Add notes** — Document any deviations from original spec
4. **Cross-reference** — If implementation revealed new requirements, add them

---

## Documentation vs Implementation

| Content Type | Where It Goes |
|--------------|---------------|
| Business context, "why" | Main documentation sections |
| Technical "how", DDL, code specs | Inside `<!-- ANCHOR: impl-* -->` blocks |
| Audit queries for humans | `templates/audit-queries.md` |
| Test queries for agents | Inside impl anchor's acceptance criteria |

---

## File Naming Conventions

- Documentation: `{topic}.md` (lowercase, hyphens)
- No separate files for implementation specs — they live as anchors within domain docs
- Plan files: `docs/plans/{feature}-plan.md` for major initiatives

---

## Coordination with Plan Files

The master plan lives at `docs/plans/journey-auditor-guide-plan.md`. When adding significant new work:

1. Add the impl anchor to the appropriate guide section
2. Register in AGENTS.md anchor index
3. Consider whether it affects the phase ordering in the plan file
4. Update the plan's table counts if adding new tables

---

## Quality Checklist

Before considering your authoring work complete:

- [ ] All new features have `impl-*` anchors
- [ ] All anchors registered in AGENTS.md index
- [ ] SUMMARY.md status markers updated
- [ ] DDL follows project conventions (see `/CLAUDE.md`):
  - `BIGINT GENERATED ALWAYS AS IDENTITY` for PKs
  - `uuidv7()` for object_ids
  - `TIMESTAMPTZ` for all timestamps
  - Explicit indexes on FKs
- [ ] No orphan references (all mentioned tables have specs)
- [ ] Acceptance criteria are verifiable by an implementing agent

---

## Getting Help

- **Code patterns:** See `/CLAUDE.md` in repo root
- **Existing examples:** `domain/organization.md` has comprehensive specs
- **Navigation:** `AGENTS.md` has the reading strategy and anchor index
