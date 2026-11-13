# Agent Instructions — Lifecycle Tests

Quick reference for agents working with integration tests. See [README.md](./README.md) for full patterns and dependency graphs.

## Quick Checklist: Adding a New Entity Trial

1. [ ] Create `consumer/{entity}.rs` implementing `Endpoint` trait
2. [ ] Add to `ScenarioContext` in `consumer/mod.rs`:
   ```rust
   pub {entity}_batches: Vec<Vec<{Entity}>>,
   pub {entity}_named_batches: HashMap<String, BatchHandle<{Entity}>>,
   ```
3. [ ] Add registration method: `register_{entity}_batch()`
4. [ ] Add random selection: `random_{entity}_from_batches()`
5. [ ] Add trial to `user_lifecycle.rs`:
   ```rust
   let {entity}_trial = async_trial(&rt, "create_{entity}s", { ... });
   ```
6. [ ] Add trial to the `run()` call at the correct dependency level
7. [ ] If the entity has child objects, implement the mixed child count pattern (see below)

## Test Harness: `harness = false`

The `user_lifecycle.rs` test uses `harness = false` with `fn main()`:

```toml
[[test]]
name = "user-lifecycle"
path = "tests/user_lifecycle.rs"
harness = false
```

- **Do NOT** add `#[test]` or `#[tokio::test]` annotations — they are incompatible with `harness = false`
- Trials are registered as `Trial` values via `async_trial()` and passed to `libtest_mimic::run()`
- Trial filtering works via `cargo test --test user-lifecycle -- <filter>`

## Dependency Waiting Pattern

Keep it simple: just check if the named batch key exists.

```rust
// Wait for dependencies
loop {
    {
        let ctx = scenario.lock().unwrap();
        if ctx.{dependency}_named_batches.contains_key("{dependency}-init") {
            break;
        }
    }
    if start.elapsed() > timeout {
        panic!("timeout waiting for {dependency}-init");
    }
    sleep(Duration::from_millis(50)).await;
}
```

**Don't over-complicate wait conditions** — extra checks can cause false negatives.

## Mixed Child Count Pattern

Consumers with child objects override `fake_create_set_schema` to produce mixed batches:
- First object: **1 child** (tests single-child path)
- Subsequent objects: **3 children** (tests batch path)

This ensures both code paths are exercised in a single POST call.

```rust
fn fake_create_schema_with_child_count(&self, count: usize) -> Self::CreateSchema {
    // Pull `count` child object_ids from ScenarioContext
    // ...
}

fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
    let fakes = (size).fake::<usize>();
    CreateSetSchema {
        objs: (0..fakes)
            .map(|i| {
                let count = if i == 0 { 1 } else { 3 };
                self.fake_create_schema_with_child_count(count)
            })
            .collect(),
    }
}
```

Consumers that use this pattern: `brief_groups`, `brief_packs`, `briefs`, `guides`, `journeys`, `steps`, `tasks`, `fluent_messages`, `prompts`.

## Batch Naming Convention

Use `"{entity}-init"` for the first batch:
- `"tasks-init"`
- `"briefs-init"`
- `"brief-groups-init"`
- `"inference-profiles-init"`
- `"prompts-init"`
- `"directives-init"`
- `"guides-init"`

**Note:** Some legacy batches use `"init-{entity}"` pattern (e.g., `"init-playbooks"`, `"init-journeys"`, `"init-fluent-messages"`). Use `"{entity}-init"` for new entities.

## Verification Trials

Not all trials create new entity batches. Some are read-only verification trials:

| Trial | Depends On | What It Verifies |
|-------|-----------|-----------------|
| `verify_journeys_recent` | `init-journeys` | GET `/journeys/recent` returns created journeys |
| `verify_tasks_by_slugs` | `tasks-init` | GET tasks by slug lookup works |
| `verify_brief_units` | `briefs-init` | Inline brief units are accessible via GET |
| `deprecate_briefs` | `briefs-init` | Deprecation flow (create → deprecate → verify) |
| `verify_guides_by_slugs` | `guides-init` | GET guides by slug lookup works |
| `deprecate_guides` | `guides-init` | Deprecation flow (create → deprecate → verify) |
| `attempts_flow` | `dispatches-init` | Attempt creation → event posting → status derivation |

## Common Mistakes

### Trial Design
- **Waiting for non-existent batches** — Verify the batch name matches exactly
- **Complex wait conditions** — Just check for key existence
- **Shared state issues** — Be careful with Arc<Mutex> and libtest_mimic
- **Adding `#[test]` to lifecycle** — The test uses `harness = false` with `fn main()`; `#[test]` annotations will not work

### Consumer Implementation
- **Fake data collisions** — Use unique names (e.g., include UUID or timestamp)
- **Missing dependencies** — Pull required object_ids from ScenarioContext
- **Fixed child counts** — Use the mixed child count pattern (`if i == 0 { 1 } else { 3 }`) to exercise both single-child and multi-child code paths

### SQL/sqlx
- **Nullable columns** — Use `?` suffix in SQL: `as "column?"` for LEFT JOIN columns
- **Stale cache** — Delete `.sqlx/` and run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`)

## Test Consumer Pattern

```rust
impl Endpoint for {Entity}Endpoint {
    fn fake_create_schema(&self) -> Self::CreateSchema {
        // Pull dependencies from ScenarioContext
        let dep_ids: Vec<Uuid> = {
            let ctx = self.context.lock().unwrap();
            ctx.{dep}_batches.iter()
                .flat_map(|batch| batch.iter().map(|x| x.object_id))
                .take(2)
                .collect()
        };
        Create{Entity}Schema {
            name: format!("test-{}", Uuid::new_v4()),
            {dep}_object_ids: dep_ids,
        }
    }
}
```

## Common Test Errors

| Error | Cause | Fix |
|-------|-------|-----|
| `timeout waiting for X` | Dependency trial failed or wrong batch name | Check trial order and names |
| `ColumnDecode { UnexpectedNullError }` | LEFT JOIN column not nullable | Add `?` suffix in SQL |
| `409 Conflict` | Duplicate unique constraint | Use unique fake data |
| `no X available in any batch` | Missing dependency registration | Check ScenarioContext has batch |

## Versioned Object Rules

1. **Atomic creation** — Path + versioned object in single INSERT via CTE
2. **No separate path trials** — Object trials create paths automatically
3. **No path endpoints** — Don't create `/api/{entity}-paths/` endpoints

## Running Tests

```bash
# All lifecycle tests
cargo test --test user-lifecycle --package dapple-prompt-api

# With logging
RUST_LOG=info cargo test --test user-lifecycle --package dapple-prompt-api

# Specific trial (substring match)
cargo test --test user-lifecycle -- create_categories

# All verification trials
cargo test --test user-lifecycle -- verify
```

## When Stuck

1. Check `consumer/briefs.rs`, `consumer/brief_groups.rs` for patterns
2. Verify batch names match exactly between registration and waiting
3. Run with `RUST_LOG=debug` for detailed output
4. Delete `.sqlx/` and run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`)
