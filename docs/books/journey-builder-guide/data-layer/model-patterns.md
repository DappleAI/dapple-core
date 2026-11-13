# Model Patterns

> **Audience:** Backend Developers · Platform Engineers

Type conventions and model patterns for the dapple-data layer.

## Type Naming Conventions

### Data Transfer Objects (DTOs)

Public-facing types for API responses:

```rust
// Public-facing DTO (for API responses)
pub struct Journey {
    pub id: Uuid,               // PK (UUIDv7)
    pub name: String,
    pub playbook_id: Uuid,
    pub steps: Vec<Step>,       // Owned Dependents as Vec
}
```

**DTO Conventions:**
- PK is `id: Uuid` (UUIDv7) — same field used internally and externally
- Prefer nullable timestamps over booleans for state
- Owned Dependents appear as `Vec<DependentDto>`
- Shared Dependents appear as `{table_name}_id: Uuid`
- Lookup table FKs remain `i16`/`i64` (not UUID)

### Internal Models

Complete internal model with all fields:

```rust
pub(crate) struct JourneyModel {
    pub id: Uuid,               // PK (UUIDv7)
    pub name: String,
    pub playbook_id: Uuid,
    pub steps: Vec<StepModel>,
}
```

### Flat Models

Flattened query result from JOINs:

```rust
pub(crate) struct FlatJourneyModel {
    // Journey fields
    pub id: Uuid,
    pub name: String,
    // Owned Dependent fields prefixed with table name
    pub step_id: Option<Uuid>,
    pub step_name: Option<String>,
    // Shared Dependent FK
    pub task_id: Option<Uuid>,
}
```

### Inserted Models

INSERT...RETURNING result:

```rust
pub(crate) struct InsertedJourneyModel {
    pub id: Uuid,               // PK (UUIDv7)
    pub name: String,
}
```

### Insert Arguments

Column-wise insert parameters:

```rust
pub(crate) struct InsertJourneyArgs {
    pub names: Vec<String>,
    pub playbook_ids: Vec<Uuid>,
    // All vectors MUST be same length
}
```

### Request/Response Schemas

```rust
// POST request schema
#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateJourneySchema {
    pub name: String,
    pub playbook_id: Uuid,
}

// Batch create schema
#[derive(JsonSchema, Serialize, Deserialize)]
pub struct CreateJourneySetSchema {
    pub objs: Vec<CreateJourneySchema>,
}

// GET query schema with enum variants
#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetJourneySetSchema {
    ByIds { ids: Vec<Uuid>, options: Option<FilterOptions> },
    ByPlaybook { playbook_id: Uuid, options: Option<FilterOptions> },
}
```

## Object Model Types

### Principal Object

Top-level entity with independent lifecycle:

```rust
pub struct Task {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}
```

### Versioned Object

Immutable entity with version history:

```rust
pub struct Brief {
    pub id: Uuid,                  // This version's UUID (UUIDv7)
    pub path_id: Uuid,             // Version lineage UUID
    pub version: Version,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

pub struct Version {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
}
```

### Path Object

Tracks version history:

```rust
pub struct BriefPath {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
}
```

## Type Suffixes

| Suffix | Purpose |
|--------|---------|
| `Model` | Internal with `id` |
| `FlatModel` | Query result to be folded |
| `InsertedModel` | From INSERT...RETURNING |
| `Args` | Column-wise insert parameters |
| `Schema` | Request/response DTOs |
| `SetSchema` | Batch operations |

## Rust Query Function Patterns

### Insert Function

```rust
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
        &args.names,         // :bind(names) -> $1
        &args.playbook_ids   // :bind(playbook_ids) -> $2
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
```

### Select Function with Folding

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

fn fold_journeys(flat: Vec<FlatJourneyModel>) -> Result<Vec<JourneyModel>, DataError> {
    // Group by journey id, collect dependents
    // Implementation uses itertools::group_by or manual folding
}
```

## Additional Types

| Type | Purpose |
|------|---------|
| `ContentHash` | SHA-1 hex string (40 chars) for content-addressable deduplication. Defined in `dapple-data::hash`. Use `.short()` for 8-char display, `.as_str()` for full hash. |

## pgvector Exception

Embedding queries use `sqlx::query_as()` with `include_str!()` instead of the standard `query_file_as!()` macro. This is the documented exception for `vector(768)` columns — SQLx compile-time checking cannot verify pgvector's custom type. See `dapple-data/src/query/guide_content_embeddings.rs` for the pattern.

## Best Practices

1. **PK is `id: Uuid`** - Same field used internally and in DTOs (UUIDv7)
2. **Use timestamps over booleans** - `revoked_at` instead of `is_revoked`
3. **Validate vector lengths** - Assert all Args vectors are equal length
4. **Preserve ordering** - UUIDv7 `id` encodes creation time; no separate ordinals needed
5. **Use query files** - Never inline SQL (exception: pgvector queries, see above)
6. **Lookup tables keep integer PKs** - `id: i16`/`id: i64` for enum/config tables
