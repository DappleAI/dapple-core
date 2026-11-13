# Lifecycle Tests

Integration tests for dapple-prompt-api that exercise the full API flow: HTTP → handlers → queries → PostgreSQL.

## Overview

The lifecycle tests validate that:
1. POST endpoints create objects correctly
2. GET endpoints return the same data that was created
3. Object relationships (shared/owned dependents) work correctly
4. Versioned objects are created atomically with their paths
5. Verification trials confirm secondary query paths (e.g., recent, by_slugs)
6. Deprecation flows work correctly (soft-delete via deprecation)

## Key Files

```
tests/
├── README.md                    # This file
├── AGENTS.md                    # Quick-reference for agents
├── user_lifecycle.rs            # Main test runner with trial definitions (harness = false)
└── consumer/
    ├── mod.rs                   # ScenarioContext, Endpoint trait, test utilities
    ├── assets.rs                # Asset endpoint consumer
    ├── brief_groups.rs          # BriefGroup endpoint consumer
    ├── brief_packs.rs           # BriefPack endpoint consumer
    ├── brief_units.rs           # BriefUnit verification consumer
    ├── briefs.rs                # Brief endpoint consumer
    ├── categories.rs            # Category endpoint consumer
    ├── directives.rs            # Directive endpoint consumer
    ├── dispatch_intent_bindings.rs  # DispatchIntentBinding endpoint consumer
    ├── dispatches.rs            # Dispatch endpoint consumer
    ├── error.rs                 # Error types for test consumers
    ├── fluent_messages.rs       # FluentMessage endpoint consumer
    ├── hats.rs                  # Hat endpoint consumer
    ├── inference_profiles.rs    # InferenceProfile endpoint consumer
    ├── journeys.rs              # Journey endpoint consumer
    ├── playbooks.rs             # Playbook endpoint consumer
    ├── prompts.rs               # Prompt endpoint consumer
    ├── stages.rs                # Stage endpoint consumer
    ├── steps.rs                 # Step endpoint consumer
    ├── tasks.rs                 # Task endpoint consumer
    └── value_slots.rs           # ValueSlot endpoint consumer
```

## Core Concepts

### ScenarioContext

Shared test state that stores batches of created objects so later tests can reference them.

```rust
pub struct ScenarioContext {
    pub task_batches: Vec<Vec<Task>>,
    pub task_named_batches: HashMap<String, BatchHandle<Task>>,
    // ... other entity batches
}
```

- **Batches**: Vectors of created objects indexed by position
- **Named batches**: HashMap lookup for semantic access (e.g., `"tasks-init"`)
- **BatchHandle**: Lightweight reference to a batch by index

### Endpoint Trait

Each entity implements the `Endpoint` trait to define how to interact with its API:

```rust
pub trait Endpoint {
    type Object;           // The DTO returned by the API
    type CreateSchema;     // POST request body
    type CreateSetSchema;  // Batch POST request body
    type GetSchema;        // GET query parameters
    type GetSetSchema;     // Batch GET query parameters

    fn path(&self) -> &str;                    // API path segment
    fn fake_create_schema(&self) -> Self::CreateSchema;  // Generate fake data
    async fn post_object_set(...) -> Result<Vec<Self::Object>, ConsumerError>;
    // ... other methods
}
```

### Trial Pattern

Each test is a "trial" - an async function wrapped in `async_trial()` that:
1. Waits for any dependencies (shared dependents) to be created
2. Creates objects via POST
3. Registers the created objects in ScenarioContext
4. Validates the response

```rust
let tasks_trial = async_trial(&rt, "create_tasks", {
    move || async move {
        let ep = TaskEndpoint { base_url, ... };
        let created = ep.post_fake_object_set(2..5).await?;
        ctx.register_task_batch("tasks-init", created);
        Ok(())
    }
});
```

### Mixed Child Count Pattern

Consumers with child objects (associative dependents) override `fake_create_set_schema` to produce batches with varying child counts. This ensures both single-child and multi-child code paths are exercised in a single POST call.

- First object in the batch: **1 child** (tests single-child path)
- Subsequent objects: **3 children** (tests batch path)

```rust
impl BriefGroupEndpoint {
    fn fake_create_schema_with_child_count(&self, count: usize) -> CreateBriefGroupSchema {
        // Pull `count` brief object_ids from ScenarioContext
        // ...
    }
}

impl Endpoint for BriefGroupEndpoint {
    fn fake_create_schema(&self) -> Self::CreateSchema {
        self.fake_create_schema_with_child_count(2) // default for individual calls
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();
        CreateBriefGroupSetSchema {
            objs: (0..fakes)
                .map(|i| {
                    let count = if i == 0 { 1 } else { 3 };
                    self.fake_create_schema_with_child_count(count)
                })
                .collect(),
        }
    }
}
```

Consumers that use this pattern: `brief_groups`, `brief_packs`, `briefs`, `journeys`, `steps`, `tasks`, `fluent_messages`, `prompts`.

## Test Harness

### `harness = false` and `fn main()`

The `user_lifecycle.rs` test uses `harness = false` in `Cargo.toml`:

```toml
[[test]]
name = "user-lifecycle"
path = "tests/user_lifecycle.rs"
harness = false
```

This means:
- The test defines its own `fn main()` instead of using `#[test]` annotations
- **libtest_mimic** handles CLI argument parsing and test filtering
- Trials are registered as `Trial` values and passed to `libtest_mimic::run()`
- The standard `cargo test` `--` filter syntax works: `cargo test --test user-lifecycle -- create_categories`

**Do NOT** add `#[test]` or `#[tokio::test]` annotations to `user_lifecycle.rs` — they are incompatible with `harness = false`.

### Topological Ordering

Trials are ordered by dependency level in the `run()` call. libtest_mimic runs lower-indexed trials first, so dependencies are naturally satisfied:

```rust
let conclusion = run(&args, vec![
    // Level 0: No dependencies
    categories_trial,
    hats_trial,
    fluent_messages_trial,
    // ...
    // Level 1: Depend on Level 0
    briefs_trial,
    // Level 2: Depend on Level 1
    brief_groups_trial,
    prompts_trial,
    // ...and so on
]);
```

## Dependency Management

### Waiting for Dependencies

When a trial depends on objects created by another trial, use polling:

```rust
// Wait for tasks to be created
let start = Instant::now();
let timeout = Duration::from_secs(30);
loop {
    {
        let ctx = scenario.lock().unwrap();
        if ctx.task_named_batches.contains_key("tasks-init") {
            break;
        }
    }
    if start.elapsed() > timeout {
        panic!("timeout waiting for tasks-init batch");
    }
    sleep(Duration::from_millis(50)).await;
}
```

### Dependency Graph

```
                        fluent_messages (no deps)
                              │
categories  hats          prompts ──────── inference_profiles (no deps)
    │        │                │
    │        │            tasks ← prompts
    │        │                │
    └────────┼────── steps ───┘
             │          │
          stages        │               value_slots (no deps)  assets (no deps)
             │          │                     │
        playbooks ──────┘               briefs ← value_slots (for inline outputs)
             │                                │
         journeys ← brief_packs        brief_groups ← briefs
             │           ↑                    │
        dispatches   brief_packs ← brief_groups
             │
    dispatch_intent_bindings ← dispatches, value_slots
```

**Verification / Read-Only Trials** (depend on creation trials but don't produce new batches used downstream):
```
verify_journeys_recent    → journeys
verify_tasks_by_slugs     → tasks
verify_brief_units        → briefs (reads inline units)
deprecate_briefs          → briefs (creates + deprecates a brief)
attempts_flow             → dispatches (creates attempt + event, verifies status)
```

**Brief Hierarchy** (versioned objects with associative tables):
```
Brief (depends on: value_slots for inline outputs, versioned)
   ↓
BriefGroup (references Briefs via brief_group_briefs associative table)
   ↓
BriefPack (references BriefGroups via brief_pack_groups associative table)
```

**Prompt → Task Chain**:
```
FluentMessage (no deps, versioned)
   ↓
Prompt (references FluentMessages via prompt_fluent_messages associative table)
   ↓
Task (references Prompts via task_prompts)
```

## Versioned Objects (CRITICAL)

### Atomic Path + Object Creation

Versioned objects (ValueSlots, Assets, FluentMessages, Playbooks) MUST create their
path and versioned object atomically in a single INSERT using CTEs:

```sql
WITH paths AS (
    INSERT INTO rev.value_slot_paths (name, value_kind_id, created_at)
    SELECT kinds.name, kinds.kind_id, now()
    FROM kinds
    RETURNING *
),
objs AS (
    INSERT INTO rev.value_slots (value_slot_path_id, version_major, ...)
    SELECT paths.id, 0, 0, 1, ...
    FROM paths
    JOIN kinds ON kinds.name = paths.name
    RETURNING *
)
SELECT ... ORDER BY kinds.ord;
```

### What NOT To Do

- **DO NOT** create separate API endpoints for paths
- **DO NOT** create separate test trials for paths
- **DO NOT** require paths to exist before creating versioned objects
- **DO NOT** use `path_object_id` in CreateSchema - include path fields directly

### Correct Pattern

The `CreateValueSlotSchema` includes path fields:

```rust
pub struct CreateValueSlotSchema {
    pub name: String,           // Path field
    pub value_kind: String,     // Path field
    pub title: String,          // Versioned object field
    pub description: Option<String>,
    pub constraints: serde_json::Value,
}
```

The handler creates both atomically - no separate path creation needed.

## Associative Dependents Pattern

When a principal object references other principals via an associative (join) table, follow this pattern:

### Example: BriefGroup → Brief relationship

**Database Tables:**
- `rev.brief_groups` - Principal object table
- `rev.brief_group_briefs` - Associative table linking groups to briefs

**Insert Pattern:**
1. Insert the principal (BriefGroup) with atomic path creation
2. Look up internal IDs for referenced objects (Briefs) using `get_brief_internal_ids()`
3. Insert associative table entries with `insert_brief_group_briefs()`

**Handler Flow:**
```rust
// 1. Look up brief internal IDs from the request's object_ids
let brief_ids = get_brief_internal_ids(&pool, &brief_object_ids).await?;

// 2. Insert the group (creates path + versioned object atomically)
let inserted_groups = insert_brief_groups(&pool, args).await?;

// 3. Insert associative entries linking groups to briefs
insert_brief_group_briefs(&pool, group_id, brief_internal_ids, ordinals).await?;

// 4. Re-fetch to get complete object with joined briefs
let groups = select_brief_groups_by_object_ids(&pool, &object_ids).await?;
```

**Test Consumer:**
The `fake_create_schema()` method should pull object_ids from ScenarioContext:
```rust
fn fake_create_schema(&self) -> Self::CreateSchema {
    let brief_ids: Vec<Uuid> = {
        let ctx = self.context.lock().unwrap();
        ctx.brief_batches.iter()
            .flat_map(|batch| batch.iter().map(|b| b.object_id))
            .take(2)
            .collect()
    };
    CreateBriefGroupSchema { brief_object_ids: brief_ids, ... }
}
```

## sqlx Nullable Columns

When using LEFT JOINs in SQL, use the `?` suffix to tell sqlx the column is nullable:

```sql
SELECT
    dib.id,
    dib.dispatch_id,
    -- Nullable from LEFT JOIN - use "?" suffix
    ti.ordinal as "text_ordinal?",
    ti.value as "text_value?",
    ji.value as "json_value?"
FROM dispatch_intent_bindings dib
LEFT JOIN text_items ti ON ti.binding_id = dib.id
LEFT JOIN json_items ji ON ji.binding_id = dib.id
```

Without the `?` suffix, sqlx may fail with `ColumnDecode { index: "N", source: UnexpectedNullError }`.

## Naming Conventions

### Batch Names

Follow the pattern: `"{entity}-init"` for the first batch of each entity type:

- `"categories-init"`
- `"hats-init"`
- `"tasks-init"`
- `"steps-init"`
- `"stages-init"`
- `"init-playbooks"` (historical - prefer `{entity}-init` for new entities)
- `"init-journeys"` (historical)
- `"init-fluent-messages"` (historical)
- `"dispatches-init"`
- `"dispatch-intent-bindings-init"`
- `"value-slots-init"`
- `"assets-init"`
- `"briefs-init"`
- `"brief-groups-init"`
- `"brief-packs-init"`
- `"brief-units-init"`
- `"directives-init"`
- `"inference-profiles-init"`
- `"prompts-init"`

**Note**: Some existing batches use `"init-{entity}"` pattern for historical reasons. New entities should use `"{entity}-init"` for consistency.

### Consumer Files

Each entity has a consumer file in `consumer/`:

- File: `consumer/{entity}.rs`
- Struct: `{Entity}Endpoint`
- Implements: `Endpoint` trait

## Running Tests

```bash
# Run all lifecycle tests
cargo test --test user-lifecycle --package dapple-prompt-api

# Run with logging
RUST_LOG=info cargo test --test user-lifecycle --package dapple-prompt-api

# Run a specific trial by name filter
cargo test --test user-lifecycle --package dapple-prompt-api -- create_categories

# Run multiple matching trials
cargo test --test user-lifecycle --package dapple-prompt-api -- verify
```

Trial filtering uses libtest_mimic's built-in CLI argument parsing (enabled by `harness = false`). The filter is a substring match against trial names.

## Debugging Failures

1. **Check the error message**: Look for specific errors like `ColumnDecode`, `409 Conflict`
2. **Check batch registration**: Ensure batches are registered with correct names
3. **Check dependency order**: Ensure trials wait for their dependencies
4. **Check sqlx cache**: Delete `.sqlx/` and run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`)
5. **Check SQL column order**: Ensure model fields match SQL SELECT column order

### Common Errors

| Error | Cause | Fix |
|-------|-------|-----|
| `timeout waiting for X batch` | Dependency trial failed or batch name mismatch | Check trial execution order and batch names |
| `ColumnDecode { index: "N", source: UnexpectedNullError }` | LEFT JOIN column not marked nullable | Add `?` suffix in SQL: `as "column?"` |
| `409 Conflict "row already exists"` | Unique constraint violation | Ensure fake data generates unique values |
| `no X available in any batch` | Consumer can't find required objects | Check ScenarioContext has the batch registered |

## Adding a New Entity

1. Create `consumer/{entity}.rs` implementing `Endpoint` trait
2. Add batch fields to `ScenarioContext` in `consumer/mod.rs`:
   ```rust
   pub {entity}_batches: Vec<Vec<Entity>>,
   pub {entity}_named_batches: HashMap<String, BatchHandle<Entity>>,
   ```
3. Add `register_{entity}_batch` and `random_{entity}_from_*` methods
4. Add trial to `user_lifecycle.rs`:
   ```rust
   let {entity}_trial = async_trial(&rt, "create_{entity}s", { ... });
   ```
5. Add trial to the `run()` call at the correct dependency level
6. Run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`)

## References

- [dapple-data README](../../dapple-data/README.md) - Data layer concepts
- [CLAUDE.md](../../../CLAUDE.md) - Project conventions
- [libtest-mimic](https://docs.rs/libtest-mimic/) - Test framework used for trials
