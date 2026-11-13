# Adding Resource Clients

> **Audience:** Backend Developers · Platform Engineers

Step-by-step guide for adding a new resource client to the SDK. Resource clients are thin wrappers over `DappleClient`'s HTTP methods that provide typed, ergonomic access to a single API resource.

## Overview

Every resource client follows the same structure:

1. A `{Entity}Client<'a>` struct that borrows `DappleClient`
2. A `pub(crate) fn new(client: &'a DappleClient) -> Self` constructor
3. Public async methods for each endpoint (`get`, `list`, `create`, etc.)
4. An accessor method on `DappleClient` in `lib.rs`

Two reference implementations cover the spectrum of complexity:

| File | Pattern | Endpoints |
|------|---------|-----------|
| `resource/playbooks.rs` | Standard CRUD | get, list, list_by_ids, list_by_slugs, create, delete, delete_one |
| `resource/guides.rs` | Complex | CRUD + multipart ingest + binary render + cards + favorites |

## Step 1: Create the Resource Module

Create `src/resource/{entity}.rs`. Start with the standard CRUD pattern from `playbooks.rs`:

```rust
//! Client for the `/api/{entities}/` endpoints.

use crate::types::{
    Create{Entity}Schema, Create{Entity}SetSchema, DeleteObjectSetSchema, FilterOptions,
    Get{Entity}SetSchema, {Entity},
};
use crate::{DappleClient, SdkError};
use uuid::Uuid;

/// Client for {entity} operations.
pub struct {Entity}Client<'a> {
    client: &'a DappleClient,
}

impl<'a> {Entity}Client<'a> {
    pub(crate) fn new(client: &'a DappleClient) -> Self {
        Self { client }
    }

    /// Get a single {entity} by object ID.
    pub async fn get(&self, id: Uuid) -> Result<{Entity}, SdkError> {
        self.client.get_one(&format!("api/{entities}/{id}")).await
    }

    /// List all {entities}.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<{Entity}>, SdkError> {
        let query = Get{Entity}SetSchema::All { options };
        self.client.get_many("api/{entities}/", &query).await
    }

    /// List {entities} by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<{Entity}>, SdkError> {
        let query = Get{Entity}SetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/{entities}/", &query).await
    }

    /// Create one or more {entities}.
    pub async fn create(&self, objs: Vec<Create{Entity}Schema>) -> Result<Vec<{Entity}>, SdkError> {
        let body = Create{Entity}SetSchema { objs };
        self.client.post_many("api/{entities}/", &body).await
    }

    /// Delete (deprecate) a single {entity} by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<{Entity}, SdkError> {
        self.client.delete_one(&format!("api/{entities}/{id}")).await
    }
}
```

### Key Conventions

- The struct borrows `DappleClient` with lifetime `'a` -- resource clients are transient handles, not owned objects.
- The constructor is `pub(crate)` -- only `DappleClient` creates resource clients.
- Public methods take owned values (not references) for IDs and schemas to match the ergonomics of the generated types.
- Path strings use `api/{entities}/` (with trailing slash for list endpoints, no trailing slash for single-object endpoints with `{id}`).

## Step 2: Choose the Right Client Methods

Map each endpoint to the appropriate `DappleClient` method based on the response shape:

| Endpoint Pattern | Client Method | Returns |
|------------------|---------------|---------|
| `GET /api/{entities}/{id}` | `get_one(path)` | Single `T` from `{ obj }` |
| `GET /api/{entities}/?query=...` | `get_many(path, query)` | `Vec<T>` from `{ objs }` |
| `POST /api/{entities}/` (batch) | `post_many(path, body)` | `Vec<T>` from `{ objs }` |
| `POST /api/{entities}/` (single) | `post_one(path, body)` | Single `T` from `{ obj }` |
| `PATCH /api/{entities}/` | `patch_many(path, body)` | `Vec<T>` from `{ objs }` |
| `DELETE /api/{entities}/{id}` | `delete_one(path)` | Single `T` from `{ obj }` |
| `DELETE /api/{entities}/` (body) | `delete_many_with_body(path, body)` | `Vec<T>` from `{ objs }` |
| `DELETE /api/{entities}/` (query) | `delete_many(path, query)` | `Vec<T>` from `{ objs }` |
| Multipart upload | `post_multipart(path, form)` | Parsed JSON `T` |
| Binary download | `get_bytes(path)` | `Bytes` |
| Binary response to POST | `post_bytes(path, body)` | `Bytes` |
| Non-standard JSON | `post_raw(path, body)` | `serde_json::Value` |

## Step 3: Register the Module

Add the module to `src/resource/mod.rs`:

```rust
pub mod {entity};
```

## Step 4: Add the Accessor to DappleClient

In `src/lib.rs`, add the accessor method inside the resource client accessors section:

```rust
/// Access the {Entity} API.
pub fn {entities}(&self) -> resource::{entity}::{Entity}Client<'_> {
    resource::{entity}::{Entity}Client::new(self)
}
```

The accessor follows the naming convention: the method name is the plural resource name (e.g., `playbooks()`, `brief_groups()`, `dispatch_task_attempts()`).

## Step 5: Verify Types Exist

The resource client imports types from `crate::types`, which re-exports from `crate::generated`. Verify that the OpenAPI spec includes:

- `{Entity}` -- The DTO returned by GET/POST
- `Create{Entity}Schema` -- Single-object POST body
- `Create{Entity}SetSchema` -- Batch wrapper with `objs: Vec<Create{Entity}Schema>`
- `Get{Entity}SetSchema` -- Query enum with variants (`All`, `ByObjectIds`, etc.)

If any types are missing from the generated code, you have two options:

1. **Add the endpoint to the API first** so the types appear in the OpenAPI spec and are generated automatically.
2. **Define manual types in `src/types.rs`** for cases where the API response does not follow the standard envelope (see `ApiTokenInfo`, `AllQuery`, `GetDispatchTaskAttemptSetSchema` for examples).

## Handling Non-Standard Responses

### Multipart Ingest

For endpoints that accept tarball uploads, use `post_multipart()` with `reqwest::multipart::Form`:

```rust
pub async fn ingest(
    &self,
    tarball_bytes: Vec<u8>,
    params: IngestParams,
) -> Result<IngestGuideResponse, SdkError> {
    let path = build_ingest_path("api/guides/ingest", &params);
    let part = Part::bytes(tarball_bytes)
        .file_name("guide.tar.gz")
        .mime_str("application/gzip")
        .map_err(|e| SdkError::Config(format!("invalid mime type: {e}")))?;
    let form = Form::new().part("file", part);
    self.client.post_multipart(&path, form).await
}
```

When feature-gating tarball creation from a directory, use `#[cfg(feature = "tarball")]`:

```rust
#[cfg(feature = "tarball")]
pub async fn ingest_from_dir(
    &self,
    dir: &std::path::Path,
    params: IngestParams,
) -> Result<IngestGuideResponse, SdkError> {
    let tarball_bytes = create_tarball_from_dir(dir)?;
    self.ingest(tarball_bytes, params).await
}
```

### Binary Downloads

For endpoints returning raw bytes (PDF, asset content), use `get_bytes()` or `post_bytes()`:

```rust
pub async fn render_pdf(
    &self,
    guide_id: Uuid,
    profile: &str,
) -> Result<bytes::Bytes, SdkError> {
    #[derive(serde::Serialize)]
    struct RenderBody<'a> {
        profile: &'a str,
    }
    let body = RenderBody { profile };
    self.client
        .post_bytes(&format!("api/guides/{guide_id}/render"), &body)
        .await
}
```

### Non-Envelope JSON

For endpoints that return non-standard JSON (not wrapped in `{ obj }` or `{ objs }`), use `post_raw()` and deserialize manually:

```rust
pub async fn create(&self, schema: CreateApiTokenSchema) -> Result<CreateApiTokenResponse, SdkError> {
    let value = self.client.post_raw("api/auth/tokens", &schema).await?;
    serde_json::from_value(value).map_err(|e| SdkError::Deserialization(e.to_string()))
}
```

## Handling Convenience Methods

Some resource clients provide convenience methods that compose lower-level calls. For example, `GuidesClient::get_by_slug()` calls `list_by_slugs()` and extracts the first result:

```rust
pub async fn get_by_slug(&self, slug: &str) -> Result<Guide, SdkError> {
    let results = self.list_by_slugs(vec![slug.to_string()], None).await?;
    results.into_iter().next().ok_or_else(|| SdkError::Api {
        status: 404,
        message: format!("guide with slug '{slug}' not found"),
    })
}
```

This pattern is appropriate when:
- The API has no single-object-by-slug endpoint
- The convenience method is commonly needed by callers
- The composition is trivial (one call + extraction)

## Complete Checklist

1. [ ] Create `src/resource/{entity}.rs` with `{Entity}Client` struct
2. [ ] Implement standard methods: `get()`, `list()`, `list_by_ids()`, `create()`
3. [ ] Add additional methods for entity-specific variants (e.g., `list_by_slugs()`, `list_by_path_ids()`)
4. [ ] Add `pub mod {entity};` to `src/resource/mod.rs`
5. [ ] Add accessor method to `DappleClient` in `src/lib.rs`
6. [ ] Verify all referenced types exist in generated code or `src/types.rs`
7. [ ] Handle non-standard responses (multipart, binary, non-envelope) if applicable
8. [ ] Build and verify: `cargo build -p dapple-sdk`

## Common Mistakes

- **Using `&str` instead of owned values** -- Generated types use `String` and `Vec<Uuid>`, not references. Match the generated signatures.
- **Forgetting the trailing slash** -- List endpoints need `api/{entities}/` with a trailing slash for query parameter serialization to work correctly.
- **Importing from `generated` directly** -- Always import from `crate::types`, which re-exports generated types and includes manual additions.
- **Adding `pub` to the constructor** -- The `new()` constructor must be `pub(crate)` so only `DappleClient` creates resource clients. External code accesses clients through accessor methods.
- **Feature-gating the entire module** -- Only feature-gate individual methods that require optional dependencies (e.g., `ingest_from_dir`), not the whole module.
- **Constructing raw JSON responses** -- Use the typed envelope methods (`get_one`, `get_many`, `post_many`) whenever the API follows the standard envelope pattern. Reserve `post_raw()` for genuinely non-standard endpoints.

## Next Steps

- [SDK Overview](./overview.md) -- Architecture, code generation, and authentication internals
