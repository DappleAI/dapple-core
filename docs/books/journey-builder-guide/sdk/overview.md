# SDK Internals

> **Audience:** Backend Developers · Platform Engineers

The Dapple SDK (`dapple-sdk`) is a typed Rust HTTP client for the Dapple Prompt API. DTO types come from the shared `dapple-types` crate (re-exported through `src/types.rs`), and hand-authored resource clients provide ergonomic RPC-style access to every API endpoint. (Earlier revisions generated types from a checked-in OpenAPI snapshot via typify at build time; that codegen — `build.rs`, `openapi/internal-api.json`, `src/generated.rs` — was removed when `dapple-types` became the single source of truth shared by the API surfaces and the SDK.)

## Crate Structure

```
crates/dapple-sdk/
├── Cargo.toml           # depends on `dapple-types` (the shared DTO crate)
└── src/
    ├── lib.rs           # DappleClient, SdkError
    ├── client.rs        # HTTP methods (get, post, patch, delete)
    ├── envelope.rs      # Response unwrapping ({ obj: ... }, { objs: [...] })
    ├── query.rs         # Bracket-notation query serialization
    ├── types.rs         # `pub use dapple_types::…` re-exports + SDK-local request schemas
    ├── auth/
    │   ├── mod.rs       # AuthProvider trait
    │   ├── api_token.rs # ApiTokenAuth (dpl_ prefix)
    │   ├── jwt.rs       # BearerAuth (raw JWT)
    │   └── service_token.rs # ServiceTokenAuth (Ed25519)
    └── resource/
        ├── mod.rs
        ├── guides.rs        # GuidesClient (complex: ingest, render, cards, favorites)
        ├── playbooks.rs     # PlaybooksClient (standard CRUD)
        ├── journeys.rs
        ├── dispatches.rs
        ├── briefs.rs, brief_groups.rs, brief_packs.rs, brief_units.rs
        ├── stages.rs, steps.rs, tasks.rs
        ├── categories.rs, hats.rs
        ├── assets.rs, drafts.rs
        ├── fluent_messages.rs, prompts.rs, directives.rs, value_slots.rs
        ├── inference_profiles.rs
        ├── shared_content.rs
        ├── versioned_objects.rs
        ├── deprecations.rs
        ├── dispatch_intent_bindings.rs, dispatch_phases.rs, dispatch_task_attempts.rs
        ├── user_details.rs, api_tokens.rs
        └── ...
```

## DappleClient

`DappleClient` is the top-level entry point. It holds an authenticated HTTP client and exposes typed resource accessors. Construction requires choosing one of three authentication methods:

```rust
pub struct DappleClient {
    http: reqwest::Client,
    base_url: String,
    auth: Box<dyn auth::AuthProvider>,
}
```

### Constructors

| Method | Auth Type | When to Use |
|--------|-----------|-------------|
| `with_api_token(base_url, token)` | Static `dpl_` token | CLI tools, scripts, development |
| `with_bearer_token(base_url, jwt)` | Pre-obtained JWT | When caller already has a JWT |
| `with_service_token(base_url, service_id, key)` | Ed25519 signing | Service-to-service (feature-gated) |

### Resource Accessors

Each accessor returns a borrowed client scoped to a single resource. The lifetime ties the resource client to `DappleClient`, preventing use-after-free:

```rust
impl DappleClient {
    pub fn playbooks(&self) -> PlaybooksClient<'_> { ... }
    pub fn guides(&self) -> GuidesClient<'_> { ... }
    pub fn journeys(&self) -> JourneysClient<'_> { ... }
    pub fn dispatches(&self) -> DispatchesClient<'_> { ... }
    pub fn briefs(&self) -> BriefsClient<'_> { ... }
    pub fn brief_groups(&self) -> BriefGroupsClient<'_> { ... }
    pub fn brief_packs(&self) -> BriefPacksClient<'_> { ... }
    pub fn brief_units(&self) -> BriefUnitsClient<'_> { ... }
    pub fn steps(&self) -> StepsClient<'_> { ... }
    pub fn stages(&self) -> StagesClient<'_> { ... }
    pub fn tasks(&self) -> TasksClient<'_> { ... }
    pub fn categories(&self) -> CategoriesClient<'_> { ... }
    pub fn hats(&self) -> HatsClient<'_> { ... }
    pub fn assets(&self) -> AssetsClient<'_> { ... }
    pub fn drafts(&self) -> DraftsClient<'_> { ... }
    pub fn fluent_messages(&self) -> FluentMessagesClient<'_> { ... }
    pub fn prompts(&self) -> PromptsClient<'_> { ... }
    pub fn directives(&self) -> DirectivesClient<'_> { ... }
    pub fn value_slots(&self) -> ValueSlotsClient<'_> { ... }
    pub fn inference_profiles(&self) -> InferenceProfilesClient<'_> { ... }
    pub fn deprecations(&self) -> DeprecationsClient<'_> { ... }
    pub fn dispatch_intent_bindings(&self) -> DispatchIntentBindingsClient<'_> { ... }
    pub fn dispatch_phases(&self) -> DispatchPhasesClient<'_> { ... }
    pub fn dispatch_task_attempts(&self) -> DispatchTaskAttemptsClient<'_> { ... }
    pub fn user_details(&self) -> UserDetailsClient<'_> { ... }
    pub fn api_tokens(&self) -> ApiTokensClient<'_> { ... }
    pub fn shared_content(&self) -> SharedContentClient<'_> { ... }
    pub fn versioned_objects(&self) -> VersionedObjectsClient<'_> { ... }
}
```

## SdkError

All SDK methods return `Result<T, SdkError>`. The error enum covers every failure mode:

```rust
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error (HTTP {status}): {message}")]
    Api { status: u16, message: String },

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
```

| Variant | Cause |
|---------|-------|
| `Http` | Network error, timeout, DNS failure (reqwest) |
| `Api` | Server returned non-2xx with error envelope |
| `Deserialization` | Response body did not match expected type |
| `Serialization` | Failed to serialize request body or query params |
| `Config` | Invalid client configuration (e.g., bad MIME type) |
| `Io` | File system errors (tarball creation) |
| `Json` | JSON parsing failure |

On HTTP 401, the client calls `auth.on_unauthorized()` before returning `Api`. This allows `ServiceTokenAuth` to invalidate its cached JWT so the next request triggers a re-exchange.

## Type Sourcing

DTO types are not generated. They live in the shared `dapple-types` crate and are
re-exported from `src/types.rs`:

```
crates/dapple-types  (the shared DTO crate)
        |
        |  consumed identically by:
        |    - dapple-prompt-api        (internal surface)
        |    - dapple-prompt-external-api (external surface)
        |    - dapple-sdk               (this client)
        v
    src/types.rs  (`pub use dapple_types::model::…::*;` + SDK-local request schemas)
```

Because every API surface and the SDK depend on the **same** `dapple-types`
structs, response DTOs cannot drift between the server and the client: a field
added to a DTO (e.g. the typed `display` hint on `BriefOutput` / `ExposedSlot`)
is served and consumed through one definition. This replaced the earlier typify
build-time codegen, which generated client types from a checked-in OpenAPI
snapshot that repeatedly drifted from the server (the drift that motivated the
`dapple-types` migration).

### SDK-local request schemas

Request bodies and query structs that the hand-authored resource clients send are
defined directly in `src/types.rs` (they are request shapes, not response DTOs):

- `IngestParams`, `IngestGuideResponse` -- Guide ingest configuration and response
- `SharedIngestParams`, `SharedIngestResponse` -- Shared content ingest
- `DeleteQueryParams` -- Parameters for versioned object deprecation
- `ApiTokenInfo`, `CreateApiTokenSchema`, `CreateApiTokenResponse` -- API token management
- `Create*Schema` / `Patch*Schema` (e.g. `CreateBriefSchema`, `CreateBriefOutputSchema`) -- POST/PATCH request bodies, kept field-for-field in step with each surface's create schema
- `AllQuery`, `ByObjectIdsQuery`, `ByJourneyQuery` -- Query structs for the GET variants
- `GetDispatchTaskAttemptSetSchema`, `GetVersionedObjectSetSchema` -- Query enums

## Authentication

### AuthProvider Trait

All auth strategies implement the internal `AuthProvider` trait:

```rust
#[async_trait]
pub(crate) trait AuthProvider: Send + Sync {
    async fn authorization_header(&self) -> Result<String, SdkError>;
    async fn on_unauthorized(&self);
}
```

### API Token (ApiTokenAuth)

The simplest auth mode. A static `dpl_`-prefixed token is sent as `Bearer {token}` on every request. No refresh logic -- `on_unauthorized()` is a no-op.

### Bearer JWT (BearerAuth)

Passes a pre-obtained JWT directly. Like API tokens, this is static with no refresh. Use when the caller already manages JWT lifecycle externally.

### Ed25519 Service Token (ServiceTokenAuth)

Feature-gated behind `service-auth`. Used for service-to-service communication:

1. Signs a payload (`service_id:timestamp`) with an Ed25519 private key.
2. POSTs the signed payload to `/api/auth/service-token` to exchange for a JWT.
3. Caches the JWT behind `Arc<RwLock<Option<String>>>`.
4. On 401, invalidates the cached JWT so the next request triggers re-exchange.

```rust
pub fn with_service_token(
    base_url: &str,
    service_id: &str,
    signing_key_bytes: &[u8; 32],
) -> Self
```

## HTTP Client Layer

The `client.rs` module provides the internal HTTP building blocks used by all resource clients. These methods handle authentication, request construction, and response envelope unwrapping.

### Envelope System

The API wraps all responses in envelopes:

```rust
// Index/batch: { status, results, objs: [...] }
pub(crate) struct IndexEnvelope<T> {
    pub status: String,
    pub results: usize,
    pub objs: Vec<T>,
}

// Single object: { status, results, obj: {...} }
pub(crate) struct GetEnvelope<T> {
    pub status: String,
    pub results: usize,
    pub obj: T,
}

// Error: { status, message }
pub(crate) struct ErrorEnvelope {
    pub status: String,
    pub message: String,
}
```

### Client Methods

| Method | Returns | Envelope | Use Case |
|--------|---------|----------|----------|
| `get_one<T>(path)` | `T` | `{ obj }` | Single resource by ID |
| `get_many<T, Q>(path, query)` | `Vec<T>` | `{ objs }` | List with query params |
| `get_bytes(path)` | `Bytes` | None | Binary content (asset streaming) |
| `post_one<T, B>(path, body)` | `T` | `{ obj }` | Create single |
| `post_many<T, B>(path, body)` | `Vec<T>` | `{ objs }` | Batch create |
| `post_multipart<T>(path, form)` | `T` | Parsed JSON | Tarball ingest |
| `post_raw<B>(path, body)` | `Value` | None | Non-standard responses |
| `post_bytes<B>(path, body)` | `Bytes` | None | PDF render |
| `patch_many<T, B>(path, body)` | `Vec<T>` | `{ objs }` | Batch update |
| `patch_multipart<T>(path, form)` | `T` | Parsed JSON | Version ingest |
| `delete_one<T>(path)` | `T` | `{ obj }` | Single delete |
| `delete_many<T, Q>(path, query)` | `Vec<T>` | `{ objs }` | Batch delete (query) |
| `delete_many_with_body<T, B>(path, body)` | `Vec<T>` | `{ objs }` | Batch delete (body) |

### Query Serialization

The API expects bracket-notation query strings for tagged enum queries:

```text
GET /api/journeys/?query=by_user&user_id=<uuid>&options[limit]=10&options[page]=1
```

The `query::bracket_serialize()` function uses `serde_qs` to produce this format from any `Serialize` type. Resource clients pass their `Get*SetSchema` enum directly to `get_many()`, which handles serialization transparently.

## Feature Flags

| Feature | Dependencies | Enables |
|---------|-------------|---------|
| `tarball` | `tar`, `flate2` | `GuidesClient::ingest_from_dir()` -- creates `.tar.gz` in memory from a directory |
| `service-auth` | `ed25519-dalek`, `rand`, `tokio` | `DappleClient::with_service_token()` -- Ed25519 service token exchange |

Neither feature is enabled by default. Enable them in `Cargo.toml`:

```toml
[dependencies]
dapple-sdk = { path = "../dapple-sdk", features = ["tarball", "service-auth"] }
```

## Non-Standard Endpoints

Most resource clients follow the standard CRUD pattern with JSON envelopes. Several endpoints deviate:

### Multipart (Guide Ingest)

`GuidesClient::ingest()` and `ingest_version()` POST a `.tar.gz` file as multipart form data. The `post_multipart()` client method constructs the request with `reqwest::multipart::Form` and parses the JSON response directly (not through the standard envelope).

### Binary (PDF Render)

`GuidesClient::render_pdf()` returns raw PDF bytes via `post_bytes()`. The response is not JSON-wrapped -- the body is the PDF content itself.

### Binary (Asset Content)

`AssetsClient` and `DraftsClient` support streaming binary content via `get_bytes()`. The response is raw bytes with appropriate `Content-Type` headers.

### Non-Envelope JSON (API Tokens)

`ApiTokensClient` uses `post_raw()` because the API token endpoints return non-standard response shapes (e.g., `{ status, token, name, prefix, object_id }`) that do not follow the `{ obj }` or `{ objs }` envelope convention.

## Next Steps

- [Adding Resource Clients](./adding-resource-clients.md) -- Step-by-step guide for extending the SDK with new resource clients
