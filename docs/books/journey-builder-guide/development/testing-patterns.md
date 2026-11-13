# Testing Patterns

> **Audience:** Backend Developers · Platform Engineers

Testing strategies for Playbooks, Tasks, and integration tests.

## Local Testing

### Test Individual Tasks

Run each Task script locally with sample JSON input:

```bash
# Set up test environment
export OUT_DIR=./tmp/out
mkdir -p $OUT_DIR

# Run with sample input
echo '{"name": "Test User", "email": "test@example.com"}' | nu collect-personal-data.nu

# Verify outputs
cat $OUT_DIR/artifact.json
cat $OUT_DIR/log.jsonl
```

**Verify each script writes:**
- `artifact.json` with `path` and `type` fields
- `log.jsonl` with structured log entries
- `error.json` on failure (with `message` and `code`)

### Test Task Chains

Simulate Step execution by running Tasks in order:

```bash
# Task 1
echo '{"name": "Test"}' | OUT_DIR=./tmp/t1 nu task1.nu

# Task 2 (reads from Task 1)
IN_ARTIFACT=./tmp/t1/artifact.json OUT_DIR=./tmp/t2 nu task2.nu

# Task 3 (reads from Task 2)
IN_ARTIFACT=./tmp/t2/artifact.json OUT_DIR=./tmp/t3 nu task3.nu
```

## Integration Smoke Test

Before registering a Playbook, run an end-to-end smoke test:

```nushell
#!/usr/bin/env nu

# smoke-test.nu - Run full pipeline locally

def main [input_file: string] {
    let base = "./tmp/smoke"
    rm -rf $base
    mkdir $base

    # Task 1
    print "Running collect-personal-data..."
    open $input_file | nu collect-personal-data.nu

    # Task 2
    print "Running format-resume..."
    IN_ARTIFACT=$"($base)/t1/artifact.json" OUT_DIR=$"($base)/t2" nu format-resume.nu

    # Verify
    let final = open $"($base)/t2/artifact.json"
    if ($final.path | path exists) {
        print $"Success! Final artifact: ($final.path)"
    } else {
        print "FAILED: Final artifact not found"
        exit 1
    }
}
```

## API Integration Tests

### Test Structure

```rust
// File: crates/dapple-prompt-api/tests/lifecycle_journeys.rs
use dapple_prompt_api::schema::journeys::*;

#[sqlx::test]
async fn test_journey_lifecycle(pool: PgPool) {
    let mut ctx = ScenarioContext::new();

    // 1. Create shared dependents first
    let playbooks = create_test_playbooks(&pool, 2).await;
    ctx.add_batch("playbooks-init", playbooks.clone());

    // 2. Create principals
    let journey_reqs = vec![
        CreateJourneySchema {
            name: "Journey 1".into(),
            playbook_id: playbooks[0].id,
        },
    ];

    let journeys = create_journeys(&pool, journey_reqs).await;
    ctx.add_batch("journeys-init", journeys.clone());

    // 3. Verify GET returns same data
    let fetched = get_journeys_by_ids(&pool,
        &journeys.iter().map(|j| j.id).collect::<Vec<_>>()
    ).await;

    assert_eq!(journeys.len(), fetched.len());
}
```

### ScenarioContext

Track batches with naming pattern `"{endpoint}-init"`:

```rust
pub struct ScenarioContext {
    pub journey_batches: Vec<Vec<Journey>>,
    pub journey_named_batches: HashMap<String, BatchHandle<Journey>>,
}

impl ScenarioContext {
    pub fn add_batch(&mut self, name: &str, batch: Vec<Journey>) {
        let handle = BatchHandle { index: self.journey_batches.len() };
        self.journey_named_batches.insert(name.to_string(), handle);
        self.journey_batches.push(batch);
    }
}
```

## SDK-Based Testing

The `dapple-sdk` provides a typed client that simplifies test setup and validation.

### Guide Ingest Dry Run

Use the SDK to validate a guide without materializing:

```rust
use dapple_sdk::DappleClient;

let client = DappleClient::with_api_token("http://localhost:8001", "dpl_test_token");

// Dry run — parse and validate only, no database writes
let summary = client.guides()
    .ingest_from_dir_dry_run("./test-guides/essay-helper/")
    .await?;

assert!(summary.warnings.is_empty(), "Unexpected warnings: {:?}", summary.warnings);
assert!(summary.derived_playbook_stages > 0, "Expected derived playbook stages");
```

### SDK Integration Patterns

```rust
// Create objects via SDK for test setup
let playbooks = client.playbooks().list(None).await?;
let journey = client.journeys().create(CreateJourneySetSchema {
    objs: vec![CreateJourneySchema {
        name: "Test Journey".into(),
        playbook_id: playbooks[0].id,
    }],
}).await?;

// Verify via SDK
let fetched = client.journeys().get(journey[0].id).await?;
assert_eq!(fetched.name, "Test Journey");
```

## Pre-Release Checklist

Before publishing a Playbook or BriefPack:

- [ ] **Sample inputs included** - Provide example JSON for each Task
- [ ] **Required binaries documented** - List dependencies (e.g., `pandoc`, `jq`)
- [ ] **Privacy/PII review completed** - Mask sensitive data in examples
- [ ] **Artifact schema validated** - All Tasks write valid `artifact.json`
- [ ] **Error paths tested** - Verify `error.json` is written on failure
- [ ] **Idempotency verified** - Tasks can be safely re-run
- [ ] **End-to-end smoke test passes** - Full pipeline works

## Testing Best Practices

- **Seed principals** explicitly in tests via `dapple_data::query::*::insert_*`
- **Use ScenarioContext** to track batches
- **Preserve ordering** - verify POST response order matches GET order
- **Wait for dependencies** - poll ScenarioContext, don't sleep
- **Log diagnostics** - CreateSchema payload, InsertArgs lengths
- **Compare POST vs GET** - ensure INSERT...RETURNING matches SELECT

## Common Issues

| Symptom | Likely Cause | Fix |
|---------|--------------|-----|
| Task exits 0 but no artifact | Forgot to write `artifact.json` | Add artifact write at end |
| Next Task can't find input | Wrong `IN_ARTIFACT` path | Check previous artifact |
| JSON parse error | Malformed input | Validate JSON structure |
| `error.json` not created | Task crashed before error handling | Add try/catch |
| Logs missing | Wrote to wrong directory | Use `$env.OUT_DIR` |
