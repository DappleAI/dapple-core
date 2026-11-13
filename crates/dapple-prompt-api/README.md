# dapple-prompt-api

REST API service for Dapple Prompt, built with Axum and providing OpenAPI 3.1 documentation.

## Overview

The `dapple-prompt-api` crate is the main API service for Dapple Prompt, exposing REST endpoints for creating, reading, and updating database objects. It serves as the primary interface to the [dapple-data](../dapple-data/README.md) data access layer.

**Key Features:**
- Batch-first API design for high throughput
- Automatic OpenAPI 3.1 documentation via `aide`
- Order-preserving operations
- Type-safe request/response schemas with JSON Schema

## Quick Start

### Running the API Server

```bash
# Set database connection
export DATABASE_URL="postgresql://user:pass@localhost:5432/dapple_dev"

# Run server
cargo run -p dapple-prompt-api

# Server starts on http://localhost:8080
```

### API Documentation

OpenAPI documentation available at:
- **ReDoc UI**: http://localhost:8080/docs
- **OpenAPI JSON**: http://localhost:8080/api.json

## Core Concepts

The API is built around **batch operations** where clients can create/read/update multiple objects in a single request while preserving order and atomicity.

## Available Endpoints

| Path | Description | Versioned |
|------|-------------|-----------|
| `/api/playbooks/` | Playbook management | Yes |
| `/api/hats/` | Hat definitions | No |
| `/api/steps/` | Step definitions | No |
| `/api/tasks/` | Task definitions | No |
| `/api/categories/` | Category definitions | No |
| `/api/stages/` | Stage definitions | No |
| `/api/journeys/` | Journey management | No |
| `/api/dispatches/` | Dispatch execution instances | No |
| `/api/fluent-message-paths/` | Fluent message paths (read-only) | - |
| `/api/fluent-messages/` | Localized messages | Yes |
| `/api/value-slot-paths/` | Value slot paths (read-only) | - |
| `/api/value-slots/` | Configurable value slots | Yes |
| `/api/asset-paths/` | Asset paths (read-only) | - |
| `/api/assets/` | Asset management | Yes |
| `/api/brief-paths/` | Brief paths (read-only) | - |
| `/api/briefs/` | Brief content | Yes |
| `/api/brief-group-paths/` | Brief group paths (read-only) | - |
| `/api/brief-groups/` | Groups of briefs | Yes |
| `/api/brief-pack-paths/` | Brief pack paths (read-only) | - |
| `/api/brief-packs/` | Packs of brief groups | Yes |
| `/api/guides/` | Structured documentation (chapters, blocks, tags) | Yes |
| `/api/dispatch-intent-bindings/` | Intent bindings for dispatches | No |

## Architecture

### Module Organization

Clear separation of concerns with one module per principal object:

```
src/
├── handler/           # Axum request handlers
│   ├── briefs.rs      # Brief endpoints (versioned)
│   ├── brief_groups.rs # BriefGroup endpoints (versioned + associative)
│   ├── brief_packs.rs # BriefPack endpoints (versioned + associative)
│   ├── journeys.rs    # Journey endpoints
│   ├── playbooks.rs   # Playbook endpoints
│   └── tasks.rs       # Task endpoints
├── route/             # Route definitions
│   ├── journeys.rs    # Journey routes
│   └── ...
├── schema/            # Request/response schemas
│   ├── journeys.rs    # Journey DTOs
│   └── ...
└── handler.rs         # Shared handler utilities
```

**Module Responsibilities:**
- `handler/` — Business logic, calls `dapple-data` queries
- `route/` — Axum route registration with `aide` for OpenAPI
- `schema/` — Request/response types with `JsonSchema` derives

### Type Conventions

```rust
// Request schema for POST
#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateJourneySchema {
    pub name: String,
    pub playbook_object_id: Uuid,
}

// Batch request schema
#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateJourneySetSchema {
    pub objs: Vec<CreateJourneySchema>,
}

// Query schema with enum variants
#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetJourneySetSchema {
    ByObjectIds { ids: Vec<Uuid>, options: Option<FilterOptions> },
    ByPlaybook { playbook_id: Uuid, options: Option<FilterOptions> },
    ByUser { user_id: Uuid, options: Option<FilterOptions> },
}
```

**Conventions:**
- Always use `Uuid` for object IDs
- Batch requests use `{objs: Vec<T>}` wrapper
- Query endpoints use enum for different query modes
- All schemas derive `JsonSchema` for OpenAPI docs

### API Design Principles

1. **Batch-First** — All POST operations accept `Vec<T>`
2. **Order Preservation** — Responses maintain input order
3. **Flexible Queries** — GET uses enum for different query modes
4. **Type Safety** — Compile-time schema validation

## API Examples

### Creating Journeys (Batch POST)

**Request:**
```bash
POST /journeys
Content-Type: application/json

{
  "objs": [
    {
      "name": "Onboarding Journey",
      "playbook_object_id": "550e8400-e29b-41d4-a716-446655440000"
    },
    {
      "name": "Advanced Features",
      "playbook_object_id": "550e8400-e29b-41d4-a716-446655440001"
    }
  ]
}
```

**Response:**
```json
[
  {
    "object_id": "123e4567-e89b-12d3-a456-426614174000",
    "name": "Onboarding Journey",
    "playbook_object_id": "550e8400-e29b-41d4-a716-446655440000",
    "steps": []
  },
  {
    "object_id": "123e4567-e89b-12d3-a456-426614174001",
    "name": "Advanced Features",
    "playbook_object_id": "550e8400-e29b-41d4-a716-446655440001",
    "steps": []
  }
]
```

### Querying Journeys (GET with Enum Query)

**By Object IDs:**
```bash
GET /journeys?query=by_object_ids&ids[]=123e4567-e89b-12d3-a456-426614174000&ids[]=123e4567-e89b-12d3-a456-426614174001
```

**By Playbook:**
```bash
GET /journeys?query=by_playbook&playbook_id=550e8400-e29b-41d4-a716-446655440000&options[limit]=10&options[page]=1
```

**By User:**
```bash
GET /journeys?query=by_user&user_id=7c9e6679-7425-40de-944b-e07fc1f90ae7
```

### Updating Playbooks (PATCH - Creates New Version)

**Request:**
```bash
PATCH /playbooks
Content-Type: application/json

{
  "objs": [
    {
      "object_id": "550e8400-e29b-41d4-a716-446655440000",
      "name": "Updated Playbook Name"
    }
  ]
}
```

**Response:**
```json
[
  {
    "object_id": "550e8400-e29b-41d4-a716-446655440000",
    "path_object_id": "770e8400-e29b-41d4-a716-446655440000",
    "name": "Updated Playbook Name",
    "version_major": 0,
    "version_minor": 0,
    "version_patch": 2
  }
]
```

## Handler Pattern

### Typical Handler Implementation

```rust
use aide::axum::IntoApiResponse;
use axum::{extract::State, Json};
use dapple_data::{query::journeys, model::journeys::InsertJourneyArgs};

pub async fn journeys_post_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateJourneySetSchema>,
) -> impl IntoApiResponse {
    // 1. Look up shared dependents
    let playbook_ids = payload.objs.iter()
        .map(|obj| obj.playbook_object_id)
        .collect::<Vec<_>>();

    let playbooks = select_playbooks_by_universal_ids(
        &state.db,
        playbook_ids,
        FilterOptions::default()
    ).await?;

    // 2. Build lookup map
    let playbook_map: HashMap<Uuid, i64> = playbooks.iter()
        .map(|p| (p.object_id, p.id))
        .collect();

    // 3. Build insert args
    let args = InsertJourneyArgs {
        names: payload.objs.iter().map(|o| o.name.clone()).collect(),
        playbook_ids: payload.objs.iter()
            .map(|o| playbook_map[&o.playbook_object_id])
            .collect(),
    };

    // 4. Insert batch
    let inserted = insert_journeys(&state.db, args).await?;

    // 5. Convert to DTOs
    let dtos = inserted.into_iter()
        .map(|m| Journey::from(m))
        .collect::<Vec<_>>();

    post_response_handler("journeys", Ok(dtos))
}
```

### Route Registration

```rust
use aide::axum::{routing::{get, post}, ApiRouter};

pub fn journeys_routes() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route("/journeys", post(journeys_post_handler))
        .api_route("/journeys", get(journeys_index_handler))
        .api_route("/journeys/:id", get(journeys_get_handler))
}
```

## Testing

### Integration Tests

Integration tests verify full lifecycle from POST → GET → PATCH:

```rust
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
            playbook_object_id: playbooks[0].object_id,
        },
        CreateJourneySchema {
            name: "Journey 2".into(),
            playbook_object_id: playbooks[1].object_id,
        },
    ];

    let journeys = post_journeys(&pool, journey_reqs).await;
    ctx.add_batch("journeys-init", journeys.clone());

    // 3. Verify GET returns same data
    let fetched = get_journeys_by_ids(
        &pool,
        &journeys.iter().map(|j| j.object_id).collect::<Vec<_>>()
    ).await;

    assert_eq!(journeys.len(), fetched.len());
    for (created, fetched) in journeys.iter().zip(fetched.iter()) {
        assert_eq!(created.object_id, fetched.object_id);
        assert_eq!(created.name, fetched.name);
    }
}
```

### Test Conventions

**Lifecycle Test Rules:**
1. **Shared dependents first** — Create all dependencies before principals
2. **Batch naming** — First batch always named `{endpoint}-init`
3. **Order verification** — Assert POST and GET return same order
4. **No faking shared data** — Always create real shared dependents
5. **Explicit dependencies** — Tests declare what batches they need

**Running Tests:**
```bash
# All integration tests
cargo test --package dapple-prompt-api --test user-lifecycle

# Specific test
cargo test --package dapple-prompt-api --test user-lifecycle -- test_journey_lifecycle

# With logging
RUST_LOG=debug cargo test
```

## Error Handling

The API uses consistent HTTP status codes and error responses:

```rust
use axum::http::StatusCode;
use dapple_data::error::DataError;

pub fn map_data_error(err: DataError) -> StatusCode {
    match err {
        DataError::NotFound(_) => StatusCode::NOT_FOUND,
        DataError::Validation(_) => StatusCode::BAD_REQUEST,
        DataError::Conflict(_) => StatusCode::CONFLICT,
        DataError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
```

**Error Response Format:**
```json
{
  "error": "Not found: Journey with id 123e4567-e89b-12d3-a456-426614174000",
  "status": 404
}
```

## Dependencies

Key dependencies:
- `axum` — Web framework
- `aide` — OpenAPI 3.1 documentation generation
- `sqlx` — Database access (via dapple-data)
- `serde` / `serde_json` — Serialization
- `schemars` — JSON Schema for OpenAPI
- `uuid` — UUID support
- `tower-http` — CORS and middleware
- `tokio` — Async runtime

## Configuration

Environment variables:
- `DATABASE_URL` — PostgreSQL connection string (required)
- `RUST_LOG` — Logging level (e.g., `info`, `debug`)
- `PORT` — Server port (default: 8080)
- `HOST` — Server host (default: 0.0.0.0)

## Development

### Adding a New Endpoint

1. **Define schema** in `src/schema/{resource}.rs`:
   ```rust
   #[derive(JsonSchema, Serialize, Deserialize)]
   pub struct CreateResourceSchema {
       pub name: String,
   }

   #[derive(JsonSchema, Serialize, Deserialize)]
   pub struct CreateResourceSetSchema {
       pub objs: Vec<CreateResourceSchema>,
   }
   ```

2. **Implement handler** in `src/handler/{resource}.rs`:
   ```rust
   pub async fn resource_post_handler(
       State(state): State<Arc<AppState>>,
       Json(payload): Json<CreateResourceSetSchema>,
   ) -> impl IntoApiResponse {
       // Handler logic
   }
   ```

3. **Register routes** in `src/route/{resource}.rs`:
   ```rust
   pub fn resource_routes() -> ApiRouter<AppState> {
       ApiRouter::new()
           .api_route("/resources", post(resource_post_handler))
   }
   ```

4. **Add to main router** in `src/lib.rs`:
   ```rust
   app = app.nest("/", resource::resource_routes());
   ```

5. **Write integration test** in `tests/user_lifecycle.rs`

### Running Locally

```bash
# Start PostgreSQL (if using Docker)
docker run -d \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=dapple_dev \
  -p 5432:5432 \
  postgres:15

# Apply migrations
psql $DATABASE_URL -f ../dapple-data/migrations/*.sql

# Run server
cargo run -p dapple-prompt-api

# Test endpoint
curl http://localhost:8080/journeys?query=by_user&user_id=00000000-0000-0000-0000-000000000001
```

## Performance Considerations

### Batch Size Limits
- Recommended: 100-1000 objects per request
- Maximum: Limited by PostgreSQL parameter limit (65535 total parameters)
- Calculate: `num_fields * batch_size < 65535`

### Query Optimization
- All queries use prepared statements via SQLx
- Indexes created for foreign keys and common filters
- JOIN operations flattened in single query to minimize round-trips

### Caching
Redis caching available via `dapple-data` cache layer (optional).

## OpenAPI Documentation

The API automatically generates OpenAPI 3.1 specs using `aide`:

```rust
use aide::axum::{routing::post, ApiRouter, IntoApiResponse};

ApiRouter::new()
    .api_route("/journeys", post(journeys_post_handler))
    // Automatically includes in OpenAPI spec
```

Access documentation:
- **Interactive docs**: http://localhost:8080/docs
- **JSON spec**: http://localhost:8080/api.json

## Contributing

When adding new features:
1. Follow patterns in existing handlers (e.g., `journeys.rs`)
2. Use batch operations for all POST/PATCH endpoints
3. Preserve order in responses
4. Add integration tests for full lifecycle
5. Update OpenAPI annotations
6. Run `cargo fmt` and `cargo clippy`

## References

- [dapple-data README](../dapple-data/README.md) — Data layer documentation
- [CLAUDE.md](../../CLAUDE.md) — Project conventions
- [Axum Documentation](https://docs.rs/axum/)
- [aide Documentation](https://docs.rs/aide/)
- [OpenAPI 3.1 Specification](https://spec.openapis.org/oas/v3.1.0)