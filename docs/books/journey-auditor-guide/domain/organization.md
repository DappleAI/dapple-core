# Organization Model

Companies, users, teams, departments, and billable entities.

---

## Why This Matters

When Acme Corp signs up for Dapple, several things happen simultaneously:

1. **A company is created** — Acme Corp enters the system with a name and basic metadata
2. **The admin becomes a member** — Sarah, who signed up, is linked to Acme as an owner
3. **A billable entity is created** — This is *who pays*. It might be Acme Corp (company pays) or Sarah personally (individual pays)
4. **Lago gets notified** — The billable entity syncs to Lago as a customer, ready to receive usage events

Later, when Sarah invites her teammate Mike:
- Mike gets added as a company member (with "member" role)
- Mike joins Sarah's team (the "Marketing" team)
- Mike's usage still bills to the same billable entity (Acme Corp pays)

This separation—who uses vs. who pays—is the core concept. A user can be in multiple companies. A company can have multiple teams. But billing flows through the billable entity, which is either a user or a company.

---

## Overview

<!-- ANCHOR: domain-organization -->
The organization model supports:
- **Users** — People with accounts
- **Companies** — Organizations (businesses or providers)
- **Teams/Departments** — Groupings within companies
- **Billable Entities** — Who pays (user or company, maps to Lago)

**Key principle:** A user can work with multiple companies. Billing is separate from organizational membership.
<!-- /ANCHOR -->

---

## Entity Relationships

<!-- ANCHOR: domain-org-relationships -->
```
User ←──────────────────────────────────────────┐
  │                                              │
  ├── company_members ──→ Company                │
  │                          │                   │
  ├── department_members ──→ Department ←── company
  │                                              │
  ├── team_members ──→ Team ←── company          │
  │                      │                       │
  │                      └── team_departments ──→ Department
  │                                              │
  └── user_billable_entities ──→ BillableEntity ←┘
                                      ↑
Company ── company_billable_entities ─┘
  │
  ├── company_parents ──→ Company (parent)
  │
  └── providers (if company_type = 'provider')
```

**Membership is flexible:**
- User can be in company without team/department
- Team can exist without department
- User can be in multiple companies
<!-- /ANCHOR -->

---

## Audit Queries

These queries help verify organizational data integrity.

### Orphan Memberships

Users should only be in teams/departments that belong to companies they're members of.

```sql
-- Users in department but not in department's company
SELECT dm.user_id, d.company_id
FROM rev.department_members dm
JOIN rev.departments d ON d.id = dm.department_id
WHERE NOT EXISTS (
    SELECT 1 FROM rev.company_members cm
    WHERE cm.user_id = dm.user_id AND cm.company_id = d.company_id
);

-- Users in team but not in team's company
SELECT tm.user_id, t.company_id
FROM rev.team_members tm
JOIN rev.teams t ON t.id = tm.team_id
WHERE NOT EXISTS (
    SELECT 1 FROM rev.company_members cm
    WHERE cm.user_id = tm.user_id AND cm.company_id = t.company_id
);
```

### Billable Entity Integrity

Every billable entity should be linked to either a user or a company, never both, never neither.

```sql
-- Billable entities without user or company reference
SELECT be.id, be.object_id
FROM rev.billable_entities be
LEFT JOIN rev.user_billable_entities ube ON ube.billable_entity_id = be.id
LEFT JOIN rev.company_billable_entities cbe ON cbe.billable_entity_id = be.id
WHERE ube.id IS NULL AND cbe.id IS NULL;
```

### Company Hierarchy Cycles

Parent-child relationships should form a tree, not a cycle.

```sql
-- Detect cycles in company parent relationships (recursive CTE)
WITH RECURSIVE hierarchy AS (
    SELECT company_id, parent_company_id, ARRAY[company_id] AS path
    FROM rev.company_parents

    UNION ALL

    SELECT h.company_id, cp.parent_company_id, h.path || cp.company_id
    FROM hierarchy h
    JOIN rev.company_parents cp ON cp.company_id = h.parent_company_id
    WHERE NOT cp.company_id = ANY(h.path)
)
SELECT * FROM hierarchy WHERE parent_company_id = ANY(path);
```

---

## Related Sections

- [Usage Tracking](usage-tracking.md) — Resource consumption by billable entity
- [Lago Integration](../integrations/recurring-billing.md) — Billable entity → Lago customer mapping
- [Customer Mapping](../integrations/recurring-billing.md#customer-subscription-mapping) — How entities map to Lago

---

## Implementation Specifications

The following sections contain technical details for agents and engineers implementing these features. Click to expand.

<details>
<summary><strong>Migration Order</strong></summary>

Migrations should be created in this order (respecting FK dependencies):

1. Lookup table: `company_member_roles`
2. `companies`
3. `company_parents` (references `companies`)
4. `departments` (references `companies`)
5. `teams` (references `companies`)
6. `team_departments` (references `teams`, `departments`)
7. `company_members` (references `companies`, `user_details`, `company_member_roles`)
8. `department_members` (references `departments`, `user_details`)
9. `team_members` (references `teams`, `user_details`)
10. `billable_entities`
11. `user_billable_entities` (references `billable_entities`, `user_details`)
12. `company_billable_entities` (references `billable_entities`, `companies`)

**Note:** `rev.providers` already exists (model taxonomy) — no separate provider table needed here.

</details>

<details>
<summary><strong>Company Member Roles Lookup Table</strong></summary>

<!-- ANCHOR: impl-company-member-roles-lookup -->
**Issue #TODO-COMPANY-MEMBER-ROLES** — Create rev.company_member_roles lookup table
**Status:** Not Started
**Dependencies:** None (foundational lookup)

```sql
CREATE TABLE rev.company_member_roles (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

INSERT INTO rev.company_member_roles (name) VALUES
    ('owner'),
    ('admin'),
    ('member');
```

**Notes:**
- `owner` — Full control, billing access
- `admin` — Manage members, settings
- `member` — Basic access

**Acceptance Criteria:**
- [ ] Migration creates table and seeds values
- [ ] No model/query needed (joined via FK)
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>Companies Table</strong></summary>

<!-- ANCHOR: impl-companies-table -->
**Issue #TODO-COMPANIES** — Create rev.companies table
**Status:** Not Started
**Dependencies:** None
**API:** `POST/GET/PATCH /companies` — Principal object

```sql
CREATE TABLE rev.companies (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_companies_object_id ON rev.companies(object_id);
CREATE INDEX idx_companies_name ON rev.companies(name);
```

**Notes:**
- Simple company table — no type distinction needed
- LLM providers (Anthropic, OpenAI) are handled by existing `rev.providers` (model taxonomy)
- Individuals are billed via `user_billable_entities`, not as companies

**Rust Types:**
- `CompanyModel` — Full model
- `FlatCompanyModel` — For queries joining related tables
- `InsertCompanyArgs` — `{ names: Vec<String> }`
- `InsertedCompanyModel` — INSERT...RETURNING result

**Queries:**
- `insert_companies.sql` — Batch insert
- `select_companies_by_object_ids.sql` — By object_id array

**Acceptance Criteria:**
- [ ] Migration creates table
- [ ] Model in `dapple-data/src/model/companies.rs`
- [ ] Queries in `dapple-data/assets/sql/companies/`
- [ ] Query functions in `dapple-data/src/query/companies.rs`
- [ ] Handler in `dapple-prompt-api/src/handler/companies.rs`
- [ ] Routes registered
- [ ] Consumer test in `tests/consumer/companies.rs`
- [ ] Integration test passing
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>Company Parents Table</strong></summary>

<!-- ANCHOR: impl-company-parents-table -->
**Issue #TODO-COMPANY-PARENTS** — Create rev.company_parents table
**Status:** Not Started
**Dependencies:** `rev.companies`
**API:** Managed via `PATCH /companies/{id}` (set parent) — Associative Dependent

```sql
CREATE TABLE rev.company_parents (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    company_id BIGINT NOT NULL REFERENCES rev.companies(id),
    parent_company_id BIGINT NOT NULL REFERENCES rev.companies(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT no_self_parent CHECK (company_id != parent_company_id)
);

CREATE UNIQUE INDEX idx_company_parents_object_id ON rev.company_parents(object_id);
CREATE UNIQUE INDEX idx_company_parents_company ON rev.company_parents(company_id);
CREATE INDEX idx_company_parents_parent ON rev.company_parents(parent_company_id);
```

**Notes:**
- Supports enterprise → subsidiary relationships
- Unique on company_id (a company has at most one parent)
- Separate table allows append-only pattern for relationship changes
- To change parent: soft-delete old row (or void pattern), insert new

**Rust Types:**
- `CompanyParentModel` — Internal model
- `InsertCompanyParentArgs` — `{ company_ids, parent_company_ids }`

**Queries:**
- `insert_company_parents.sql` — Set parent relationship
- `select_company_parents_by_company_ids.sql` — Get parent for companies
- `select_company_children_by_parent_ids.sql` — Get children of parent

**Acceptance Criteria:**
- [ ] Migration creates table with constraints
- [ ] Model in `dapple-data/src/model/companies/company_parents.rs`
- [ ] Queries in `dapple-data/assets/sql/companies/company_parents/`
- [ ] Integrated into Company DTO (includes `parent_object_id: Option<Uuid>`)
- [ ] Cycle detection query works (see Audit Queries)
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>Departments Table</strong></summary>

<!-- ANCHOR: impl-departments-table -->
**Issue #TODO-DEPARTMENTS** — Create rev.departments table
**Status:** Not Started
**Dependencies:** `rev.companies`
**API:** `POST/GET/PATCH /departments` — Principal object

```sql
CREATE TABLE rev.departments (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    company_id BIGINT NOT NULL REFERENCES rev.companies(id),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_departments_object_id ON rev.departments(object_id);
CREATE INDEX idx_departments_company ON rev.departments(company_id);
CREATE UNIQUE INDEX idx_departments_company_name ON rev.departments(company_id, name);
```

**Notes:**
- Belongs to exactly one company
- Name unique within company

**Rust Types:**
- `DepartmentModel` — Full model with company details
- `InsertDepartmentArgs` — `{ company_ids, names }`
- `InsertedDepartmentModel` — INSERT...RETURNING result

**Queries:**
- `insert_departments.sql` — Batch insert
- `select_departments_by_object_ids.sql` — By object_id array
- `select_departments_by_company.sql` — All departments in a company

**Acceptance Criteria:**
- [ ] Migration creates table with unique constraint on (company_id, name)
- [ ] Model in `dapple-data/src/model/departments.rs`
- [ ] Queries in `dapple-data/assets/sql/departments/`
- [ ] Handler in `dapple-prompt-api/src/handler/departments.rs`
- [ ] Integration test passing
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>Teams Table</strong></summary>

<!-- ANCHOR: impl-teams-table -->
**Issue #TODO-TEAMS** — Create rev.teams table
**Status:** Not Started
**Dependencies:** `rev.companies`
**API:** `POST/GET/PATCH /teams` — Principal object

```sql
CREATE TABLE rev.teams (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    company_id BIGINT NOT NULL REFERENCES rev.companies(id),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_teams_object_id ON rev.teams(object_id);
CREATE INDEX idx_teams_company ON rev.teams(company_id);
CREATE UNIQUE INDEX idx_teams_company_name ON rev.teams(company_id, name);
```

**Notes:**
- Belongs to exactly one company
- May or may not be in a department (see team_departments)
- Name unique within company

**Rust Types:**
- `TeamModel` — Full model with company details, optional department
- `InsertTeamArgs` — `{ company_ids, names }`
- `InsertedTeamModel` — INSERT...RETURNING result

**Queries:**
- `insert_teams.sql` — Batch insert
- `select_teams_by_object_ids.sql` — By object_id array
- `select_teams_by_company.sql` — All teams in a company
- `select_teams_by_department.sql` — Teams in a department

**Acceptance Criteria:**
- [ ] Migration creates table with unique constraint on (company_id, name)
- [ ] Model in `dapple-data/src/model/teams.rs`
- [ ] Queries in `dapple-data/assets/sql/teams/`
- [ ] Handler in `dapple-prompt-api/src/handler/teams.rs`
- [ ] Integration test passing
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>Team Departments Table</strong></summary>

<!-- ANCHOR: impl-team-departments-table -->
**Issue #TODO-TEAM-DEPARTMENTS** — Create rev.team_departments table
**Status:** Not Started
**Dependencies:** `rev.teams`, `rev.departments`
**API:** Managed via `PATCH /teams/{id}` (assign department) — Associative Dependent

```sql
CREATE TABLE rev.team_departments (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    team_id BIGINT NOT NULL REFERENCES rev.teams(id),
    department_id BIGINT NOT NULL REFERENCES rev.departments(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_team_departments_object_id ON rev.team_departments(object_id);
CREATE UNIQUE INDEX idx_team_departments_team ON rev.team_departments(team_id);
CREATE INDEX idx_team_departments_department ON rev.team_departments(department_id);
```

**Notes:**
- Associates team with department (optional)
- Unique on team_id (team is in at most one department)
- Team and department must belong to same company (enforce in app layer or trigger)

**Rust Types:**
- `TeamDepartmentModel` — Internal model
- `InsertTeamDepartmentArgs` — `{ team_ids, department_ids }`

**Queries:**
- `insert_team_departments.sql` — Assign team to department
- `delete_team_departments_by_team_ids.sql` — Remove department assignment

**Acceptance Criteria:**
- [ ] Migration creates table with unique constraint on team_id
- [ ] Model in `dapple-data/src/model/teams/team_departments.rs`
- [ ] Integrated into Team DTO (includes `department_object_id: Option<Uuid>`)
- [ ] Validation: team.company_id == department.company_id
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>Company Members Table</strong></summary>

<!-- ANCHOR: impl-company-members-table -->
**Issue #TODO-COMPANY-MEMBERS** — Create rev.company_members table
**Status:** Not Started
**Dependencies:** `rev.companies`, `rev.user_details`, `rev.company_member_roles`
**API:** `POST/GET/DELETE /company-members` — Membership management

```sql
CREATE TABLE rev.company_members (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    company_id BIGINT NOT NULL REFERENCES rev.companies(id),
    user_id BIGINT NOT NULL REFERENCES rev.user_details(user_id),
    role_id BIGINT NOT NULL REFERENCES rev.company_member_roles(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_company_members_object_id ON rev.company_members(object_id);
CREATE UNIQUE INDEX idx_company_members_company_user ON rev.company_members(company_id, user_id);
CREATE INDEX idx_company_members_user ON rev.company_members(user_id);
CREATE INDEX idx_company_members_role ON rev.company_members(role_id);
```

**Notes:**
- User can be member of multiple companies (no unique on user_id alone)
- `role_id` references lookup (1=owner, 2=admin, 3=member)
- Unique on (company_id, user_id) — one membership per company per user
- References `rev.user_details` — never touch `auth` schema directly

**Rust Types:**
- `CompanyMemberModel` — Full model with joined role name, user details
- `InsertCompanyMemberArgs` — `{ company_ids, user_ids, role_ids }`
- `InsertedCompanyMemberModel` — INSERT...RETURNING result

**Queries:**
- `insert_company_members.sql` — Add members to company
- `select_company_members_by_company.sql` — All members of a company
- `select_company_members_by_user.sql` — All companies user belongs to
- `delete_company_members.sql` — Remove membership (soft delete or hard)

**Acceptance Criteria:**
- [ ] Migration creates table with FKs to lookups and user_details
- [ ] Model in `dapple-data/src/model/company_members.rs`
- [ ] Queries in `dapple-data/assets/sql/company_members/`
- [ ] Handler in `dapple-prompt-api/src/handler/company_members.rs`
- [ ] Integration test: add member, list members, remove member
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>Department Members Table</strong></summary>

<!-- ANCHOR: impl-department-members-table -->
**Issue #TODO-DEPARTMENT-MEMBERS** — Create rev.department_members table
**Status:** Not Started
**Dependencies:** `rev.departments`, `rev.user_details`
**API:** `POST/GET/DELETE /department-members` — Membership management

```sql
CREATE TABLE rev.department_members (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    department_id BIGINT NOT NULL REFERENCES rev.departments(id),
    user_id BIGINT NOT NULL REFERENCES rev.user_details(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_department_members_object_id ON rev.department_members(object_id);
CREATE UNIQUE INDEX idx_department_members_dept_user ON rev.department_members(department_id, user_id);
CREATE INDEX idx_department_members_user ON rev.department_members(user_id);
```

**Notes:**
- User can be in multiple departments (across companies)
- Membership implies user is also in the department's company
- App layer should validate: user must be company_member before department_member
- References `rev.user_details` — never touch `auth` schema directly

**Rust Types:**
- `DepartmentMemberModel` — Full model with user details
- `InsertDepartmentMemberArgs` — `{ department_ids, user_ids }`

**Queries:**
- `insert_department_members.sql` — Add members to department
- `select_department_members_by_department.sql` — All members of a department
- `select_department_members_by_user.sql` — All departments user belongs to
- `delete_department_members.sql` — Remove membership

**Acceptance Criteria:**
- [ ] Migration creates table with FK to user_details
- [ ] Model in `dapple-data/src/model/department_members.rs`
- [ ] Queries in `dapple-data/assets/sql/department_members/`
- [ ] Handler validates user is company member first
- [ ] Integration test passing
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>Team Members Table</strong></summary>

<!-- ANCHOR: impl-team-members-table -->
**Issue #TODO-TEAM-MEMBERS** — Create rev.team_members table
**Status:** Not Started
**Dependencies:** `rev.teams`, `rev.user_details`
**API:** `POST/GET/DELETE /team-members` — Membership management

```sql
CREATE TABLE rev.team_members (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    team_id BIGINT NOT NULL REFERENCES rev.teams(id),
    user_id BIGINT NOT NULL REFERENCES rev.user_details(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_team_members_object_id ON rev.team_members(object_id);
CREATE UNIQUE INDEX idx_team_members_team_user ON rev.team_members(team_id, user_id);
CREATE INDEX idx_team_members_user ON rev.team_members(user_id);
```

**Notes:**
- User can be in multiple teams (across companies)
- Membership implies user is also in the team's company
- App layer should validate: user must be company_member before team_member
- References `rev.user_details` — never touch `auth` schema directly

**Rust Types:**
- `TeamMemberModel` — Full model with user details
- `InsertTeamMemberArgs` — `{ team_ids, user_ids }`

**Queries:**
- `insert_team_members.sql` — Add members to team
- `select_team_members_by_team.sql` — All members of a team
- `select_team_members_by_user.sql` — All teams user belongs to
- `delete_team_members.sql` — Remove membership

**Acceptance Criteria:**
- [ ] Migration creates table with FK to user_details
- [ ] Model in `dapple-data/src/model/team_members.rs`
- [ ] Queries in `dapple-data/assets/sql/team_members/`
- [ ] Handler validates user is company member first
- [ ] Integration test passing
<!-- /ANCHOR -->

</details>

<!-- NOTE: rev.providers already exists in model taxonomy (20260131114605_model_taxonomy.sql)
     It handles LLM provider API endpoints (anthropic, openai, mistral, self-hosted, stub).
     No separate provider table needed in the organization model. -->

<details>
<summary><strong>Billable Entities Table</strong></summary>

<!-- ANCHOR: impl-billable-entities-table -->
**Issue #TODO-BILLABLE-ENTITIES** — Create rev.billable_entities table
**Status:** Not Started
**Dependencies:** None (but extended by user/company_billable_entities)
**API:** `POST/GET /billable-entities` — Principal object (base table)

```sql
CREATE TABLE rev.billable_entities (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    lago_customer_external_id TEXT,
    lago_subscription_external_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_billable_entities_object_id ON rev.billable_entities(object_id);
CREATE UNIQUE INDEX idx_billable_entities_lago_customer ON rev.billable_entities(lago_customer_external_id)
    WHERE lago_customer_external_id IS NOT NULL;
CREATE INDEX idx_billable_entities_lago_sub ON rev.billable_entities(lago_subscription_external_id);
```

**Notes:**
- Base table for billing — extended by `user_billable_entities` or `company_billable_entities`
- Class table inheritance pattern (no nullable FKs on this table)
- Lago IDs nullable until subscription is created in Lago
- `lago_customer_external_id` format: `dapple_be_{object_id}`
- One billable entity can have multiple subscriptions over time

**Rust Types:**
- `BillableEntityModel` — Base model (may include extension type indicator)
- `InsertBillableEntityArgs` — `{ lago_customer_external_ids, lago_subscription_external_ids }`
- `InsertedBillableEntityModel` — INSERT...RETURNING result

**Queries:**
- `insert_billable_entities.sql` — Create base record
- `select_billable_entities_by_object_ids.sql` — By object_id array
- `select_billable_entity_by_lago_customer_id.sql` — Lookup by Lago external ID

**Acceptance Criteria:**
- [ ] Migration creates table with partial unique index on lago_customer_external_id
- [ ] Model in `dapple-data/src/model/billable_entities.rs`
- [ ] Queries in `dapple-data/assets/sql/billable_entities/`
- [ ] DTO includes `entity_type: "user" | "company"` derived from extension table
- [ ] Integration test: create user billable entity, create company billable entity
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>User Billable Entities Table</strong></summary>

<!-- ANCHOR: impl-user-billable-entities-table -->
**Issue #TODO-USER-BILLABLE-ENTITIES** — Create rev.user_billable_entities table
**Status:** Not Started
**Dependencies:** `rev.billable_entities`, `rev.user_details`
**API:** Created atomically with billable_entity — Owned Dependent (class table extension)

```sql
CREATE TABLE rev.user_billable_entities (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    billable_entity_id BIGINT NOT NULL REFERENCES rev.billable_entities(id),
    user_id BIGINT NOT NULL REFERENCES rev.user_details(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_user_billable_entities_object_id ON rev.user_billable_entities(object_id);
CREATE UNIQUE INDEX idx_user_billable_entities_billable ON rev.user_billable_entities(billable_entity_id);
CREATE UNIQUE INDEX idx_user_billable_entities_user ON rev.user_billable_entities(user_id);
```

**Notes:**
- Extends `billable_entities` for individual users (class table inheritance)
- One billable entity per user (unique on user_id)
- One user per billable entity (unique on billable_entity_id)
- References `rev.user_details` — never touch `auth` schema directly
- Created automatically when user signs up (or on first billing action)

**Rust Types:**
- `UserBillableEntityModel` — Full model with user details
- `InsertUserBillableEntityArgs` — `{ billable_entity_ids, user_ids }`

**Queries:**
- `insert_user_billable_entities.sql` — Create extension record
- `select_user_billable_entity_by_user.sql` — Get billable entity for user

**Acceptance Criteria:**
- [ ] Migration creates table with FKs
- [ ] Model in `dapple-data/src/model/billable_entities/user_billable_entities.rs`
- [ ] Single CTE insert creates both base + extension atomically
- [ ] Integration test: lookup user's billable entity
<!-- /ANCHOR -->

</details>

<details>
<summary><strong>Company Billable Entities Table</strong></summary>

<!-- ANCHOR: impl-company-billable-entities-table -->
**Issue #TODO-COMPANY-BILLABLE-ENTITIES** — Create rev.company_billable_entities table
**Status:** Not Started
**Dependencies:** `rev.billable_entities`, `rev.companies`
**API:** Created atomically with billable_entity — Owned Dependent (class table extension)

```sql
CREATE TABLE rev.company_billable_entities (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT uuidv7(),
    billable_entity_id BIGINT NOT NULL REFERENCES rev.billable_entities(id),
    company_id BIGINT NOT NULL REFERENCES rev.companies(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_company_billable_entities_object_id ON rev.company_billable_entities(object_id);
CREATE UNIQUE INDEX idx_company_billable_entities_billable ON rev.company_billable_entities(billable_entity_id);
CREATE UNIQUE INDEX idx_company_billable_entities_company ON rev.company_billable_entities(company_id);
```

**Notes:**
- Extends `billable_entities` for companies (class table inheritance)
- One billable entity per company (unique on company_id)
- One company per billable entity (unique on billable_entity_id)
- Created when company sets up billing (B2B scenarios)

**Rust Types:**
- `CompanyBillableEntityModel` — Full model with company details
- `InsertCompanyBillableEntityArgs` — `{ billable_entity_ids, company_ids }`

**Queries:**
- `insert_company_billable_entities.sql` — Create extension record
- `select_company_billable_entity_by_company.sql` — Get billable entity for company

**Acceptance Criteria:**
- [ ] Migration creates table with FKs
- [ ] Model in `dapple-data/src/model/billable_entities/company_billable_entities.rs`
- [ ] Single CTE insert creates both base + extension atomically
- [ ] Integration test: lookup company's billable entity
<!-- /ANCHOR -->

</details>
