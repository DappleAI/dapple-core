# dapple-sdk Architecture Plan

**Status:** Draft
**Created:** 2026-02-15
**Last Updated:** 2026-02-15
**Target Version:** 0.1.0
**Dependencies:** dapple-prompt-api (OpenAPI spec generation via aide)

---

## 1. Executive Summary

The `dapple-sdk` crate provides a Rust HTTP client for the Dapple Prompt API. It lives at `crates/dapple-sdk/` inside the workspace monorepo and has **zero** dependency on `dapple-data`, `sqlx`, or any database infrastructure. The SDK depends only on HTTP-level concerns: `reqwest`, `serde`, `uuid`, and its own DTO types.

This plan recommends a **hybrid approach** to code generation: auto-generate DTO types from the OpenAPI specification at build time using a `build.rs` codegen step, then hand-author the resource client modules that provide ergonomic RPC-style methods. This leverages the same OpenAPI pipeline that `dapple-ui` already uses (fetching `/api.json` from the running API or from a checked-in spec snapshot), eliminates the ~30 hand-written type modules that would otherwise drift from the API, and preserves the SDK's ergonomic resource-client surface.

The primary consumers are:
- **dapple-prompt-flow** -- workflow engine calling back into the API
- **dapple-prompt-conduit** -- conduit service making internal API calls
- **External Rust integrators** -- third-party Rust applications consuming the public API
- **Integration tests** -- replacing raw `reqwest` calls in the lifecycle test suite

## 2. Context and Background

### Current State

The API surface is large: 71 path entries, 305 component schemas in the OpenAPI 3.1 specification. The spec is generated at runtime by `aide` + `schemars` from the Rust handler and schema types, served at `GET /api.json`, and post-processed by `openapi_fixups::add_discriminators()` to inject proper `discriminator` metadata for tagged union schemas.

The existing `dapple-ui` TypeScript workspace already has a working OpenAPI code-generation pipeline:

1. **Fetch**: `scripts/fetch-internal-openapi-spec.ts` downloads `/api.json` from the running API and saves it to `openapi/internal-api.json`.
2. **Fix**: `scripts/generate-internal-openapi-types.ts` patches `$defs` references (an aide/schemars quirk where parameter-level schemas use `#/$defs/` instead of `#/components/schemas/`), moves them into `components/schemas`, and rewrites references.
3. **Generate**: `@hey-api/openapi-ts` with `@hey-api/typescript` plugin produces `types/internal-api/types.gen.ts` (210KB, all 305 schemas as TypeScript interfaces).
4. **Consume**: `lib/internalApiClient.ts` is a hand-written generic HTTP client that uses these generated types for type annotations. It handles Ed25519 service token signing, JWT caching, and 401 retry.

The API server also exposes `build_openapi_spec()` as a public function (no runtime state required) used by `dapple-prompt-mock-api` to serve the same spec without a database.

A skeleton `crates/dapple-sdk/` already exists with ~30 hand-written type modules and ~28 resource client modules. These lack a `Cargo.toml` and `lib.rs`, so nothing compiles yet. The hand-written types are reasonable but already show drift from the actual API schemas (e.g., the SDK playbook types define `PlaybookStageReference` and `PlaybookCategoryReference` while the actual API returns `PlaybookStage`, `PlaybookStep`, `PlaybookTask` with nested structure).

### Problem Statement

Hand-maintaining ~30 type modules (each mirroring DTO schemas from `dapple-data` and request/response schemas from `dapple-prompt-api`) is fragile. Every new endpoint, schema change, or field addition requires updating both the API and the SDK in lockstep. The TypeScript side solved this with `@hey-api/openapi-ts`; the Rust side should solve it analogously.

### Key Constraints

- The SDK must **not** depend on `dapple-data` or `sqlx` (no database types leaking into the client).
- The SDK must support three auth modes: API tokens (`dpl_` prefix), service tokens (`dps_` prefix via Ed25519), and delegated JWTs.
- The API uses bracket-notation query parameter serialization for tagged enum queries.
- Response envelopes differ by verb: `{ status, results, objs }` for index/post, `{ status, results, obj }` for get, with error variants `{ status, message }`.
- Some endpoints are non-standard: multipart tarball ingest, PDF render returning binary bytes, favorites endpoints, streaming asset content.

## 3. Investigation Findings

### OpenAPI Spec Quality Assessment

The generated spec is OpenAPI 3.1.0. Key observations:

- **Tagged unions**: schemars 0.9 generates `oneOf` with `const` tag properties. The `openapi_fixups::add_discriminators()` post-processor injects proper `discriminator` metadata with `propertyName` and `mapping`. This is critical for code generators to produce correct Rust enums.
- **`$defs` quirk**: Parameter-level schemas (from `schemars::schema_for!`) use `#/$defs/` references instead of `#/components/schemas/`. The TypeScript pipeline fixes this with a pre-processing step. A Rust codegen pipeline will need the same fix.
- **`$schema` in parameters**: Some parameter schemas include a `$schema` key, which is non-standard for OpenAPI and may confuse code generators. This should be stripped in pre-processing.
- **Generic response wrappers**: The spec includes many `GetResponse_for_*`, `PostResponse_for_*`, `IndexResponse_for_*`, `DeleteResponse_for_*` schemas. These are schemars-generated names for Rust generic structs like `GetResponse<Journey>`. A code generator will produce types named `GetResponseForJourney`, etc.
- **Operation IDs**: Present and meaningful (e.g., `playbooksIndex`, `journeysPost`). These can drive method naming in a fully-generated client.

### Rust OpenAPI Code Generator Landscape

| Tool | Approach | Maturity | Fit |
|------|----------|----------|-----|
| **progenitor** (Oxide) | Generates complete Rust client from OpenAPI 3.0 | Production-grade, used by Oxide cloud | Generates both types AND client methods. Opinionated about client structure. May not handle the bracket-notation query params or envelope unwrapping. |
| **openapi-generator** (Java-based) | Mustache template codegen for many languages | Very mature, Rust template exists | Heavy Java dependency, generated Rust code is often not idiomatic. |
| **typify** (Oxide) | Generates just types (structs/enums) from JSON Schema | Stable, actively maintained | Perfect for types-only generation. Used internally by progenitor. |
| **Custom build.rs** | Parse OpenAPI JSON, emit `serde` structs | Full control | Most effort, but highest fidelity to existing patterns. |

### Recommendation: typify for Types + Hand-Written Resource Clients

**typify** (https://github.com/oxidecomputer/typify) is the best fit because:

1. It generates **only types** -- exactly what we need. The resource client layer needs hand-authoring anyway because of the envelope unwrapping, bracket-notation query params, and non-standard endpoints (multipart, binary streaming).
2. It handles JSON Schema features used by schemars/aide: `oneOf` with discriminators, `anyOf` for `Option<T>`, `$ref` resolution, format specifiers (`uuid`, `date-time`).
3. It produces idiomatic Rust: `#[derive(Clone, Debug, Serialize, Deserialize)]` structs with proper `serde` attributes.
4. It is maintained by Oxide (same team as progenitor) and is battle-tested.
5. It integrates cleanly as a `build.rs` step or a standalone codegen script.

**progenitor** was considered but rejected because:
- It generates the full client, which would conflict with the hand-written resource client layer that handles envelope unwrapping and bracket-notation.
- It assumes standard REST conventions (JSON body responses, standard query params) that do not match the Dapple API's envelope + RPC-query pattern.
- Forking or configuring progenitor to handle these patterns would be more work than hand-writing the ~28 resource client modules.

## 4. Architecture

### Crate Layout

```
crates/dapple-sdk/
+-- Cargo.toml
+-- build.rs                          # Codegen: typify from OpenAPI spec
+-- openapi/
|   +-- internal-api.json             # Checked-in snapshot of the API spec
|   +-- README.md                     # Instructions for updating the spec
+-- src/
    +-- lib.rs                        # DappleClient, SdkError, re-exports
    +-- client.rs                     # Core HTTP client (reqwest wrapper)
    +-- auth/
    |   +-- mod.rs
    |   +-- api_token.rs              # dpl_ token auth
    |   +-- service_token.rs          # dps_ Ed25519 service auth
    |   +-- jwt.rs                    # JWT delegated auth
    +-- envelope.rs                   # Response envelope unwrapping
    +-- query.rs                      # Bracket-notation query serialization
    +-- generated.rs                  # include!(concat!(env!("OUT_DIR"), "/types.rs"))
    +-- types.rs                      # Re-exports from generated + manual additions
    +-- resource/
        +-- mod.rs                    # Resource client module registry
        +-- playbooks.rs             # PlaybooksClient
        +-- journeys.rs              # JourneysClient
        +-- guides.rs                # GuidesClient (with ingest, render, cards, favorites)
        +-- dispatches.rs            # DispatchesClient (with task attempts)
        +-- briefs.rs                # BriefsClient
        +-- brief_groups.rs          # BriefGroupsClient
        +-- brief_packs.rs           # BriefPacksClient
        +-- brief_units.rs           # BriefUnitsClient
        +-- steps.rs                 # StepsClient
        +-- stages.rs                # StagesClient
        +-- tasks.rs                 # TasksClient
        +-- categories.rs            # CategoriesClient
        +-- hats.rs                  # HatsClient
        +-- assets.rs                # AssetsClient (with streaming content)
        +-- drafts.rs                # DraftsClient
        +-- fluent_messages.rs       # FluentMessagesClient
        +-- prompts.rs               # PromptsClient
        +-- directives.rs            # DirectivesClient
        +-- value_slots.rs           # ValueSlotsClient
        +-- inference_profiles.rs    # InferenceProfilesClient
        +-- deprecations.rs          # DeprecationsClient
        +-- dispatch_intent_bindings.rs
        +-- dispatch_phases.rs       # DispatchPhasesClient
        +-- dispatch_task_attempts.rs
        +-- user_details.rs          # UserDetailsClient
        +-- api_tokens.rs            # ApiTokensClient (CRUD for dpl_ tokens)
        +-- shared_content.rs        # SharedContentClient (ingest)
        +-- auth_token.rs            # AuthTokenClient (get-auth-token)
        +-- service_token.rs         # ServiceTokenClient (get-service-token)
```

### OpenAPI Spec Management

The OpenAPI spec is managed as a checked-in snapshot, following the same pattern as `dapple-ui/packages/ui/openapi/internal-api.json`:

1. **Refresh**: A developer runs a script (or the existing TypeScript `fetch-internal-openapi-spec.ts`, or a new Rust binary) that hits the running API's `/api.json` endpoint and writes the result to `crates/dapple-sdk/openapi/internal-api.json`.
2. **Pre-process**: The `build.rs` script reads the spec, applies the same `$defs` fixup that the TypeScript pipeline applies (moving `$defs` to `components/schemas`, rewriting `#/$defs/` references), and strips `$schema` keys from parameter schemas.
3. **Generate**: `typify` converts the cleaned-up `components/schemas` into Rust structs/enums and writes them to `$OUT_DIR/types.rs`.
4. **Include**: `src/generated.rs` pulls in the generated code via `include!`.
5. **Re-export**: `src/types.rs` re-exports from `generated` and adds any manual type aliases or helper implementations.

**Alternative spec source**: Since `dapple-prompt-api::build_openapi_spec()` is already a public function, a workspace-internal codegen binary could generate the spec without a running server. However, this would require the SDK crate to depend on `dapple-prompt-api` at build time (transitively pulling in `dapple-data`, `sqlx`, `aide`, etc.), which defeats the purpose of SDK independence. The checked-in snapshot approach keeps the build lightweight.

### Type Generation Strategy

#### What typify generates

From the 305 component schemas, typify will produce approximately:

- **~40 DTO structs**: `Journey`, `Playbook`, `Guide`, `Brief`, `BriefGroup`, `BriefPack`, `Dispatch`, `Asset`, etc.
- **~60 Create/Patch schemas**: `CreateJourneySchema`, `CreatePlaybookSetSchema`, `PatchBriefSchema`, etc.
- **~15 tagged enums**: `GetJourneySetSchema`, `GetPlaybookSetSchema`, `BriefUnitSubtype`, `MediaSource`, etc.
- **~80 response wrappers**: `GetResponseForJourney`, `PostResponseForPlaybook`, `IndexResponseForCategory`, etc.
- **~100 ancillary types**: Refs, nested sub-schemas, OpenAPI meta-types.

#### What we filter out

The response wrapper types (`GetResponse_for_*`, `PostResponse_for_*`, etc.) are not needed by the SDK because the resource clients unwrap envelopes internally. The OpenAPI meta-types (`OpenApi`, `PathItem`, `Operation`, `Parameter`, etc.) are also noise -- these are schema descriptions of the spec itself that aide exposes.

The `build.rs` script will configure typify to either:
- (a) Generate everything and let `src/types.rs` selectively re-export only the useful types, or
- (b) Pre-filter the schemas fed to typify, excluding names matching `^(Get|Post|Index|Delete)Response_for_`, `^(Get|Post|Index|Delete)ErrorResponse$`, and OpenAPI meta-types.

Option (b) is preferred to keep compile times down and avoid generating ~180 types that are never used.

#### Manual type additions

A small number of types do not appear in the OpenAPI schema and must be defined manually:

```rust
// src/types.rs (manual additions alongside generated re-exports)

/// Common filter options for list queries.
/// Note: FilterOptions may be generated by typify if it appears in components/schemas.
/// If not (because it's in $defs), define it manually here.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FilterOptions {
    pub limit: Option<i64>,
    pub page: Option<i64>,
}

/// Parameters for guide ingest operations.
#[derive(Clone, Debug, Default)]
pub struct IngestParams {
    pub dry_run: bool,
    pub source_path: Option<String>,
}

/// Response from guide ingest (dry_run=true or actual ingest).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IngestGuideResponse {
    pub status: String,
    // Fields vary between dry_run and actual ingest
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

/// Parameters for delete operations.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeleteQueryParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}
```

### build.rs Codegen Pipeline

```rust
// crates/dapple-sdk/build.rs
use std::path::Path;

fn main() {
    let spec_path = Path::new("openapi/internal-api.json");
    println!("cargo::rerun-if-changed=openapi/internal-api.json");

    // 1. Read and parse the OpenAPI spec
    let spec_text = std::fs::read_to_string(spec_path)
        .expect("Failed to read OpenAPI spec");
    let mut spec: serde_json::Value = serde_json::from_str(&spec_text)
        .expect("Failed to parse OpenAPI spec");

    // 2. Apply $defs fixup (same logic as dapple-ui's generate-internal-openapi-types.ts)
    let components_schemas = spec
        .pointer_mut("/components/schemas")
        .expect("Missing components/schemas");
    fix_defs_references(&mut spec);

    // 3. Filter out unwanted schemas (response wrappers, OpenAPI meta-types)
    filter_schemas(&mut spec);

    // 4. Feed to typify
    let mut type_space = typify::TypeSpace::default();
    // Configure typify settings...
    let schema = serde_json::from_value(spec["components"]["schemas"].clone())
        .expect("Failed to parse schemas");
    type_space.add_root_schema(schema).unwrap();

    // 5. Write to OUT_DIR
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("types.rs");
    let content = type_space.to_stream().to_string();
    std::fs::write(&out_path, content).unwrap();
}
```

Note: The exact typify API calls will depend on the version used. The above is illustrative. The implementation agent should consult `typify`'s documentation for the precise API (it may use `TypeSpace::new()`, `add_ref_types()`, or a JSON Schema root document approach).

### Core Client Architecture

#### DappleClient

```rust
// src/lib.rs

pub struct DappleClient {
    http: reqwest::Client,
    base_url: String,
    auth: Box<dyn AuthProvider + Send + Sync>,
}

impl DappleClient {
    /// Create a client with API token authentication (dpl_ prefix).
    pub fn with_api_token(base_url: &str, token: &str) -> Self { ... }

    /// Create a client with service token authentication (dps_ prefix).
    #[cfg(feature = "service-auth")]
    pub fn with_service_token(base_url: &str, config: ServiceTokenConfig) -> Self { ... }

    /// Create a client with a raw Bearer token (pre-obtained JWT).
    pub fn with_bearer_token(base_url: &str, token: &str) -> Self { ... }

    // Resource client accessors
    pub fn playbooks(&self) -> PlaybooksClient<'_> { PlaybooksClient::new(self) }
    pub fn journeys(&self) -> JourneysClient<'_> { JourneysClient::new(self) }
    pub fn guides(&self) -> GuidesClient<'_> { GuidesClient::new(self) }
    pub fn dispatches(&self) -> DispatchesClient<'_> { DispatchesClient::new(self) }
    pub fn briefs(&self) -> BriefsClient<'_> { BriefsClient::new(self) }
    // ... all resource accessors
}
```

#### AuthProvider Trait

```rust
// src/auth/mod.rs

#[async_trait]
pub(crate) trait AuthProvider {
    /// Get the Authorization header value for a request.
    async fn authorization_header(&self) -> Result<String, SdkError>;

    /// Notify that the server returned 401 (token may need refresh).
    async fn on_unauthorized(&self);
}

// Implementations:
// - ApiTokenAuth: static "Bearer dpl_..." header
// - BearerAuth: static "Bearer <jwt>" header
// - ServiceTokenAuth: signs Ed25519 token, exchanges for JWT, caches with TTL
```

#### Response Envelope Handling

The API uses a consistent envelope pattern. The SDK unwraps it transparently:

```rust
// src/envelope.rs

/// Envelope for index/list responses: { status, results, objs }
#[derive(Deserialize)]
struct IndexEnvelope<T> {
    status: String,
    results: usize,
    objs: Vec<T>,
}

/// Envelope for single-object GET responses: { status, results, obj }
#[derive(Deserialize)]
struct GetEnvelope<T> {
    status: String,
    results: usize,
    obj: T,
}

/// Error envelope: { status, message }
#[derive(Deserialize)]
struct ErrorEnvelope {
    status: String,
    message: String,
}
```

The core HTTP methods parse the response body and unwrap:

```rust
// src/client.rs (internal methods on DappleClient)

impl DappleClient {
    /// GET single object: unwraps { obj } envelope.
    pub(crate) async fn get_one<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, SdkError> {
        let resp = self.request(Method::GET, path).send().await?;
        let envelope: GetEnvelope<T> = self.handle_response(resp).await?;
        Ok(envelope.obj)
    }

    /// GET list with query params: unwraps { objs } envelope.
    pub(crate) async fn get_many<T: DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<Vec<T>, SdkError> {
        let qs = bracket_serialize(query)?;
        let resp = self.request(Method::GET, &format!("{path}?{qs}"))
            .send().await?;
        let envelope: IndexEnvelope<T> = self.handle_response(resp).await?;
        Ok(envelope.objs)
    }

    /// POST batch: unwraps { objs } envelope.
    pub(crate) async fn post_many<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<Vec<T>, SdkError> {
        let resp = self.request(Method::POST, path)
            .json(body)
            .send().await?;
        let envelope: IndexEnvelope<T> = self.handle_response(resp).await?;
        Ok(envelope.objs)
    }

    /// POST multipart (for tarball ingest).
    pub(crate) async fn post_multipart<T: DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<T, SdkError> { ... }

    /// POST returning raw bytes (for PDF render).
    pub(crate) async fn post_bytes<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<bytes::Bytes, SdkError> { ... }

    // PATCH, DELETE follow similar patterns
}
```

#### Bracket-Notation Query Serialization

The API expects tagged enum queries serialized with bracket notation in query parameters:

```
GET /api/journeys/?query=by_user&user_id=<uuid>&options[limit]=10&options[page]=1
```

```rust
// src/query.rs

/// Serialize a value into bracket-notation query string.
/// E.g., GetJourneySetSchema::ByUser { user_id, options: Some(FilterOptions { limit: 10 }) }
/// becomes: "query=by_user&user_id=<uuid>&options[limit]=10"
pub fn bracket_serialize<T: Serialize>(value: &T) -> Result<String, SdkError> {
    // Use serde_qs or a custom serializer that produces bracket notation.
    serde_qs::to_string(value).map_err(|e| SdkError::Serialization(e.to_string()))
}
```

### SdkError

```rust
// src/lib.rs

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
}
```

### Resource Client Pattern

Each resource client follows a consistent structure. Taking `PlaybooksClient` as an example:

```rust
// src/resource/playbooks.rs

use crate::DappleClient;
use crate::SdkError;
use crate::types::{
    CreatePlaybookSchema, CreatePlaybookSetSchema,
    GetPlaybookSetSchema, PatchPlaybookSchema, PatchPlaybookSetSchema,
    Playbook, FilterOptions, DeleteQueryParams,
};
use uuid::Uuid;

pub struct PlaybooksClient<'a> {
    client: &'a DappleClient,
}

impl<'a> PlaybooksClient<'a> {
    pub(crate) fn new(client: &'a DappleClient) -> Self {
        Self { client }
    }

    // --- Single-object GET ---

    pub async fn get(&self, id: Uuid) -> Result<Playbook, SdkError> {
        self.client.get_one(&format!("api/playbooks/{id}")).await
    }

    // --- List queries ---

    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Playbook>, SdkError> {
        let query = GetPlaybookSetSchema::All { options };
        self.client.get_many("api/playbooks/", &query).await
    }

    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Playbook>, SdkError> {
        let query = GetPlaybookSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/playbooks/", &query).await
    }

    pub async fn list_by_slugs(
        &self,
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Playbook>, SdkError> {
        let query = GetPlaybookSetSchema::BySlugs { slugs, options };
        self.client.get_many("api/playbooks/", &query).await
    }

    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Playbook>, SdkError> {
        let query = GetPlaybookSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/playbooks/", &query).await
    }

    // --- Create ---

    pub async fn create(
        &self,
        objs: Vec<CreatePlaybookSchema>,
    ) -> Result<Vec<Playbook>, SdkError> {
        let body = CreatePlaybookSetSchema { objs };
        self.client.post_many("api/playbooks/", &body).await
    }

    // --- Patch (creates new version) ---

    pub async fn patch(
        &self,
        objs: Vec<PatchPlaybookSchema>,
    ) -> Result<Vec<Playbook>, SdkError> {
        let body = PatchPlaybookSetSchema { objs };
        self.client.patch_many("api/playbooks/", &body).await
    }

    // --- Delete ---

    pub async fn delete(
        &self,
        path_object_ids: Vec<Uuid>,
        level: Option<String>,
    ) -> Result<Vec<Playbook>, SdkError> {
        let query = DeleteQueryParams { level };
        self.client.delete_many("api/playbooks/", &query).await
    }

    pub async fn delete_one(&self, id: Uuid) -> Result<Playbook, SdkError> {
        self.client.delete_one(&format!("api/playbooks/{id}")).await
    }
}
```

### Non-Standard Endpoints

#### Guide Ingest (Multipart Tarball)

```rust
// src/resource/guides.rs (partial)

impl<'a> GuidesClient<'a> {
    pub async fn ingest(
        &self,
        tarball_bytes: Vec<u8>,
        params: IngestParams,
    ) -> Result<IngestGuideResponse, SdkError> {
        let path = build_ingest_path("api/guides/ingest", &params);
        let part = reqwest::multipart::Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")?;
        let form = reqwest::multipart::Form::new().part("file", part);
        self.client.post_multipart(&path, form).await
    }

    #[cfg(feature = "tarball")]
    pub async fn ingest_from_dir(
        &self,
        dir: &std::path::Path,
        params: IngestParams,
    ) -> Result<IngestGuideResponse, SdkError> {
        let tarball_bytes = create_tarball_from_dir(dir)?;
        self.ingest(tarball_bytes, params).await
    }

    pub async fn ingest_version(
        &self,
        tarball_bytes: Vec<u8>,
        params: IngestParams,
    ) -> Result<IngestGuideResponse, SdkError> {
        let path = build_ingest_path("api/guides/ingest", &params);
        let part = reqwest::multipart::Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")?;
        let form = reqwest::multipart::Form::new().part("file", part);
        self.client.patch_multipart(&path, form).await
    }

    pub async fn render_pdf(
        &self,
        guide_id: Uuid,
        profile: &str,
    ) -> Result<bytes::Bytes, SdkError> {
        #[derive(serde::Serialize)]
        struct RenderBody<'a> { profile: &'a str }
        let body = RenderBody { profile };
        self.client.post_bytes(&format!("api/guides/{guide_id}/render"), &body).await
    }
}
```

#### Asset Content Streaming

```rust
// src/resource/assets.rs (partial)

impl<'a> AssetsClient<'a> {
    pub async fn content(
        &self,
        id: Uuid,
    ) -> Result<bytes::Bytes, SdkError> {
        self.client.get_bytes(&format!("api/assets/{id}/content")).await
    }

    pub async fn content_stream(
        &self,
        id: Uuid,
    ) -> Result<impl futures::Stream<Item = Result<bytes::Bytes, SdkError>>, SdkError> {
        self.client.get_stream(&format!("api/assets/{id}/content")).await
    }
}
```

#### Favorites

```rust
// src/resource/guides.rs (partial)

impl<'a> GuidesClient<'a> {
    pub async fn favorite(&self, guide_id: Uuid) -> Result<AddFavoriteGuideResponse, SdkError> {
        self.client.post_one(&format!("api/guides/{guide_id}/favorite"), &()).await
    }

    pub async fn unfavorite(&self, guide_id: Uuid) -> Result<RemoveFavoriteGuideResponse, SdkError> {
        self.client.delete_one(&format!("api/guides/{guide_id}/favorite")).await
    }

    pub async fn is_favorite(&self, guide_id: Uuid) -> Result<CheckFavoriteGuideResponse, SdkError> {
        self.client.get_one(&format!("api/guides/{guide_id}/favorite")).await
    }

    pub async fn favorites(&self) -> Result<ListFavoriteGuidesResponse, SdkError> {
        self.client.get_one("api/guides/favorites").await
    }
}
```

### Cargo.toml

```toml
[package]
name = "dapple-sdk"
version = "0.1.0"
edition = "2024"
description = "Rust SDK for the Dapple Prompt API"

[features]
default = []
tarball = ["dep:tar", "dep:flate2"]
service-auth = ["dep:ed25519-dalek", "dep:rand"]
stream = ["dep:futures"]

[dependencies]
reqwest = { version = "0.12", features = ["json", "multipart"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_qs = "0.13"
uuid = { version = "1", features = ["serde"] }
thiserror = "2"
async-trait = "0.1"
url = "2"
bytes = "1"
log = "0.4"

# Optional: tarball creation for guide ingest from directory
tar = { version = "0.4", optional = true }
flate2 = { version = "1", optional = true }

# Optional: Ed25519 service token signing
ed25519-dalek = { version = "2", features = ["rand_core"], optional = true }
rand = { version = "0.8", optional = true }

# Optional: streaming support
futures = { version = "0.3", optional = true }

[build-dependencies]
typify = "0.2"
serde_json = "1"
```

Note: The exact `typify` version should be pinned to whatever is current at implementation time. The implementation agent should verify the API.

## 5. Pre-Processing the OpenAPI Spec (build.rs Detail)

The `build.rs` script needs to handle three known issues with the aide/schemars-generated spec:

### Issue 1: `$defs` References

Parameter schemas generated by `schemars::schema_for!` produce local `$defs` that reference types like `FilterOptions`:

```json
{
  "parameters": [{
    "schema": {
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "$defs": {
        "FilterOptions": { "type": "object", ... }
      },
      "oneOf": [
        { "$ref": "#/$defs/FilterOptions" }
      ]
    }
  }]
}
```

**Fix**: Walk the entire spec. For any object with `$defs`, move each definition into `components/schemas` (if not already present), then rewrite all `#/$defs/Foo` references to `#/components/schemas/Foo`, and remove the `$defs` key.

### Issue 2: `$schema` Keys

Some schemas include `"$schema": "https://json-schema.org/draft/2020-12/schema"` which is valid JSON Schema but not expected in OpenAPI components. This can confuse typify.

**Fix**: Strip all `$schema` keys from the spec.

### Issue 3: Schema Filtering

The spec includes ~80 response wrapper types (`GetResponse_for_*`, etc.) and ~20 OpenAPI meta-types (`OpenApi`, `PathItem`, `Operation`, etc.) that the SDK does not need. These inflate compile times and clutter the generated module.

**Fix**: Before feeding to typify, remove schemas whose names match:
- `^(Get|Post|Index|Delete)Response_for_`
- `^(Get|Post|Index|Delete)ErrorResponse$`
- `^PostErrorResponse$`
- OpenAPI meta-types: `OpenApi`, `PathItem`, `Paths`, `Operation`, `Parameter`, `Response`, `Responses`, `RequestBody`, `Header`, `Tag`, `Link`, `Example`, `Encoding`, `MediaType`, `Server`, `ServerVariable`, `SecurityScheme`, `OAuth2Flows`, `Components`, `Info`, `Contact`, `License`, `ExternalDocumentation`, `SchemaObject`, `ReferenceOr_for_*`, `ApiKeyLocation`, `CookieStyle`, `HeaderStyle`, `PathStyle`, `QueryStyle`
- `HealthResponse` (not needed by SDK)

This should reduce the generated types from ~305 to ~100-120 meaningful types.

## 6. Spec Refresh Workflow

Developers update the checked-in spec when the API changes:

```bash
# Option 1: From running API (requires API server at the configured URL)
curl -s https://dapple-prompt-api.kube.dappleai.dev/api.json | python3 -m json.tool > crates/dapple-sdk/openapi/internal-api.json

# Option 2: Using the existing dapple-ui script (if node/tsx is available)
cd ../dapple-ui/packages/ui && pnpm run internal-api:fetch-spec
cp openapi/internal-api.json ../../dapple-prompt/crates/dapple-sdk/openapi/

# Option 3: From the mock API binary (no database needed)
cargo run --bin dapple-prompt-mock-api &
curl -s http://localhost:8082/api.json | python3 -m json.tool > crates/dapple-sdk/openapi/internal-api.json
```

After updating:
```bash
cargo build -p dapple-sdk  # triggers build.rs, regenerates types
cargo test -p dapple-sdk    # verify nothing broke
```

The spec file should be committed to version control so that CI builds do not require a running API server.

## 7. Testing Strategy

### Unit Tests

Located in `crates/dapple-sdk/src/` as `#[cfg(test)]` modules:

- **Query serialization**: Verify bracket-notation output for each `Get*SetSchema` variant.
- **Envelope unwrapping**: Verify that `IndexEnvelope`, `GetEnvelope`, and error envelopes parse correctly.
- **Auth providers**: Verify API token header format, bearer token format.

### Integration Tests Against Mock Server

The SDK can be tested against `dapple-prompt-mock-api` which already exists and serves the real OpenAPI spec:

```rust
// tests/integration.rs
use dapple_prompt_mock_api::{Fixture, spawn_mock_server};
use dapple_sdk::DappleClient;

#[tokio::test]
async fn test_playbooks_list() {
    let fixture = Fixture::default().with_playbooks(2);
    let (base_url, _handle) = spawn_mock_server(fixture).await;

    let client = DappleClient::with_bearer_token(&base_url, "test-token");
    let playbooks = client.playbooks().list(None).await.unwrap();
    assert_eq!(playbooks.len(), 2);
}
```

This approach avoids needing a real database for SDK tests while still exercising the full HTTP round-trip.

### Type Compatibility Tests

A build-time or test-time check that the generated types can deserialize actual API responses:

```rust
#[test]
fn test_journey_deserialize() {
    let json = r#"{
        "object_id": "019463f4-0000-7000-8000-000000000001",
        "name": "Test Journey",
        "user_object_id": "019463f4-0000-7000-8000-000000000002",
        "playbook_object_id": "019463f4-0000-7000-8000-000000000003",
        "brief_packs": [],
        "inference_profiles": [],
        "created_at": "2026-01-15T12:00:00Z"
    }"#;
    let journey: Journey = serde_json::from_str(json).unwrap();
    assert_eq!(journey.name, "Test Journey");
}
```

## 8. Implementation Steps

### Phase 1: Foundation (build.rs + core client)

1. **Create `Cargo.toml`** with all dependencies listed above.
2. **Copy the OpenAPI spec** from `dapple-ui/packages/ui/openapi/internal-api.json` to `crates/dapple-sdk/openapi/internal-api.json`.
3. **Write `build.rs`** with the $defs fixup, schema filtering, and typify codegen.
4. **Write `src/generated.rs`** with `include!` directive.
5. **Write `src/types.rs`** with selective re-exports and manual additions.
6. **Write `src/lib.rs`** with `DappleClient`, `SdkError`, module declarations.
7. **Write `src/client.rs`** with the core HTTP methods (get_one, get_many, post_many, etc.).
8. **Write `src/envelope.rs`** with the response envelope types and unwrapping logic.
9. **Write `src/query.rs`** with bracket-notation serialization.
10. **Write `src/auth/mod.rs`** with the `AuthProvider` trait and `ApiTokenAuth` + `BearerAuth` implementations.
11. **Verify `cargo build -p dapple-sdk` succeeds**.

### Phase 2: Resource Clients

Implement resource client modules in priority order (most-used first):

1. `playbooks.rs`, `journeys.rs` -- most common operations
2. `guides.rs` -- complex (ingest, render, cards, favorites)
3. `dispatches.rs` -- complex (task attempts sub-resource)
4. `briefs.rs`, `brief_groups.rs`, `brief_packs.rs`, `brief_units.rs`
5. `steps.rs`, `stages.rs`, `tasks.rs`
6. `categories.rs`, `hats.rs`
7. `assets.rs`, `drafts.rs` -- streaming support
8. `fluent_messages.rs`, `prompts.rs`, `directives.rs`, `value_slots.rs`
9. `inference_profiles.rs`, `deprecations.rs`
10. `dispatch_intent_bindings.rs`, `dispatch_phases.rs`, `dispatch_task_attempts.rs`
11. `user_details.rs`, `api_tokens.rs`
12. `shared_content.rs`, `auth_token.rs`, `service_token.rs`

### Phase 3: Service Auth

1. **Write `src/auth/service_token.rs`** with Ed25519 signing and JWT exchange (behind `service-auth` feature).
2. **Write `src/auth/jwt.rs`** with JWT caching and refresh logic.

### Phase 4: Integration Tests

1. **Write tests against `dapple-prompt-mock-api`** for each resource client.
2. **Add type-compatibility snapshot tests** using real API response samples.

### Phase 5: Adoption

1. **Migrate `dapple-prompt-flow`** from raw `reqwest` to `dapple-sdk`.
2. **Migrate integration test consumers** to use the SDK.
3. **Update AGENTS.md** with SDK usage guidance.

## 9. Files to Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-sdk/Cargo.toml` | Create | Package manifest with dependencies |
| `crates/dapple-sdk/build.rs` | Create | OpenAPI spec preprocessing + typify codegen |
| `crates/dapple-sdk/openapi/internal-api.json` | Create (copy) | Checked-in API spec snapshot |
| `crates/dapple-sdk/openapi/README.md` | Create | Instructions for refreshing the spec |
| `crates/dapple-sdk/src/lib.rs` | Create | Crate root with DappleClient and re-exports |
| `crates/dapple-sdk/src/client.rs` | Create | Core HTTP methods |
| `crates/dapple-sdk/src/envelope.rs` | Create | Response envelope types and unwrapping |
| `crates/dapple-sdk/src/query.rs` | Create | Bracket-notation query serialization |
| `crates/dapple-sdk/src/generated.rs` | Create | include! for typify output |
| `crates/dapple-sdk/src/types.rs` | Rewrite | Re-exports from generated + manual types |
| `crates/dapple-sdk/src/auth/mod.rs` | Create | AuthProvider trait and implementations |
| `crates/dapple-sdk/src/auth/api_token.rs` | Create | dpl_ token auth provider |
| `crates/dapple-sdk/src/auth/service_token.rs` | Create | dps_ Ed25519 auth provider (optional) |
| `crates/dapple-sdk/src/auth/jwt.rs` | Create | JWT caching and refresh |
| `crates/dapple-sdk/src/resource/mod.rs` | Rewrite | Module declarations only |
| `crates/dapple-sdk/src/resource/*.rs` | Rewrite | All 28 resource client modules |
| `crates/dapple-sdk/src/types/*.rs` | **Delete** | All 30+ hand-written type files (replaced by codegen) |

## 10. Deferred Decisions

The following are left to the implementation agent:

- **Exact typify configuration** -- version, settings for rename policies, derive macros, etc.
- **Retry logic** -- whether the SDK should auto-retry on 5xx errors or leave that to callers.
- **Rate limiting** -- whether to implement client-side rate limiting.
- **Logging** -- whether to use `log` or `tracing` (the rest of the workspace uses `log`).
- **Error detail parsing** -- how much of the error response body to include in `SdkError::Api`.
- **Streaming content type detection** -- how to handle `Content-Type` headers for asset streaming.

## 11. Open Questions and Risks

### Open Questions

1. **typify version stability**: typify is pre-1.0. The implementation agent should verify that the latest version handles the OpenAPI 3.1 schemas correctly, particularly `oneOf` with `discriminator` and `anyOf` for `Option<T>`. If typify cannot handle the full spec, fall back to a simpler approach: generate types from the `components/schemas` JSON using a custom `build.rs` that walks the JSON and emits Rust structs directly.

2. **FilterOptions location**: `FilterOptions` is defined via `$defs` in parameter schemas, not in `components/schemas`. After the $defs fixup, it should appear in `components/schemas` and be generated by typify. Verify this.

3. **Tagged enum codegen quality**: The API's `GetPlaybookSetSchema` uses `#[serde(tag = "query", rename_all = "snake_case")]`. The OpenAPI spec represents this as `oneOf` with `const` tag properties. Verify that typify generates the correct Rust enum with `#[serde(tag = "query")]`.

4. **DateTime serialization**: The API uses a custom `DateTime` wrapper that serializes as RFC3339. The OpenAPI spec exposes this as `{ "type": "string", "format": "date-time" }`. typify may generate this as `chrono::DateTime<Utc>`, `time::OffsetDateTime`, or `String`. Using `String` is simplest and avoids additional dependencies.

### Risks

- **Schema drift**: If the API spec changes and the checked-in snapshot is not updated, the SDK types will be stale. Mitigation: add a CI check that fetches the live spec from the mock server and diffs against the checked-in version.
- **typify limitations**: If typify cannot handle certain schema patterns, individual types may need manual overrides in `src/types.rs`. This is a minor risk since the fallback is straightforward.
- **Build time**: typify codegen adds time to the build. Mitigation: the generated types are cached by cargo unless `openapi/internal-api.json` changes.

## 12. Comparison: Original Plan vs. Revised Plan

| Aspect | Original (Hand-Crafted) | Revised (Hybrid Codegen) |
|--------|-------------------------|--------------------------|
| Type modules | ~30 hand-written files | 1 generated file + 1 re-export file |
| Type maintenance | Manual sync with API changes | Regenerate from spec |
| Resource clients | ~28 hand-written files | ~28 hand-written files (same) |
| Auth module | Hand-written | Hand-written (same) |
| Build complexity | Simple `cargo build` | `build.rs` with typify dependency |
| Spec dependency | None (types match by convention) | Checked-in OpenAPI JSON snapshot |
| Drift risk | High | Low (spec is single source of truth) |
| Total files | ~65 source files | ~35 source files |

The core trade-off is: a one-time investment in `build.rs` complexity eliminates ongoing type maintenance burden. Given the API surface of 305 schemas across 71 endpoints, this is a clear win.
