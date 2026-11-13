# GitHub Issue: Recursive directory scanning, verify mode, and `.sql/` target default for `cargo sqlc compose`

**Repository:** soulstompp/sql-composer
**Labels:** enhancement, cargo-sqlc

---

## Title

`cargo sqlc compose`: recursive directory scanning, `--verify` mode, and `.sql/` target default

## Body

### Problem

`cargo sqlc compose` currently scans a single flat directory of `.sqlc` files. Real-world projects organize SQL templates into subdirectories — one per database entity — with shared reusable statements in sibling directories:

```
assets/sqlc/
  shared/               # Reusable complete SQL statements (composed into entity queries, also valid standalone)
    insert_object_paths.sqlc
    insert_versioned_objects.sqlc
    deprecate_events.sqlc
    ...
  tasks/                # Entity: tasks
    insert_tasks.sqlc   # Uses :compose(shared/insert_object_paths.sqlc)
    patch_tasks.sqlc
    deprecate_tasks.sqlc
    select_tasks.sqlc   # Pass-through (no :compose)
  steps/                # Entity: steps
    insert_steps.sqlc
    ...
  briefs/               # ...16 entity directories total
    ...
```

Every `.sqlc` file — including those in `shared/` — is a complete, valid SQL statement. The `shared/` files happen to be reused via `:compose()` in other queries, but they're also perfectly valid to compose on their own.

The desired output mirrors the full subdirectory structure into a hidden `.sql/` directory (making it obvious the output is generated, not editable):

```
assets/.sql/
  shared/
    insert_object_paths.sql
    insert_versioned_objects.sql
    deprecate_events.sql
    ...
  tasks/
    insert_tasks.sql
    patch_tasks.sql
    ...
  steps/
    insert_steps.sql
    ...
```

Today this requires calling `cargo sqlc compose` once per subdirectory from an external wrapper script, which defeats the purpose of having a proper CLI tool.

### Proposed Changes

#### 1. Recursive directory scanning

`cargo sqlc compose --source assets/sqlc/ --target assets/.sql/` should:

- Recursively walk all subdirectories under `--source`
- For each `.sqlc` file found, compose it and write the `.sql` output to the corresponding relative path under `--target`
- Create subdirectories in `--target` as needed

Example: `assets/sqlc/tasks/insert_tasks.sqlc` → `assets/.sql/tasks/insert_tasks.sql`

The source directory is already added as a search path (line 100 of current `main.rs`), so `:compose(shared/insert_object_paths.sqlc)` inside `tasks/insert_tasks.sqlc` resolves correctly — the source root is the search root.

#### 2. `--verify` mode

```
cargo sqlc compose --source assets/sqlc/ --target assets/.sql/ --verify
```

- Compose all templates to memory
- Diff each composed result against the existing file in `--target`
- If any file differs (or is missing from `--target`), print the diff and exit with code 1
- If all files match, exit 0

This is for CI: the generated `.sql` files are committed to git, and `--verify` ensures they stay in sync with the `.sqlc` sources. Analogous to `cargo fmt -- --check`.

#### 3. Change `SQLC_TARGET_DIR` default to `.sql`

The current default target directory is `sql`. Change it to `.sql` (hidden directory). This makes the generated output visually distinct from source — people won't accidentally edit files in a hidden directory thinking they're the source of truth.

```rust
// Current
#[arg(long, env = "SQLC_TARGET_DIR", default_value = "sql")]
target: PathBuf,

// Proposed
#[arg(long, env = "SQLC_TARGET_DIR", default_value = ".sql")]
target: PathBuf,
```

### Current Behavior

```rust
// main.rs lines 104-122 — flat read_dir, no recursion
for entry in std::fs::read_dir(source_dir)? {
    let entry = entry?;
    let path = entry.path();
    if path.extension().is_some_and(|ext| ext == "sqlc") {
        // compose and write...
    }
}
```

Subdirectories are silently skipped. Only `.sqlc` files directly in the `--source` directory are composed.

### Proposed CLI

```
cargo sqlc compose \
  --source <dir>            # Root of .sqlc templates (now recursive), default: "sqlc"
  --target <dir>            # Root of .sql output (mirrors source structure), default: ".sql" (was "sql")
  --dialect postgres        # (existing, unchanged)
  --skip-prepare            # (existing, unchanged)
  --verify                  # New: diff-only mode for CI
```

### Use Case

We have ~320 SQL files across 16 database entities. Each entity has INSERT, PATCH, DEPRECATE, and SELECT queries that share 70-85% identical CTE infrastructure. sql-composer lets us extract shared CTEs into reusable statements and compose them, but the CLI needs to handle the real directory layout natively rather than requiring a wrapper script to iterate subdirectories.

### Implementation Notes

- The recursive walk can use `walkdir` or recursive `std::fs::read_dir`
- Relative path from source root determines output path: `path.strip_prefix(source_root)` → same relative path under target
- No exclusion logic needed — every `.sqlc` file is a complete statement and should produce output
- Verify mode: `compose()` as normal but compare against `std::fs::read()` of the existing target file instead of writing
