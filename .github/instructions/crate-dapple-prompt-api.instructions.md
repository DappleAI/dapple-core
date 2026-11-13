---
applyTo: "crates/dapple-prompt-api/src/**/*"
title: "Dapple Prompt API Crate Guide"
description: "The REST layer to access the data layer of this workspace."
---

# `dapple-prompt-api` REST API Instructions

## Philosophy

### General Philosophy
- Create consistent routes, handles, and schemas for all endpoints to ensure a predictable and maintainable API structure.
- Routes should follow the data layer closely with root routs for Principal Objects and nested routes for Dependent Objects.

### Testing Philosophy
- Every endpoint you create must be fully covered in the lifecycle integration tests. These tests are the most advanced consumer of the API and ensure the integrity of the data layer.


## Additional Resources
- API documentation available via Scalar UI when running the service
- OpenAPI spec generated at runtime by aide

## Integration Testing & Lifecycle Tests

This file collects guidance intended for test agents and contributors running lifecycle/integration tests. Move agent-specific instructions here rather than keeping them in README.md.

Key type names and concepts (use these exact names in tests and implementations):
- Principal Object: top-level entity under test (e.g., Playbook, Journey, Step, Task)
- Owned Dependent: data owned by a Principal Object and created/managed by its POST (no separate test creation required)
- Shared Dependent: data that must exist independently (e.g., Tasks that are reused) and must be created before associating with a Principal Object
- ScenarioContext: shared test state that stores named batches of created objects for later tests
- BatchHandle: lightweight handle stored in ScenarioContext representing one created batch (name + vector of objects)
- CreateXSchema: test/client-side creation schema for object X (used to POST)
- InsertXArgs: flattened column-wise args passed to UNNEST in SQL inserts
- FlatXModel / InsertedXModel / PublicXModel: dapple-data internal models mapping SQL rows to rust structs

Important testing patterns
- Batches and ordering:
  - Tests create data in batches saved to ScenarioContext under a name (first batch convention: "{endpoint}-init").
  - When creating a set, tests should preserve order. Use the UNNEST-with-ORDINALITY pattern in SQL and join args.ord into the final SELECT so GETs and POST responses can be ordered and compared reliably.
  - SQL pattern to use in assets/*.sql:
    WITH args AS (
      SELECT *
      FROM UNNEST($1::UUID[], $2::UUID[]) WITH ORDINALITY AS o(col1, col2, ord)
    ), ...
    ... final SELECT ... ORDER BY args.ord

- Waiting for dependencies:
  - Lifecycle tests must wait for required Shared Dependents to exist (by checking ScenarioContext batches) before running dependent trials.
  - Prefer explicit wait-for-batch loops (poll ScenarioContext) rather than time sleeps.

- Column-wise inserts:
  - Build InsertXArgs so every vector (each column) has identical length (one element per object).
  - Group UNNEST parameters into a single UNNEST call with WITH ORDINALITY for performance and deterministic ordinals.

- Validation and comparisons:
  - POST responses (from INSERT...RETURNING) must be compared against GETs. Ensure SQL SELECTs return the same column names and sources (e.g., use playbook_tasks.created_at, not tasks.created_at).
  - Fold/convert Flat/Inserted models to public models consistently; watch for copy-paste bugs that mix parent/child ids.

- Diagnostics:
  - When tests fail, log the CreateXSchema payload, constructed InsertXArgs lengths, and the SQL-returned rows. This typically reveals mismatched lengths, incorrect UUIDs, or ordering issues.

Use this AGENTS.md as the authoritative guidance for integration test authors and automated agents that scaffold tests.

## Testing Concepts

- Principal objects (e.g., `stages`) must be seeded by the test itself when needed. Tests are responsible for inserting principals via `dapple_data` helper functions (for example `dapple_data::query::stages::insert_stages`) and registering them into the `ScenarioContext`.

- Associative dependent tables (e.g., `playbook_stages`, `dispatch_stages`, `playbook_steps`, `dispatch_steps`) are not created at the same time as their principals in production code; instead callers in tests or the API insert dependents via the associative insert paths after principals exist.

- The lifecycle test (`tests/user_lifecycle.rs`) seeds `stages` at test start and registers them in `ScenarioContext` so endpoint fakes can reference them. This mirrors how steps/tasks are produced earlier in the test and keeps test setup explicit.

- Avoid seeding principals in application runtime (e.g., `main`) — tests should be self-contained to avoid surprising side effects in other environments.

- When adding new principal objects in the schema, update this file and the lifecycle tests to include seeding guidance so future contributors and automated agents understand the test seeding model.

## Important Considerations
> The test suites you manage for this API are crucial to not only test your code but to ensure the integrity of the data layer. Remain diligent.
> You have access to the pgsql extension tool in VSCode for exploring the database schema and running queries directly against the prompt-db.kube connection, which is the same as $DAPPLE_PROMPT_DATABASE_URL.
