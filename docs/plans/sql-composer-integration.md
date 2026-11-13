# sql-composer Integration into dapple-data

**Status:** Draft
**Created:** 2026-02-24
**Updated:** 2026-03-08
**Source:** `~/src/sql-composer` → `~/src/dapple/dapple-prompt/crates/dapple-data`

---

## Context

dapple-data has ~320 SQL files across 16 versioned entities. Each entity has 4-6 query files (insert, select, select_by_object_ids, select_by_path_ids, patch, deprecate) that share 70-85% identical CTE infrastructure — path creation, versioning, deprecation events, JOIN chains. When agents work with these queries they must parse through ~100 lines of boilerplate to find the ~15 entity-specific lines. sql-composer's composition model lets agents reason about queries as composable pieces: "this INSERT uses the standard versioning scaffold + these entity-specific columns."

---

## Architecture

### Pipeline

```
sqlc/*.sqlc sources  →  cargo sqlc compose  →  .sql/*.sql output  →  sqlx::query_file_as!()
  (authored,              (recursive scan,       (generated,           (compile-time verified,
   :compose/:bind)         compose resolution)    hidden dir,           alphabetical param order)
                                                  committed)
```

- sql-composer is a **pre-processor** that runs before `cargo build`
- Source files use `.sqlc` extension; generated output uses `.sql`. The target directory defaults to `.sql/` (hidden) so it's clearly generated and won't be confused with editable source
- `cargo sqlc compose` recursively scans the source directory, composes every `.sqlc` file, and mirrors the subdirectory structure into the target. The source directory is automatically a search path for `:compose()` resolution
- Generated `.sql` files in `.sql/` are committed to git (so SQLX_OFFLINE builds work without the composer)
- Rust query functions must pass `query_file_as!()` parameters in **alphabetical order** of bind names (see Bind Parameter Ordering below)
- No runtime overhead — composition happens at dev time only

### Directory Layout

```
crates/dapple-data/assets/
  sqlc/                             # Source of truth (authored, contains :compose/:bind directives)
    shared/                         # Reusable complete SQL statements (composed into entity queries via :compose(), also valid standalone)
      insert_object_paths.sqlc
      insert_versioned_objects.sqlc
      numbered_path_details.sqlc
      patch_new_versions.sqlc
      deprecate_latest_versions.sqlc
      deprecate_events.sqlc         # INSERT-only (no UPDATE — deprecation derived at query time)
    tasks/
      insert_tasks.sqlc
      patch_tasks.sqlc
      deprecate_tasks.sqlc
      select_tasks.sqlc             # Pass-through (no composition yet, Phase 2)
      select_tasks_by_ids.sqlc
      ...
    steps/                          # Same pattern per entity
      ...
  .sql/                             # Generated output (hidden dir, committed, consumed by sqlx)
    shared/                         # Mirrored from sqlc/shared/ — complete standalone statements
      insert_object_paths.sql
      ...
    tasks/                          # Mirrored from sqlc/tasks/
      insert_tasks.sql
      ...
```

Source files use `.sqlc` in `sqlc/`; generated output uses `.sql` in `.sql/` (hidden directory). The hidden directory makes it obvious that `.sql/` is derived — you edit `sqlc/`, not `.sql/`. The current `assets/sql/` directory gets renamed to `assets/.sql/` and all `query_file_as!()` paths updated accordingly.

Every `.sqlc` file under `sqlc/` — including those in `shared/` — is a complete, valid SQL statement. The `shared/` directory contains reusable statements that entity queries compose via `:compose()`, but they are also valid standalone and get composed into their own `.sql` output. `cargo sqlc compose --source sqlc/ --target .sql/` recursively walks all subdirectories, composes each `.sqlc` file, and mirrors the directory structure into `.sql/`. The source directory is automatically a search path, so `:compose(shared/insert_object_paths.sqlc)` resolves relative to the source root. SELECT queries start as pass-through `.sqlc` files (plain SQL, no `:compose` directives) and gain composition in Phase 2.

### Bind Parameter Ordering

sql-composer assigns `$N` placeholder indices by **alphabetically sorting** all `:bind()` names in the composed query. This differs from the current hand-written SQL where parameters follow document order.

**Convention:** All Rust `query_file_as!()` calls must pass parameters in alphabetical order of their bind names.

Example for `insert_tasks.sqlc` (in `sqlc/tasks/`) with binds `names`, `slugs`, `descriptions`, `functions`, `function_inputs`, `content_hashes`:

| Bind name | $N (alphabetical) |
|-----------|-------------------|
| content_hashes | $1 |
| descriptions | $2 |
| function_inputs | $3 |
| functions | $4 |
| names | $5 |
| slugs | $6 |

The Rust function becomes:
```rust
sqlx::query_file_as!(
    InsertedTaskModel,
    "assets/.sql/tasks/insert_tasks.sql",
    &args.content_hashes,   // $1
    &args.descriptions,     // $2
    &args.function_inputs,  // $3
    &args.functions,        // $4
    &args.names,            // $5
    &args.slugs,            // $6
)
```

For composed queries where `:compose()` includes introduce additional binds (e.g., deprecation's `deprecated_by_user_id` and `to_level_id`), ALL bind names across the composed query are merged and sorted alphabetically. The Rust function must pass parameters in that merged alphabetical order.

---

## Phase 1: CTE Infrastructure Composition

Focus on INSERT, PATCH, and DEPRECATE where shared statements reference fixed tables (`rev.object_paths`, `rev.versioned_objects`) and need no table-name parametrization. No changes to sql-composer's core composition engine needed.

### Shared Statements

**`shared/insert_object_paths.sqlc`** — Creates paths + numbers them (identical across all 16 entities):
```sql
new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT (SELECT id FROM entity_type)
    FROM args
    ORDER BY args.ord
    RETURNING id
),
numbered_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),
```

**`shared/insert_versioned_objects.sqlc`** — Creates versions + numbers them:
```sql
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT np.id, args.content_hash
    FROM numbered_paths np
    JOIN args ON args.ord = np.ord
    ORDER BY np.id
    RETURNING id, path_id, content_hash, created_at
),
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),
```

**`shared/numbered_path_details.sqlc`**:
```sql
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
),
```

**`shared/patch_new_versions.sqlc`** — PATCH version creation with conflict handling:
```sql
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT paths.path_id, args.content_hash
    FROM args
    JOIN paths ON paths.ord = args.ord
    ORDER BY paths.ord
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),
```

**`shared/deprecate_latest_versions.sqlc`** — Resolves latest deprecation level and finds latest version per path:
```sql
latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),
latest_versions AS (
    SELECT DISTINCT ON (op.id)
        op.id AS path_id,
        vo.id AS versioned_object_id,
        ld.to_level_id AS from_level_id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    ORDER BY op.id, vo.created_at DESC
),
resolved AS (
    SELECT lv.versioned_object_id, lv.from_level_id, args.reason
    FROM args
    JOIN latest_versions lv ON lv.path_id = args.path_id
),
```

**`shared/deprecate_events.sqlc`** — Insert deprecation event (INSERT-only, no UPDATE):
```sql
events AS (
    INSERT INTO rev.deprecation_events (
        versioned_object_id, from_level_id, to_level_id, reason, deprecated_by_user_id
    )
    SELECT
        resolved.versioned_object_id,
        resolved.from_level_id,
        :bind(to_level_id)::SMALLINT,
        resolved.reason,
        :bind(deprecated_by_user_id)::UUID
    FROM resolved
    RETURNING *
),
```

Migration #19 removed `deprecation_level_id` from `rev.versioned_objects`. Deprecation level is now derived at query time via a `latest_deprecation` CTE using `DISTINCT ON (versioned_object_id)` from `rev.deprecation_events`, joined with `LEFT JOIN`. No UPDATE is needed. This same `latest_deprecation` CTE pattern is also used in SELECT queries to display deprecation status.

Note: `:bind()` parameters from composed statements are merged with the parent template's binds. sql-composer assigns `$N` indices by alphabetically sorting ALL bind names across the composed query. For example, a deprecate query with parent binds `path_ids`, `reasons` and composed binds `deprecated_by_user_id`, `to_level_id` produces: `deprecated_by_user_id` → `$1`, `path_ids` → `$2`, `reasons` → `$3`, `to_level_id` → `$4`.

### Example: Composed INSERT

**`tasks/insert_tasks.sqlc`**:
```sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        :bind(names)::TEXT[],
        :bind(slugs)::TEXT[],
        :bind(descriptions)::TEXT[],
        :bind(functions)::TEXT[],
        :bind(function_inputs)::JSONB[],
        :bind(content_hashes)::TEXT[]
    ) WITH ORDINALITY AS o(name, slug, description, function, function_input, content_hash, ord)
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'task'
),
:compose(shared/insert_object_paths.sqlc)
path_details AS (
    INSERT INTO rev.task_path_details (path_id, name, slug)
    SELECT np.id, args.name, args.slug
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, name, slug
),
:compose(shared/numbered_path_details.sqlc)
:compose(shared/insert_versioned_objects.sqlc)
new_tasks AS (
    INSERT INTO rev.tasks (versioned_object_id, description, function, function_input)
    SELECT nuv.id, args.description, args.function, args.function_input
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, function, function_input, created_at
),
numbered_tasks AS (
    SELECT nt.*, row_number() OVER (ORDER BY nt.id) AS ord FROM new_tasks nt
)
SELECT
    nut.id, nut.versioned_object_id, nuv.path_id,
    npd.name, npd.slug, nuv.content_hash,
    nut.description, nut.function, nut.function_input, nut.created_at
FROM numbered_tasks nut
JOIN numbered_versions nuv ON nuv.ord = nut.ord
JOIN numbered_paths nup ON nup.ord = nut.ord
JOIN numbered_path_details npd ON npd.ord = nut.ord
ORDER BY nut.id;
```

**What agents see:** "standard versioning scaffold (3 compose directives) + task-specific path_details + task-specific entity INSERT + task-specific SELECT." The 30+ lines of infrastructure CTEs collapse to 3 compose lines.

### Example: Composed DEPRECATE

**`tasks/deprecate_tasks.sqlc`**:
```sql
WITH args AS (
    SELECT *
    FROM UNNEST(
        :bind(path_ids)::UUID[],
        :bind(reasons)::TEXT[]
    ) WITH ORDINALITY AS o(path_id, reason, ord)
),
:compose(shared/deprecate_latest_versions.sqlc)
:compose(shared/deprecate_events.sqlc)
SELECT
    resolved.versioned_object_id,
    vo.path_id,
    tpd.name,
    tpd.slug,
    vo.content_hash,
    events.to_level_id AS deprecation_level_id,
    t.description,
    t.function,
    t.function_input,
    t.created_at
FROM resolved
JOIN events ON events.versioned_object_id = resolved.versioned_object_id
JOIN rev.versioned_objects vo ON vo.id = resolved.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.task_path_details tpd ON tpd.path_id = op.id
JOIN rev.tasks t ON t.versioned_object_id = vo.id
ORDER BY t.id;
```

**What agents see:** "standard deprecation scaffold (2 compose directives) + task-specific SELECT." The ~35 lines of deprecation infrastructure collapse to 2 compose lines.

### Impact Estimate

| Pattern | Entities | Shared lines/entity | Total deduplication |
|---------|----------|---------------------|---------------------|
| INSERT versioned | 16 | ~30 | ~480 lines |
| PATCH | 12 | ~25 | ~300 lines |
| DEPRECATE | 15 | ~35 | ~525 lines |
| **Total** | | | **~1,305 lines** |

Plus the cognitive benefit: agents see composition intent instead of scanning boilerplate.

---

## Phase 2: SELECT Body Composition (requires sql-composer enhancement)

SELECT queries have the highest repetition but need table-name parametrization. This requires adding a `:literal(name)` macro to sql-composer that emits compile-time text substitution (not a `$N` bind parameter).

### `:literal()` semantics

```sql
-- :literal(entity_table) emits literal text, e.g. "tasks"
-- :bind(ids) emits $N placeholder
SELECT e.id
FROM rev.:literal(entity_table) e
WHERE e.id = ANY(:bind(ids)::UUID[])
```

Composed with `literals = { "entity_table": "tasks" }`, this generates:
```sql
SELECT e.id FROM rev.tasks e WHERE e.id = ANY($1::UUID[])
```

This is a compile-time-only mechanism — never used with runtime inputs. The literal values come from a manifest file or CLI arguments, not from query parameters.

### Shared SELECT fragments with `:literal()`

Would allow extracting the versioning JOIN chain (entity → versioned_objects → object_paths → path_details → deprecation_levels) that repeats 4-6 times per entity across select/select_by_ids/select_by_path_ids/deprecate queries.

**Deferred until Phase 1 is stable and proven.**

---

## Changes Needed in sql-composer

### Phase 1 (required)

| Change | File | Description |
|--------|------|-------------|
| Recursive directory scanning | `cargo-sqlc/src/main.rs` | Walk `--source` subdirectories recursively, compose every `.sqlc` file, mirror directory structure into `--target` |
| `--verify` mode | `cargo-sqlc/src/main.rs` | Compose to memory, diff against existing `--target` files, exit 1 on mismatch (for CI). Analogous to `cargo fmt -- --check` |
| Default target `.sql/` | `cargo-sqlc/src/main.rs` | Change `SQLC_TARGET_DIR` default from `sql` to `.sql`. Hidden directory makes it obvious that output is generated |

No `--search-path` flag needed — the source directory is already added as a search path (line 100 of current `main.rs`), and all `:compose()` references resolve relative to the source root. No `--exclude` flag needed — every `.sqlc` file under source is a complete, valid statement and gets composed into output.

### Phase 2 (deferred)

| Change | File | Description |
|--------|------|-------------|
| `:literal(name)` element | `types.rs`, `parser/mod.rs`, `composer.rs` | Compile-time text substitution element type |
| Literal substitution API | `composer.rs` | `compose_with_literals(HashMap<String, String>)` method |
| Dynamic compose paths | `composer.rs` | `:compose(:literal(x)_suffix.sqlc)` resolution |

---

## Migration Strategy

Sequential plan — each step builds on the previous.

#### Step 1: File issue on sql-composer
File a GitHub issue requesting recursive directory scanning and `--verify` mode for `cargo sqlc compose`. See `docs/plans/sql-composer-recursive-compose-issue.md` for the full issue text.

#### Step 2: Implement cargo-sqlc enhancements locally
Develop the features in `~/src/sql-composer`. Point dapple-prompt at the local sql-composer via path dependency in workspace `Cargo.toml`:
```toml
[workspace.dependencies]
sql-composer = { path = "../../sql-composer/crates/sql-composer" }
```

#### Step 3: Create shared statements
Create `assets/sqlc/shared/` with the 6 shared statement files.

#### Step 4: Pilot with `tasks`
- Copy `assets/.sql/tasks/*.sql` → `assets/sqlc/tasks/*.sqlc`
- Refactor `insert_tasks.sqlc`, `patch_tasks.sqlc`, `deprecate_tasks.sqlc` to compose shared statements
- SELECT files remain pass-through
- Run `cargo sqlc compose --source assets/sqlc/ --target assets/.sql/`
- Diff generated `assets/.sql/tasks/*.sql` — **will NOT match byte-for-byte** due to alphabetical bind reordering
- Update Rust `query_file_as!()` calls in `src/query/tasks.rs` to pass params in alphabetical order
- Run `cargo sqlx prepare --workspace` and `cargo test --workspace`

#### Step 5: Migrate remaining entities
One entity at a time. Order by complexity:
1. **Simple** (few entity-specific fields): categories, hats, prompts
2. **Medium** (1-2 dependents): briefs, tasks, stages, directives, value_slots
3. **Complex** (multiple dependents): playbooks, brief_groups, brief_packs, guides, steps
4. **Special**: assets (asset_kind_id lookup), inference_profiles, fluent_messages

For each entity: create `.sqlc` files → compose → update Rust param order → `cargo sqlx prepare` → test.

#### Step 6: Publish sql-composer 0.0.2
Once all entities are migrated and tests pass with the local path dependency:
- Publish sql-composer 0.0.2 to crates.io with the cargo-sqlc enhancements
- Switch dapple-prompt from path dependency to `sql-composer = "0.0.2"`
- Re-run full test suite to confirm

#### Step 7: CI check
Add `cargo sqlc compose --source assets/sqlc/ --target assets/.sql/ --verify` to CI.

---

## Trade-offs

### Benefits
- ~1,305 lines of SQL deduplication across INSERT/PATCH/DEPRECATE
- Bug fixes to shared infrastructure propagate to all entities via recompose
- Agents reason about composition intent, not boilerplate
- Adding a new versioned entity requires only the entity-specific parts
- Existing sqlx workflow completely preserved

### Risks
- **Build complexity**: Adds a pre-processing step. Mitigated by committing generated files — most builds never run the composer.
- **Debugging**: sqlx errors reference generated `.sql` line numbers. Mitigated by keeping generated files readable and committed.
- **Bind parameter reordering**: sql-composer sorts bind parameters alphabetically, requiring Rust query functions to be updated. Mitigated by systematic per-entity migration with test verification.
- **CTE comma conventions**: Shared statements used as CTE clauses via `:compose()` must consistently include trailing commas. Needs a clear convention documented in the shared directory.

### Non-risks
- **sqlx compatibility**: Generated `.sql` is identical to current hand-written files
- **No runtime overhead**: Composition is dev-time only
- **Minimal Rust code changes**: Only parameter order in `query_file_as!()` calls changes; paths unchanged
- **No breaking changes**: Incremental adoption, one entity at a time

---

## Verification

1. **Functional equivalence**: Each migrated entity's generated `.sql` produces identical query results (bind parameter order will differ from hand-written SQL)
2. **sqlx prepare**: `cargo sqlx prepare --workspace` succeeds after each entity migration
3. **Full test suite**: `cargo test --workspace` passes
4. **CI gate**: `cargo sqlc compose --verify` prevents drift between `sqlc/` sources and `.sql/` output

---

## Decisions Made

- **File extension:** `.sqlc` for source templates (not `.tql`)
- **All SQL becomes `.sqlc`:** SELECT queries included as pass-through files; `.sql/` is entirely generated
- **Target directory:** `.sql/` (hidden) — prevents confusion with editable source. Current `assets/sql/` renamed to `assets/.sql/`; all `query_file_as!()` paths updated
- **No generated headers:** Generated `.sql` stays identical to current files (cleaner diffs during migration)
- **Bind parameter order:** Alphabetical (sql-composer default). Rust query functions updated to match.
- **Dependency:** Local path dep during development → publish 0.0.2 → lock to crates.io 0.0.2
- **No UPDATE in deprecation:** Shared deprecation statement is INSERT-only; deprecation level derived at query time
- **No fragments — all complete statements:** Every `.sqlc` file is a complete, valid SQL statement. `shared/` files are reusable statements composed into entity queries, but are also valid standalone and produce their own output in `.sql/`
- **Native recursive compose:** `cargo sqlc compose` handles recursive directory scanning and verify mode natively — no wrapper script, no exclusion flags needed

## Open Questions (Resolved)

1. ~~Naming convention for shared files~~ → No underscore prefix; `shared/insert_object_paths.sqlc` (decided)
2. ~~Should compose be wrapped in nushell script~~ → No; `cargo sqlc compose` handles recursion natively. The existing `--skip-prepare` flag already controls whether `cargo sqlx prepare` runs after compose (decided)
3. ~~How should cargo-sqlc discover directories~~ → Recursive walk of `--source`; source dir is already a search path for `:compose()` resolution (decided)
4. ~~Fragment exclusion mechanism~~ → Not needed; every `.sqlc` file is a complete statement and gets composed into output (decided)
