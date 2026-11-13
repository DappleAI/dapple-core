# Migration Guide

> **Audience:** Backend Developers · Platform Engineers

Guidelines for creating database migrations in dapple-data.

## Migration File Structure

Migrations live in `crates/dapple-data/migrations/`:

```
migrations/
├── 20240115_initial_schema.sql
├── 20240116_add_journeys.sql
├── 20240117_seed_dispatch_phases.sql
└── ...
```

## Naming Conventions

- `YYYYMMDD_description.sql` - Standard migration
- `YYYYMMDD_seed_{table_name}.sql` - Seed data migration
- `YYYYMMDD_insert_{table_name}.sql` - Insert-only migration

## Table Design Rules

### Primary Keys

```sql
-- Principal and versioned tables: UUID with UUIDv7
id UUID DEFAULT uuidv7() PRIMARY KEY

-- Lookup/enum tables only: BIGINT identity
id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY
```

There is no separate `object_id` column — `id` is the UUID primary key used both internally and externally.

### Foreign Keys

```sql
-- Named {related_table_singular}_id, UUID FK for principal/versioned tables
playbook_id UUID NOT NULL REFERENCES web.playbooks(id),

-- ALWAYS add explicit index
CREATE INDEX idx_journeys_playbook_id ON web.journeys(playbook_id);
```

### Timestamps

```sql
-- Use TIMESTAMPTZ, named {adjective}_at
created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
updated_at TIMESTAMPTZ,
revoked_at TIMESTAMPTZ,
completed_at TIMESTAMPTZ
```

## Prohibited Data Types

| Avoid | Use Instead |
|-------|-------------|
| `timestamp` | `timestamptz` |
| `char(n)`, `varchar(n)` | `text` with CHECK constraint |
| `serial` | `bigint generated always as identity` |
| `money` | `numeric` |
| `timetz` | `timestamptz` |
| `timestamptz(0)` | `timestamptz` (no precision) |

## Table Naming

- Plural snake_case: `journeys`, `playbook_steps`
- Associative tables: `{principal_singular}_{dependent_singular}`
- Path tables: `{principal_singular}_paths`

## Schema Organization

| Schema | Purpose |
|--------|---------|
| `public` | Accessible to all services |
| `web` | Web application tables |
| `rev` | RevOps analytics, billing, auditing |
| `auth` | NextAuth.js tables |

## Example: Creating a Versioned Object

```sql
-- 20240120_add_briefs.sql

-- Path table (tracks version lineage)
CREATE TABLE rev.brief_paths (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Versioned table
CREATE TABLE rev.briefs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    path_id UUID NOT NULL REFERENCES rev.brief_paths(id),
    versioned_object_id UUID NOT NULL REFERENCES rev.versioned_objects(id),

    -- Data columns
    name TEXT NOT NULL,
    title TEXT,
    description TEXT,
    content_hash TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- FK indexes
CREATE INDEX idx_briefs_path_id ON rev.briefs(path_id);
CREATE INDEX idx_briefs_versioned_object_id ON rev.briefs(versioned_object_id);
```

Version columns (`version_major`, `version_minor`, `version_patch`) and `content_hash` live on `rev.versioned_objects`, not on the entity table directly.

## Example: Creating an Associative Table

```sql
-- 20240121_add_brief_group_briefs.sql

CREATE TABLE rev.brief_group_briefs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    brief_group_id UUID NOT NULL REFERENCES rev.brief_groups(id),
    brief_id UUID NOT NULL REFERENCES rev.briefs(id),

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (brief_group_id, brief_id)
);

CREATE INDEX idx_brief_group_briefs_group_id ON rev.brief_group_briefs(brief_group_id);
CREATE INDEX idx_brief_group_briefs_brief_id ON rev.brief_group_briefs(brief_id);
```

No `ordinal` column is needed — UUIDv7 IDs encode insertion order. Use `INSERT ... ORDER BY args.ord` to ensure UUIDv7s are generated in sequence, then `SELECT ... ORDER BY id` to return them in order.

## Seed Data

All seed data MUST be in migration files:

```sql
-- 20240117_seed_dispatch_phases.sql

INSERT INTO rev.dispatch_phases (code, name, description) VALUES
    ('queued', 'Queued', 'Task is waiting to be executed'),
    ('running', 'Running', 'Task is currently executing'),
    ('failed', 'Failed', 'Task execution failed'),
    ('ok', 'Completed', 'Task completed successfully'),
    ('canceled', 'Canceled', 'Task was canceled'),
    ('unsafe', 'Unsafe', 'Task vetoed the step via signals.unsafe — do not refire');
```

## Running Migrations

**NEVER apply migrations with `psql -f`.** Always use `cargo sqlx migrate run`:

```bash
# Apply pending migrations
cargo sqlx migrate run

# Check migration status
cargo sqlx migrate info

# Prepare SQLx offline mode (preferred: `dapple sqlx prepare`)
cargo sqlx prepare --workspace -- --all-targets
```

## SQLx Offline Mode

- Set `SQLX_OFFLINE=true` for CI/Docker builds
- After changing `.sqlc` files, run `dapple sqlc compose` (compose + prepare in one), or the raw `cargo sqlc compose` then `cargo sqlx prepare --workspace -- --all-targets`
- Commit `.sqlx/` directory for offline verification

## Common Pitfalls

- Missing FK indexes (PostgreSQL doesn't auto-create)
- Using `timestamp` without timezone
- Using `BIGINT` PKs on principal/versioned tables (use `UUID DEFAULT uuidv7()`)
- Not seeding lookup tables in migrations
- Using precision on `timestamptz`
- Running migrations with `psql -f` instead of `cargo sqlx migrate run`
