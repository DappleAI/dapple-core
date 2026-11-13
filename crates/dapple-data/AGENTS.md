# Agent Instructions — dapple-data

Quick reference for agents working with the data access layer. See [README.md](./README.md) for full patterns and examples.

> **See also:** [`docs/books/journey-builder-guide/data-layer/`](../../docs/books/journey-builder-guide/) — narrative documentation of the data-layer patterns covered tersely here.

## Core Philosophy

### Data Layer Principles
1. **Append-only by default** — Avoid UPDATE operations; use INSERT for versioning and soft deletes
2. **Set-wise operations** — Use column-wise array parameters with `UNNEST WITH ORDINALITY` for batch operations
3. **UUIDv7 ordering** — Use `ORDER BY id` for creation-time ordering; UUIDv7's timestamp-based generation provides natural time ordering. `UNNEST WITH ORDINALITY` in CTEs preserves the caller's array order during INSERT, ensuring UUIDv7s are generated in the intended sequence. This means **array position → id order** — callers specify semantic ordering simply by arranging arrays; no explicit ordinal fields needed in CreateSchemas. The `ord` column stays internal to CTEs and is never included in final SELECT/ORDER BY.
4. **Minimal round-trips** — Flatten related data in queries to reduce database calls. UUID FKs can be used directly in JOINs — no internal ID resolution needed. See "Database Round-Trip Anti-Pattern" section.
5. **Type safety** — Use SQLx compile-time verification via `query_file_as!` macro
6. **Offline Mode** - Use `SQLX_OFFLINE=false` for local development, a `sql prepare` will be handled by the software engineers when pushing to local docker images.

### Relational Object Model

**Principal Object** — Top-level entity with independent lifecycle (e.g., Playbook, Journey, Step, Task)
- Has dedicated table in database
- Created independently of other objects
- May have Path and Versioned variants

**Owned Dependent** — Data owned by a Principal Object
- Same lifecycle as parent
- Created/deleted with parent
- Foreign key to parent table

**Shared Dependent** — Data shared across multiple principals
- Independent lifecycle
- Must exist before being referenced
- Referenced by `id` (UUID)

**Associative Dependent** — Many-to-many join table between principals
- Special type of Owned Dependent
- Named `{principal_singular}_{dependent_singular}` (e.g., `brief_group_briefs`, `prompt_fluent_messages`)
- **NO separate API endpoint** — created/queried as part of the parent Principal
- Parent's CreateSchema includes references (e.g., `brief_ids: Vec<Uuid>`)
- Parent's DTO includes the associations (e.g., `briefs: Vec<BriefReference>`)
- Created atomically with parent via single INSERT query with CTE

**Versioned Relational Object** — Immutable versioned data
- Contains `path_id` referencing Path Object
- Has version columns: `version_major`, `version_minor`, `version_patch`
- Updated via PATCH (creates new version)

**Path Relational Object** — Tracks version history
- Named `{table_singular}_paths`
- One-to-many with Versioned Objects
- Contains immutable path data

## Type Naming Conventions

### Data Transfer Objects (DTOs)
```rust
// Public-facing DTO (for API responses)
pub struct Journey {
    pub id: Uuid,               // UUIDv7 primary key
    pub name: String,
    pub playbook_id: Uuid,      // FK as UUID directly
    pub steps: Vec<Step>,       // Owned Dependents as Vec
}
```

**DTO Conventions:**
- The `id: Uuid` field is the primary key (UUIDv7). Lookup tables use `id: i64` internally.
- Prefer nullable timestamps over booleans for state: use `revoked_at: Option<DateTime<Utc>>` instead of `is_revoked: bool`. The timestamp provides both the state and when it happened.
- Owned Dependents appear as `Vec<DependentDto>`
- Shared Dependents appear as `{table_name}_id: Uuid`

## Quick Checklist: Adding a New Entity

### Principal Object
1. [ ] Migration in `migrations/YYYYMMDD_create_{table}.sql`
2. [ ] `.sqlc` source files in `assets/sqlc/{entity}/` (see [sql-composer Workflow](#sql-composer-workflow)):
   - `insert_{entity}.sqlc` — use `:compose()` for shared CTEs, `:bind()` for parameters
   - `select_{entities}_body.sqlc` — body template with `:compose(@page)` slot
   - `pages/{variant}.sqlc` — page CTE files (by_object_ids, by_slugs, all, etc.)
   - `select_{entities}_by_{variant}.sqlc` — one-liner callers using `:compose(body.sqlc, @page = pages/{variant}.sqlc)`
3. [ ] Run `cargo sqlc compose` to generate `assets/.sql/{entity}/` output
4. [ ] Models in `src/model/{entity}.rs`:
   - `{Entity}Model` (internal, with `id: Uuid`)
   - `Flat{Entity}Model` (for JOIN results)
   - `Inserted{Entity}Model` (with `ord: i32`)
   - `Insert{Entity}Args` (column-wise vectors)
5. [ ] Query functions in `src/query/{entity}.rs` — bind params in **alphabetical order**
6. [ ] Run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`)

### Versioned Object
Same as above, plus:
1. [ ] Path table: `{entity}_paths` with `id UUID DEFAULT uuidv7()`
2. [ ] Versioned table includes `path_id UUID`, `version_major/minor/patch`
3. [ ] INSERT `.sqlc` uses `:compose(shared/insert_object_paths.sqlc)`, `:compose(shared/numbered_path_details.sqlc)`, `:compose(shared/insert_versioned_objects.sqlc)`
4. [ ] PATCH `.sqlc` uses `:compose(shared/patch_new_versions.sqlc)`
5. [ ] DEPRECATE `.sqlc` uses `:compose(shared/deprecate_latest_versions.sqlc)`, `:compose(shared/deprecate_events.sqlc)`

## Migrations

**NEVER apply migrations with `psql -f`.** Always use `cargo sqlx migrate run`. This ensures:
- Migrations are tracked in the `_sqlx_migrations` table
- Migrations are applied exactly once
- Migration state is consistent across environments
- Rollbacks and status checks work via `cargo sqlx migrate info`

```bash
# Preferred wrappers (run from the right dir, DATABASE_URL already in the env):
nu -c 'use assets/nu/modules/dapple; dapple sqlx migrate'   # apply pending migrations
nu -c 'use assets/nu/modules/dapple; dapple sqlx info'      # migration status

# Raw equivalents (run from crates/dapple-data so ./migrations resolves):
# Apply pending migrations
cargo sqlx migrate run

# Check status
cargo sqlx migrate info
```

## Common Mistakes

### SQL / sql-composer
- **Never edit `.sql/` files directly** — Always edit `.sqlc` sources in `assets/sqlc/`, then run `dapple sqlc compose` (or `cargo sqlc compose`)
- **Shared fragments are complete queries only** — never a clause fragment (dangling `LEFT JOIN ...`) or a parenthesized expression; the call site owns the parens/JOIN/CTE wrapper (see "Shared fragments are COMPLETE queries only")
- **Comment with `#`, never `--`** — `#` comments are stripped from the generated `.sql`; `--` comments are passed through to the database. -- comments are small notes to the DBAs 
- **Alphabetical bind ordering** — `:bind()` names are sorted alphabetically across the entire composed query. Rust `query_file_as!` params must match this order.
- **Missing `ord`** — Always include `ord` in final SELECT and ORDER BY
- **No UNNEST** — Always use `UNNEST WITH ORDINALITY` for batch operations
- **Inline SQL** — Never inline SQL strings; always use query files

### Models
- **Mixing PK conventions** — Principal/versioned tables use `id: Uuid`; only lookup tables use `id: i64`
- **Mismatched vectors** — All vectors in `Insert*Args` must have identical length
- **Wrong type suffix** — Use exact names: `FlatXModel`, `InsertedXModel`, `InsertXArgs`

### Queries
- **UUID FKs are direct** — Since all principal/versioned tables use UUID PKs, FK values from the API can be used directly in INSERT args. No CTE resolution from `object_id` to `id` is needed.
- **Multiple round-trips** — Batch all related inserts in a single query with CTEs
- **Wrong bind order** — Rust `query_file_as!` parameters must be passed in alphabetical order of `:bind()` names. Check the generated `.sql` file to verify `$N` assignments.

## SQL CTE Order

```sql
WITH args AS (...)       -- 1. UNNEST input parameters
paths AS (...)           -- 2. Create/lookup paths (versioned only)
inputs AS (...)          -- 3. Resolve shared dependents
objs AS (...)            -- 4. Main INSERT
SELECT ... ORDER BY ord; -- 5. Final SELECT
```

### Pipeline

```
assets/sqlc/**/*.sqlc  (authored — edit these)
        ↓
cargo sqlc compose --source assets/sqlc/ --target assets/.sql/
        ↓
assets/.sql/**/*.sql   (generated — committed to git, never hand-edited)
        ↓
sqlx::query_file_as!("assets/.sql/...", ...)  (Rust compile-time verification)
```

### Composition Directives

| Directive | Purpose | Example |
|-----------|---------|---------|
| `:bind(name)::TYPE` | Named parameter — assigned `$N` alphabetically | `:bind(names)::TEXT[]` |
| `:compose(path.sqlc)` | Inline content from shared file | `:compose(shared/insert_object_paths.sqlc)` |
| `:compose(@slot)` | Declare a named slot (filled by caller) | `:compose(@page)` |
| `:compose(body.sqlc, @slot = filler.sqlc)` | Compose a template filling a slot with a file | `:compose(tasks/select_tasks_body.sqlc, @page = tasks/pages/by_object_ids.sqlc)` |
| `# comment` | Documentation comment — **STRIPPED** from generated `.sql` | `# True iff every exposed slot is bound` |
| `-- comment` | SQL comment — **PASSED THROUGH** to the database | (avoid; use `#` instead) |

### Shared fragments are COMPLETE queries only (hard rule)

A shared `.sqlc` file (anything under `assets/sqlc/shared/` meant for `:compose`) must contain a
**complete query** — a full `SELECT ...` statement. Correlated references to an outer alias are
fine when documented via a `#`-comment alias contract (see
`shared/select/latest_deprecation_probe.sqlc`). What is NEVER allowed:

- a bare clause fragment (a dangling `LEFT JOIN ...`, a loose `WHERE ...` arm), or
- a parenthesized expression — the wrapping `()` does not belong in the file.

The **call site owns all wrapper syntax**. That is what keeps the fragment reusable: the same
complete query composes as a scalar expression `(:compose(f))`, a LATERAL body
`LEFT JOIN LATERAL (:compose(f)) x ON TRUE`, or a CTE body `name AS (:compose(f))` — and every
composing source file remains a complete statement in its own right. A site that needs a different
correlation alias inlines the SELECT verbatim with a `#` pointer comment to the shared file
instead of composing it. (`shared/fork_cutoff_filter.sqlc`, a bare predicate, predates this rule
and is grandfathered — do not copy its shape.)

### Comments: use `#`, never `--`

sql-composer strips `#` comments from the composed `.sql`, so they document the
`.sqlc` source without ever reaching Postgres. `--` comments are passed through
verbatim and DO hit the database. **Document `.sqlc` files exclusively with `#`.**
Reserve `--` for the rare case you genuinely want a comment in the DB-facing SQL
(almost never).

- Works as a full line (`# rationale...`) or trailing on an SQL line
  (`$1::UUID[],   # journey_id[]`).
- A leading `#` line is removed cleanly. An inline `#` also consumes its
  newline, so a trailing-comment line can merge with the next one in the
  generated output — harmless (whitespace is insignificant in SQL outside
  string literals), and verified to leave the query token stream unchanged.
- A `#` anywhere on a line strips to end-of-line, so do not use the jsonb path
  operators `#>` / `#>>` / `#-` in `.sqlc` (they would be eaten). Use `->` /
  `->>` or the helper functions instead. (No query currently needs them.)

### Versioned INSERT `.sqlc` Pattern

```sql
-- File: assets/sqlc/tasks/insert_tasks.sqlc
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

### SELECT `.sqlc` Pattern (Slot-Based)

SELECT queries use the **slot pattern** to eliminate duplication. Each entity has:
1. A **body template** (`select_{entities}_body.sqlc`) with `:compose(@page)` slot — contains shared JOINs, SELECT columns, and `latest_deprecation`
2. **Page CTE files** (`pages/{variant}.sqlc`) — each defines how to obtain the initial set of entity IDs
3. **Caller files** (`select_{entities}_by_{variant}.sqlc`) — one-liners that compose the body with a specific page

```sql
-- Body template: assets/sqlc/tasks/select_tasks_body.sqlc
WITH :compose(shared/select/latest_deprecation.sqlc)
:compose(@page)
SELECT t.id, t.versioned_object_id, ...
FROM task_page tkp
JOIN rev.tasks t ON t.id = tkp.id
...
ORDER BY t.id;
```

```sql
-- Page CTE: assets/sqlc/tasks/pages/by_object_ids.sqlc
args AS (
    SELECT * FROM UNNEST(:bind(object_ids)::UUID[]) WITH ORDINALITY AS o(id, ord)
),
task_page AS (
    SELECT t.id FROM args JOIN rev.tasks t ON t.id = args.id
    ORDER BY args.ord
    LIMIT :bind(limit)::BIGINT OFFSET :bind(offset)::BIGINT
)
```

```sql
# Caller: assets/sqlc/tasks/select_tasks_by_object_ids.sqlc
:compose(tasks/select_tasks_body.sqlc, @page = tasks/pages/by_object_ids.sqlc)
```

### Workflow Commands

```bash
# Preferred: compose .sqlc -> .sql AND prepare the sqlx offline cache, one step.
# (Wraps the raw commands below; run `nu assets/nu/init/setup.nu --environment sandbox` once to
#  put `dapple sqlc` on your PATH.)
nu -c 'use assets/nu/modules/dapple; dapple sqlc compose'

# CI drift check (read-only): does generated .sql match .sqlc sources?
nu -c 'use assets/nu/modules/dapple; dapple sqlc verify'

# Raw equivalents:
# Generate .sql/ from .sqlc sources (after editing .sqlc files)
cargo sqlc compose --source assets/sqlc/ --target assets/.sql/

# Verify generated files match sources (CI check)
cargo sqlc compose --verify --source assets/sqlc/ --target assets/.sql/

# After compose, prepare sqlx metadata (compose runs a crate-local prepare;
# the workspace prepare covers api/flow/external-api which also reference these
# queries — `dapple sqlc compose` does this for you). `-- --all-targets` makes
# the cache cover test-target inline `sqlx::query!` macros too.
cargo sqlx prepare --workspace -- --all-targets
```

> `cargo sqlc compose` runs `cargo sqlx prepare` for the dapple-data crate by
> default; pass `--skip-prepare` to suppress it. `dapple sqlc compose` skips that
> crate-local prepare and runs `cargo sqlx prepare --workspace -- --all-targets` instead so every
> prompt crate's offline cache stays in sync. **Because sqlx hashes the full
> query text, removing/adding comments rewrites the `.sqlx` cache — always
> recompose + prepare and commit the regenerated `.sqlx/` alongside the `.sql`.**

## SQL Query Patterns

### Query File Structure
All query **sources** live in `assets/sqlc/{principal_object}/` as `.sqlc` files. Generated `.sql` output lives in `assets/.sql/{principal_object}/`. Rust functions reference the generated path: `"assets/.sql/{principal_object}/..."`.

### INSERT Query Pattern
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

### PATCH Query Pattern (for Versioned Objects)
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
paths AS (...),
:compose(shared/patch_new_versions.sqlc)
-- entity-specific INSERT + SELECT...
```

### Key SQL Guidelines
- **UNNEST WITH ORDINALITY preserves array order** — The `ord` column ensures `INSERT ... ORDER BY args.ord` generates UUIDv7s in the caller's intended sequence. This captures semantic ordering in `id` without needing stored ordinal columns or explicit ordinal fields in CreateSchemas.
- **ORDER BY id** for semantic ordering — Since array position flows through to UUIDv7 generation, `ORDER BY id` returns items in the order the caller specified.
- **No `ord` in final SELECT** — The `ord` column stays internal to CTEs; it's a mechanism, not output.
- **Use CTEs** for readability: `args` → `inputs` → `paths` → `objs` → final SELECT
- **Join owned dependents** for FlatModel results
- **Only return `id`** for shared dependents — UUID FKs reference the shared table's `id` directly
- **Never inline SQL** — always use query files
- **Stored `ordinal` columns are rarely needed** — In append-only systems, semantic ordering is captured by UUIDv7 `id`. Associative tables should have their own `id` column so ordering works the same way: `INSERT ... ORDER BY` ensures UUIDv7s are generated in sequence, `SELECT ... ORDER BY id` returns the intended order. No stored ordinal columns, no explicit ordinal fields in CreateSchemas. Exceptions:
  - `rev.asset_chunks` / `rev.draft_chunks` — ordinal is byte-range assembly position (data semantics, not just ordering).
  - `rev.brief_units.ord` / `rev.brief_outputs.ord` (migration 95) — **one authored sequence that spans two tables.** A brief's units are authored as a single ordered list but materialize into brief_units (inputs/notices/markdown) and brief_outputs (outputs) as two separate INSERT batches, so id-magnitude ordering sorts every unit before every output no matter how the author interleaved them. `/steps` has to render both together, so it needs the shared 1-based ordinal both tables carry (`0` = a pre-migration-95 row, which ties and falls through to the historical name ordering). **Do not "simplify" these away to `COALESCE(brief_unit_id, brief_output_id)`** — that is the exact bug migration 95 fixed, and `test_steps_orders_exposed_slots_by_authored_position` fails on it by design.
    - **The legacy sentinel only works if nothing sits between the ordinal and `vspd.name`.** The first cut of the reader sorted by `brief_id` above the name tiebreak, on the theory that a step's slots should cluster by brief. `brief_id` is a real value on legacy rows, so the `ord = 0` tie never reached the name — single-brief steps looked fine (which is why the first test passed) while **68% of steps expose slots from 2+ briefs** and every one of those legacy panes silently reordered. `test_steps_legacy_zero_ord_keeps_name_ordering` zeroes the ordinals of a two-brief fixture and fails on any such key. If cross-brief order ever genuinely needs authoring, it wants an ordinal on the brief-within-group link — not `brief_id`, which orders by insert time and answers a different question.

## Rust Query Function Patterns

### Insert Function
```rust
// File: crates/dapple-data/src/query/journeys.rs
pub async fn insert_journeys(
    pool: &PgPool,
    args: InsertJourneyArgs,
) -> Result<Vec<InsertedJourneyModel>, DataError> {
    // Validate all vectors are same length
    assert_eq!(args.names.len(), args.playbook_ids.len());

    let rows = sqlx::query_file_as!(
        InsertedJourneyModel,
        "assets/.sql/journeys/insert_journeys.sql",
        // Parameters MUST be in alphabetical order of :bind() names
        &args.names,         // :bind(names) → $1
        &args.playbook_ids   // :bind(playbook_ids) → $2
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
```

### Select Function
```rust
pub async fn select_journeys_by_ids(
    pool: &PgPool,
    ids: &[Uuid],
) -> Result<Vec<JourneyModel>, DataError> {
    let flat_rows = sqlx::query_file_as!(
        FlatJourneyModel,
        "assets/.sql/journeys/select_journeys_by_ids.sql",
        ids
    )
    .fetch_all(pool)
    .await?;

    // Fold flat rows into structured models
    let models = fold_journeys(flat_rows)?;
    Ok(models)
}

// Helper to convert flat rows to structured models
fn fold_journeys(flat: Vec<FlatJourneyModel>) -> Result<Vec<JourneyModel>, DataError> {
    // Group by journey id, collect steps
    // Implementation uses itertools::group_by or manual folding
}
```

### Schema Organization
- `public` — Accessible to all services, for external tools
- `web` — Web application tables, user-facing features, session/auth data
- `rev` — RevOps analytics, billing, auditing (largest schema; also hosts the canonical org primitive `rev.companies` and the customer-side `rev.external_service_tokens`)
- `auth` — NextAuth.js tables (not modified by this workspace except in tests)

### Namespace + scope context via Postgres GUCs

Internal flows don't set the GUCs → defaults to dapple + 'public', identical to pre-GUC behavior. External ingest (`dapple-prompt-external-api`'s `POST /api/guides/ingest`) opens a transaction, calls `set_config(name, value, true)` (true = `is_local`, scoped to the txn), and every nested INSERT picks up the customer's namespace + `'external'` scope automatically. The override mechanism lives in `dapple-prompt-api::handler::guide_ingest::NamespaceOverride` (a pub struct) consumed by the pub `ingest_guide_tarball` and `materialize_guide_with_hash` entry points.

### Seed Data and Lookup Tables

**All seed data MUST be in migration files.** Never insert reference data directly via psql, scripts, or other means outside of migrations.

**Lookup/Enum Tables** — Tables that define valid values for foreign key references:
- `rev.value_kinds` — Type system for value slots (text, int, number, bool, json, asset, **dataset**). `dataset` (the 7th, spreadsheet / tabular) is stored as a `rev.dispatch_intent_binding_dataset_items` row that PINS the current `rev.asset_dataset_extractions` row by hard FK (shared reference, no envelope copy; "current" = latest per asset, no `is_current` flag) — the first binding item table with hard FK integrity; envelope-not-rows. See `docs/plans/spreadsheet-dataset-ingest.md`.
- `rev.asset_kinds` — Asset type classification (image, document, audio, video, binary)
- `rev.dispatch_phases` — Task-attempt lifecycle states. Non-terminal: `queued`, `running`. Terminal: `ok`, `failed` (retryable via re-dispatch), `canceled` (deliberately stopped), `unsafe` (a task's `signals.unsafe = true` veto — poisoned, do NOT re-dispatch). A step's derived verdict is worst-wins across its task attempts (any `unsafe` → unsafe; else any `failed` → failed; else `ok`).
- `rev.brief_unit_types` — Brief UI element types (output, markdown, notice, input)
- `rev.brief_input_control_types` — Input control variants (text, textarea, select, radio, etc.)
- `rev.brief_output_source_kinds` — Output data sources (input, files, computed)

**Migration Naming Conventions:**
- `YYYYMMDD_seed_{table_name}.sql` — Dedicated seed migration (separate from DDL)
- `YYYYMMDD_insert_{table_name}.sql` — Alternative naming for insert-only migrations
- Embedded in DDL — Acceptable when table creation and seeding are tightly coupled


### Internal Models
```rust
// Complete internal model — UUID-only PK
pub(crate) struct JourneyModel {
    pub id: Uuid,                   // UUIDv7 primary key
    pub name: String,
    pub playbook_id: Uuid,          // UUID FK directly
    pub steps: Vec<StepModel>,
}

// Flattened query result (from JOIN)
pub(crate) struct FlatJourneyModel {
    // Journey fields
    pub id: Uuid,                   // UUIDv7 primary key
    pub name: String,
    // Owned Dependent fields prefixed with table name
    pub step_id: Option<Uuid>,      // UUID FK
    pub step_name: Option<String>,
    // Shared Dependent as UUID directly
    pub task_id: Option<Uuid>,      // UUID FK
}

// INSERT...RETURNING result
pub(crate) struct InsertedJourneyModel {
    pub id: Uuid,                   // UUIDv7 primary key
    pub name: String,
    // No ord field - use ORDER BY id for time-based ordering (UUIDv7)
}

// Column-wise insert arguments
pub(crate) struct InsertJourneyArgs {
    pub names: Vec<String>,
    pub playbook_ids: Vec<Uuid>,    // UUID FKs directly
    // All vectors MUST be same length
}
```

## Database Schema Guidelines

### Table Design Rules
- **Primary keys:** `UUID DEFAULT uuidv7()` for principal and versioned tables; `SMALLINT/BIGINT GENERATED ALWAYS AS IDENTITY` for lookup tables only
- **Foreign keys:** Named `{related_table_singular}_id`, always add explicit index. UUID FKs for principal/versioned tables; integer FKs for lookup tables.
- **Timestamps:** Use `TIMESTAMPTZ`, named `{adjective}_at` (e.g., `created_at`, `updated_at`)
- **Constraints:** Use `NOT NULL`, `CHECK`, `UNIQUE` constraints liberally
- **Indexes:** Create for FK columns, frequent filters, and join keys

### Table Naming
- Plural snake_case: `journeys`, `playbook_steps`
- Associative tables: `{principal_singular}_{dependent_singular}` (e.g., `playbook_step`)
- Path tables: `{principal_singular}_paths` (e.g., `journey_paths`)

### Column Naming
- Primary key: `id` (UUID for principal/versioned tables, SMALLINT/BIGINT for lookup tables)
- Foreign key: `{table_singular}_id`
- Timestamps: `{adjective}_at`
- Version columns: `version_major`, `version_minor`, `version_patch`

**Special cases:**
- `user_id` remains `BIGINT` in `rev.user_details` and FK tables referencing `auth.users` (NextAuth compatibility)
- `versioned_object_id` is an FK column name (references a versioned object), not a PK pattern

**Prohibited data types:**
- ❌ `timestamp` → use `timestamptz`
- ❌ `char(n)` or `varchar(n)` → use `text` (with `CHECK (LENGTH(...) <= n)` if needed)
- ❌ `serial` → use `bigint generated always as identity` (lookup tables only)
- ❌ `money` → use `numeric`
- ❌ `timetz` → use `timestamptz`
- ❌ `timestamptz(0)` or any precision → use `timestamptz` (no precision specifier)

### Multi-Tenant Conventions for `*_path_details` Tables

Every `*_path_details` table that carries a human-readable identifier
(`name`, `slug`, `title`) MUST scope that UNIQUE constraint by namespace.
Composite `(name, namespace_company_id)` and `(slug, namespace_company_id)`
constraints are the convention; bare global UNIQUEs are a multi-tenant
landmine — two customers shipping a guide whose author picks the same
playbook/task/stage name would otherwise collide at the DB level.


### Database
```bash
# Wrappers (preferred): dapple sqlx <cmd> — runs from the right dir with
# DATABASE_URL already exported by the dapple env overlay.
dapple sqlx migrate          # Run migrations (ALWAYS sqlx, NEVER psql directly)
dapple sqlx info             # Check migration status
dapple sqlx prepare          # Prepare SQLx offline cache (--workspace --all-targets)
dapple sqlx prepare --check  # Verify offline cache is up to date (CI)

# Raw equivalents:
cargo sqlx migrate run
cargo sqlx migrate info
cargo sqlx prepare --workspace -- --all-targets
```

### SQLx Offline Mode
- Set `SQLX_OFFLINE=true` for CI/Docker builds
- Run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`) after changing `.sql` files
- Commit `.sqlx/` directory for offline verification


## File Locations

```
assets/sqlc/{entity}/       # SQL source files (.sqlc) — EDIT THESE
assets/sqlc/shared/         # Shared CTE fragments for composition
assets/.sql/{entity}/       # Generated SQL output — NEVER EDIT DIRECTLY
migrations/                 # DDL and seed data
src/model/{entity}.rs       # Rust types
src/query/{entity}.rs       # Query functions
src/hash.rs                 # Content-addressable hashing (ContentHash, hash_* functions)
src/lib.rs                  # ContentHash type definition, crate exports
```

## Chunked Asset Storage

For binary assets (audio, video, images) that need streaming or byte-range access:

### Data Model

```
drafts → draft_chunks (append-only during creation)
    ↓ finalize
assets → asset_drafts → drafts (provenance)
       → asset_chunks → chunks (immutable)
```

### Key Operations

### Prohibited Operations

- **UPDATE on chunks** — Chunks are immutable (audit trail integrity)
- **DELETE on referenced chunks** — Breaks asset integrity

### Common Mistakes

- **Forgetting asset_drafts provenance** — Assets must link to source draft via asset_drafts table
- **Not ordering by chunk.id** — Chunks must be read in insertion order

See [README.md](./README.md#chunked-asset-storage) for full documentation.

### Polymorphic Model Reference

`inference_profile_configs` and `fluent_message_engine_bodies` use polymorphic references:
- Exactly one of `model_release_id` OR `model_variant_id` must be set
- CHECK constraint enforces this at database level


## Declarative Macros (`src/macros.rs`)

Seven `macro_rules!` macros eliminate repetitive trait implementations across model and query files. All macros are available crate-wide via `#[macro_use] mod macros;` in `lib.rs`.

### Quick Reference

| Macro | Purpose | Example |
|-------|---------|---------|
| `impl_keyed!(Type)` | `Keyed` + `RelationalObject` | `impl_keyed!(TaskModel);` |
| `impl_keyed!(Type, inserted)` | Above + `InsertedModel` marker | `impl_keyed!(InsertedTaskModel, inserted);` |
| `impl_versioned!(Type, KEY)` | `VersionedRelationalObject` + `UnifiedVersionedObject` + `Depreciable` | `impl_versioned!(TaskModel, TASK);` |
| `impl_insert_column_args!(Type, [fields])` | `InsertColumnArgs` from field list | `impl_insert_column_args!(InsertTaskArgs, [names, descriptions]);` |
| `validate_insert_args!(fn_name, Type)` | Column-count validation function | `validate_insert_args!(validate_insert_task_args, InsertTaskArgs);` |
| `impl_principal_traits! { ... }` | Full trait bundle for versioned principals | See below |
| `impl_dto_id!(Type)` | `DataTransferObject` for DTOs with `id` | `impl_dto_id!(BriefUnit);` |
| `impl_flat_to_parent! { ... }` | `FlatRelationalObject::to_parent_inserted` | See below |

### `impl_principal_traits!` (most common)

Bundles `Keyed`, `RelationalObject`, `InsertedModel`, `PrincipalObject`, versioning traits, and `DataTransferObject`:

```rust
impl_principal_traits! {
    entity_key: TASK,           // constant from entity_type_keys
    inserted: InsertedTaskModel,
    model: TaskModel,
    dto: Task,
    display_field: name,        // optional — omit for no display name
}
```

### `impl_flat_to_parent!`

Generates `FlatRelationalObject::to_parent_inserted`. List `copy` fields (Copy types: Uuid, i32, OffsetDateTime, bool) and `clone` fields (String, Option<String>, serde_json::Value):

```rust
impl_flat_to_parent! {
    FlatTaskModel => InsertedTaskModel {
        copy: [id, versioned_object_id, path_id,
               version_major, version_minor, version_patch, created_at],
        clone: [name, slug, description, function, function_input],
    }
}
```

### When NOT to use macros

- **From/Into conversions** — Too much entity-specific variation (Version construction, child Vec defaults)
- **Association insert wrappers** — `sqlx::query_file_as!` requires string literal paths
- **Custom validate functions** — Dispatch/playbook_step validators have non-standard logic

## Content-Addressable Versioning

Content-addressable hashing provides deduplication for versioned objects. A `content_hash` (SHA-1) is computed from an entity's semantic content and stored on `rev.versioned_objects`. On PATCH/ingest, if the new hash matches the latest version, the operation short-circuits and returns the existing version.

### ContentHash Type (`src/lib.rs`)

```rust
#[derive(Clone, Eq, PartialEq, Hash, JsonSchema, Debug, Deserialize, Serialize)]
pub struct ContentHash(String);  // 40-char hex SHA-1

impl ContentHash {
    pub fn new(hash: String) -> Self
    pub fn short(&self) -> &str     // 8-char prefix for display
    pub fn as_str(&self) -> &str    // Full 40-char hash
}
```

### Hash Module (`src/hash.rs`)

**Utility functions:**
- `normalize_text(s: &str) -> String` — CRLF→LF, trim whitespace
- `canonical_json(value: &serde_json::Value) -> String` — Deterministic JSON via BTreeMap key sorting
- `sha1_hex(data: &[u8]) -> String` — SHA-1 hex digest
- `is_excluded_path(relative_path: &str) -> bool` — Filters `.git/`, `__MACOSX/`, `.DS_Store`, `.gitignore`, `.swp`, `.swo`

**Hash function inventory:**

hash_{dto}

**Merkle pattern:** Playbooks, stages, and steps use sorted child content hashes so that the parent hash changes when any descendant changes, enabling cascade dedup detection.

> **See also:** "Testing Repair Operations" in [`crates/dapple-prompt-api/AGENTS.md`](../dapple-prompt-api/AGENTS.md) — required reading before adding any test against a backfill / refresh / rebuild endpoint. A test that asserts only on a `*_repaired: true` response flag does not actually verify repair behavior.

## Data Integrity Rules
- **Append-only** — No UPDATE operations in normal flow
- **Soft deletes** — Track state changes in separate tables
- **Immutable versions** — PATCH creates new version for Versioned Objects
- **No triggers** — Except FK constraints and unique constraints
- **Audit everything** — Include `created_at`, `updated_at` timestamps
- **Journey ↔ Guide pinning** — see next section
- **Reference exact versions; don't bake copies or resolve latest-by-path** — see next section

## Versioned object references — pin exact versions; don't bake copies or resolve latest-by-path

The data model is a content-addressed DAG. `object_paths` / `*_path_details(name, slug)` are the
**identity of a thing across versions** (git-path-like); resolving by them means "the latest version",
which **drifts** over time. Each entity row's `id` (1:1 with a `versioned_object_id`) is a **specific
immutable version**.

**Rule:** when one versioned object references another for display or composition, **pin the exact
version id** — never (a) bake a text *copy* of the other object's content into your own row, nor
(b) store/resolve the relationship by path/name/slug (= latest).

**Legitimate uses of path/name/slug** (these are NOT the default for stored references): upsert/lineage
(deciding two rows are the same thing across versions), browse/catalog ("newest of each"),
pin-latest-**once** at create time then frozen (e.g. `journeys.guide_id`), and forward-looking write
targets (which version to bind next).

**Deprecation never filters pinned reads.** Multiple versions run actively side by side — that is
the entire point of pinning. `rev.deprecation_events` gate browse/catalog and new-journey
attachment ONLY; a read that resolves a journey's pinned version (packs, briefs, slots, DONE
rollups, dispatch-ready gates, the computed-output producer) must NOT join `latest_deprecation`.
A re-ingest deprecating the pinned version would otherwise silently null the journey out — every
pre-re-ingest journey stalls (this shipped in `5eb5649f` and was reverted 2026-07-16). The pinned
walk lives in `assets/sqlc/shared/select/journey_pinned_packs.sqlc`; compose it, don't hand-roll.

**Append-only ⇒ Merkle.** A reference column alone is not enough. Because the model is append-only, the
only way to move a pin is to create a **new version**, which only happens when the **content hash
changes**. So an entity that references a Fluent message for display **must fold that Fluent message's
`content_hash` into its own content hash** (`hash_brief`/`hash_brief_group`/`hash_brief_pack`/
`hash_stage`/`hash_step` do this — ingest passes the Fluent `content_hash` where the `name` placeholder
used to go). Otherwise a Fluent-body edit leaves the entity hash unchanged → the entity is reused →
it stays pinned to the stale Fluent version and the edit silently doesn't propagate. (Known limitation:
Fluent `.description` *attributes* are not persisted as bodies, so brief/group/pack descriptions are
baked from the plan attribute — consistently in both INSERT and PATCH — and attribute-only edits do not
re-version. Only body text is reference-pinned.)

**Intentional non-localized placeholders** (document, don't reference-wire): `value_slots.title`
(label comes from the input's `label_fluent_message_id`), and `tasks`/`playbooks` descriptions
(`TaskDef`/`PlaybookDef` carry no description and there is no per-entity Fluent display message). These
store `name` as a placeholder — a value-of-its-own, not a copy of another versioned object — so they are
NOT the anti-pattern.

## Performance Considerations
- **Batch operations** — Always use set-wise inserts with UNNEST
- **Minimize indexes** — Only create for actual query patterns
- **Explicit FK indexes** — PostgreSQL doesn't auto-index FKs
- **Partition large tables** — Use RANGE/LIST/HASH for >100M rows
- **Use COPY for bulk** — Faster than individual INSERTs

## pgvector Embedding Tables

Guide content embeddings use the `pgvector` extension for vector similarity search. This introduces patterns that differ from standard entity tables.

### Key Differences from Standard Tables

1. **`pgvector` crate required** -- Add `pgvector = { version = "0.4", features = ["sqlx"] }` to Cargo.toml. Use `pgvector::Vector` for binding vector parameters.
2. **`sqlx::query_as()` may be needed** -- `query_file_as!` may not handle `vector(768)` parameters at compile time. Using runtime `sqlx::query_as()` is the accepted exception for vector operations.
3. **Delete-then-reinsert pattern** -- Embeddings are regenerated atomically: `DELETE WHERE guide_id = $1` (cascades to junction tables), then batch `INSERT`. This is NOT an UPDATE -- it is a delete + append.
4. **Junction tables for tags and locales** -- Mirror the `rev.guide_block_tags` pattern. Tags are copied from the source block. Locales are derived from the guide's `authoring_locale` in `rev.guide_path_details`.
5. **Locale references** -- Use the existing `rev.fluent_message_locales` table (not a new `rev.locales` table). Locale tags are auto-populated by Fluent message ingestion.

### Embeddable Block Types

Only these directive types produce embeddings: `prose`, `stage`, `step`, `block`, `ref`. Skip: `nushell`, `brief_pack`, `inference_profile`.

### Vector Dimensions

The default embedding dimension is **768** (ModernBERT with Matryoshka truncation). The DDL column type is `vector(768)`. If the embedding model changes, a new migration must alter the column width and all existing embeddings must be regenerated.

See `docs/plans/pg-vector-phase2-implementation.md` for the full implementation specification.

**SQL sources:** `assets/sqlc/guide_content_embeddings/` (7 `.sqlc` files: insert, delete, 3 search variants, 2 junction inserts) → generated to `assets/.sql/guide_content_embeddings/`
