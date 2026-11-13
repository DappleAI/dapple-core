# Query Patterns

> **Audience:** Backend Developers · Platform Engineers

SQL query patterns used in dapple-data for batch operations and relational objects.

## Core Principles

1. **Append-only by default** - Avoid UPDATE operations; use INSERT for versioning
2. **Set-wise operations** - Use column-wise array parameters with `UNNEST WITH ORDINALITY`
3. **UUIDv7 ordering** - Use `ORDER BY id` for creation-time ordering; no separate ordinal fields needed
4. **Minimal round-trips** - Flatten related data in queries; UUID FKs can be used directly in JOINs

## sql-composer Pipeline

All SQL is authored as `.sqlc` source files and compiled to `.sql` output. **Never edit `.sql/` files directly.**

```
assets/sqlc/**/*.sqlc  (authored — edit these)
        |
cargo sqlc compose --source assets/sqlc/ --target assets/.sql/
        |
assets/.sql/**/*.sql   (generated — committed to git, never hand-edited)
        |
sqlx::query_file_as!("assets/.sql/...", ...)  (Rust compile-time verification)
```

### Composition Directives

| Directive | Purpose | Example |
|-----------|---------|---------|
| `:bind(name)::TYPE` | Named parameter — assigned `$N` alphabetically | `:bind(names)::TEXT[]` |
| `:compose(path.sqlc)` | Inline content from shared file | `:compose(shared/insert_object_paths.sqlc)` |

### Alphabetical Bind Ordering

sql-composer assigns `$N` placeholders by **alphabetically sorting all `:bind()` names** across the entire composed query (including composed-in files). The same `:bind(name)` used in multiple places maps to the same `$N`.

Example: binds `names`, `descriptions`, `content_hashes` -> `content_hashes=$1`, `descriptions=$2`, `names=$3`

**Rust `query_file_as!` parameters MUST match this alphabetical order.**

### Shared Statement Files (`assets/sqlc/shared/`)

| File | CTEs Provided | Used By |
|------|---------------|---------|
| `insert_object_paths.sqlc` | `new_paths`, `numbered_paths` | All versioned INSERT |
| `numbered_path_details.sqlc` | `numbered_path_details` | All versioned INSERT |
| `insert_versioned_objects.sqlc` | `new_versions`, `numbered_versions` | All versioned INSERT |
| `patch_new_versions.sqlc` | `new_versions`, `numbered_versions` | All versioned PATCH |
| `deprecate_latest_versions.sqlc` | `latest_deprecation`, `latest_versions`, `resolved` | All DEPRECATE |
| `deprecate_events.sqlc` | `events` | All DEPRECATE |

## Query File Structure

All query **sources** live in `crates/dapple-data/assets/sqlc/{principal_object}/` as `.sqlc` files. Generated `.sql` output lives in `assets/.sql/{principal_object}/`. Rust functions reference the generated path.

```
assets/sqlc/journeys/           # Source files (edit these)
|-- insert_journeys.sqlc
|-- select_journeys_by_ids.sqlc
+-- select_journeys_by_playbook.sqlc

assets/.sql/journeys/           # Generated output (never edit)
|-- insert_journeys.sql
|-- select_journeys_by_ids.sql
+-- select_journeys_by_playbook.sql
```

## INSERT Query Pattern

```sql
-- Source: assets/sqlc/journeys/insert_journeys.sqlc
-- Generated: assets/.sql/journeys/insert_journeys.sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        :bind(names)::TEXT[],
        :bind(playbook_ids)::UUID[]
    ) WITH ORDINALITY AS o(name, playbook_id, ord)
),
objs AS (
    INSERT INTO web.journeys (name, playbook_id, created_at)
    SELECT args.name, args.playbook_id, now()
    FROM args
    ORDER BY args.ord
    RETURNING id, name, playbook_id, created_at
)
SELECT objs.id, objs.name, objs.playbook_id, objs.created_at
FROM objs
ORDER BY objs.id;
```

Key points:
- `:bind(names)::TEXT[]` instead of positional `$1::TEXT[]`
- `id UUID DEFAULT uuidv7()` PK — no `object_id` or `gen_random_uuid()`
- `ORDER BY args.ord` in the INSERT ensures UUIDv7s are generated in array order
- Final SELECT uses `ORDER BY objs.id` — no `ord` in the output

## SELECT Query Pattern

```sql
-- Source: assets/sqlc/journeys/select_journeys_by_ids.sqlc
WITH args AS (
    SELECT * FROM UNNEST(:bind(ids)::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    j.id,
    j.name,
    j.created_at,
    -- Owned Dependent fields with prefix
    s.id as step_id,
    s.name as step_name,
    -- Shared Dependent as UUID FK
    t.id as task_id
FROM args
JOIN web.journeys j ON j.id = args.id
LEFT JOIN web.journey_steps js ON js.journey_id = j.id
LEFT JOIN web.steps s ON s.id = js.step_id
LEFT JOIN web.tasks t ON t.id = s.task_id
ORDER BY j.id, s.id;
```

## PATCH Query Pattern (Versioned Objects)

```sql
-- Source: assets/sqlc/playbooks/patch_playbooks.sqlc
WITH args AS (
    SELECT *
    FROM UNNEST(
        :bind(content_hashes)::TEXT[],
        :bind(descriptions)::TEXT[],
        :bind(path_ids)::UUID[]
    ) WITH ORDINALITY AS o(content_hash, description, path_id, ord)
),
paths AS (
    SELECT
        pp.id as path_id,
        COALESCE(MAX(p.version_major), 0) as latest_major,
        COALESCE(MAX(p.version_minor), 0) as latest_minor,
        COALESCE(MAX(p.version_patch), 0) as latest_patch,
        args.ord
    FROM args
    JOIN rev.playbook_paths pp ON pp.id = args.path_id
    LEFT JOIN rev.playbooks p ON p.path_id = pp.id
    GROUP BY pp.id, args.ord
),
:compose(shared/patch_new_versions.sqlc)
-- entity-specific INSERT + SELECT follows...
```

## Versioned INSERT Pattern

Versioned objects use shared CTEs for path creation and version tracking:

```sql
-- Source: assets/sqlc/tasks/insert_tasks.sqlc
WITH args AS (
    SELECT *
    FROM UNNEST(
        :bind(names)::TEXT[],
        :bind(descriptions)::TEXT[],
        :bind(content_hashes)::TEXT[]
    ) WITH ORDINALITY AS o(name, description, content_hash, ord)
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'task'
),
:compose(shared/insert_object_paths.sqlc)
path_details AS (
    INSERT INTO rev.task_path_details (path_id, name, slug)
    SELECT np.id, args.name, rev.slugify(args.name)
    FROM args JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, name, slug
),
:compose(shared/numbered_path_details.sqlc)
:compose(shared/insert_versioned_objects.sqlc)
-- entity-specific INSERT + SELECT follows...
```

## CTE Order Convention

1. `args` - UNNEST input parameters
2. `new_paths` / `numbered_paths` - Create/lookup paths (versioned only)
3. `path_details` / `numbered_path_details` - Path-level metadata (versioned only)
4. `new_versions` / `numbered_versions` - Version envelope (versioned only)
5. `inputs` - Prepare data, look up shared dependents
6. `objs` - Main INSERT
7. Final SELECT - `ORDER BY objs.id`

## Anti-Pattern: Separate ID Lookups

**Don't do this:**
```rust
// Multiple round-trips
let brief_ids = get_brief_ids(&pool, &brief_uuids).await?;
let inserted = insert_brief_groups(&pool, args).await?;
insert_brief_group_briefs(&pool, assoc_args).await?;
```

**Do this instead:**
```sql
-- Single query with CTE — UUID FKs used directly
WITH args AS (...),
brief_refs AS (
    SELECT b.id as brief_id, ref.ordinal
    FROM args
    CROSS JOIN LATERAL UNNEST(args.brief_ids) WITH ORDINALITY AS ref(id, ordinal)
    JOIN rev.briefs b ON b.id = ref.id
),
new_groups AS (INSERT INTO rev.brief_groups ... RETURNING *),
new_assocs AS (INSERT INTO rev.brief_group_briefs ... RETURNING *)
SELECT ... FROM new_groups ...;
```

## Key Guidelines

- **ALWAYS** use `UNNEST WITH ORDINALITY` for batch operations
- **Use `:bind(name)::TYPE`** for named parameters — never positional `$N` in `.sqlc` files
- **Never edit `.sql/` directly** — always edit `.sqlc` sources and run `cargo sqlc compose`
- **Use CTEs** for readability and single round-trip
- **Join owned dependents** for FlatModel results
- **Return `id: Uuid`** for all principal/versioned objects — no `object_id`
- **ORDER BY id** in final SELECT — no `ord` in output (UUIDv7 preserves insertion order)
- **Never inline SQL** - always use query files

## Workflow Commands

```bash
# After editing .sqlc files, regenerate .sql output:
cd crates/dapple-data
cargo sqlc compose --source assets/sqlc/ --target assets/.sql/

# Verify generated files match sources (CI check):
cargo sqlc compose --verify --source assets/sqlc/ --target assets/.sql/

# Then prepare sqlx metadata (preferred: `dapple sqlx prepare`):
cargo sqlx prepare --workspace -- --all-targets
```
