# SDK Overview

> **Audience:** SDK Consumers · API Consumers · Agents

The `dapple-sdk` crate provides a typed Rust HTTP client for the Dapple Prompt API. All request and response types are auto-generated from the OpenAPI specification at build time via `typify`, and resource clients expose ergonomic RPC-style methods over the standard REST envelope.

---

## Quick Start

Add the SDK to your `Cargo.toml`:

```toml
[dependencies]
dapple-sdk = { path = "../dapple-sdk" }
# Enable optional features as needed:
# dapple-sdk = { path = "../dapple-sdk", features = ["tarball", "service-auth"] }
```

Create a client and start making requests:

```rust
use dapple_sdk::{DappleClient, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    let client = DappleClient::with_api_token(
        "https://api.example.com",
        "dpl_my_token",
    );

    // List all playbooks
    let playbooks = client.playbooks().list(None).await?;
    println!("Found {} playbooks", playbooks.len());

    // Get a specific guide by slug
    let guide = client.guides().get_by_slug("build-resume").await?;
    println!("Guide: {}", guide.title);

    Ok(())
}
```

---

## Authentication

`DappleClient` supports three authentication strategies, each exposed via a dedicated constructor.

### API Token (PAT)

The simplest approach. Create a Personal Access Token through the API or UI. Tokens use a `dpl_` prefix and are sent as `Authorization: Bearer dpl_...` on every request.

```rust
let client = DappleClient::with_api_token(
    "https://api.example.com",
    "dpl_your_token_here",
);
```

### Pre-obtained JWT

If you already hold a JWT (for example, from an OAuth flow or session exchange), pass it directly. The token is sent as-is in the `Authorization: Bearer <jwt>` header.

```rust
let client = DappleClient::with_bearer_token(
    "https://api.example.com",
    "eyJhbGciOiJFZDI1NTE5...",
);
```

### Ed25519 Service Token

For service-to-service communication. The client signs a token with an Ed25519 private key, exchanges it for a JWT, and caches the JWT for reuse. On HTTP 401, the cached JWT is automatically invalidated and re-obtained.

Requires the `service-auth` feature:

```toml
dapple-sdk = { path = "../dapple-sdk", features = ["service-auth"] }
```

```rust
let signing_key: [u8; 32] = load_signing_key();
let client = DappleClient::with_service_token(
    "https://api.example.com",
    "my_service_id",
    &signing_key,
);
```

---

## Resource Clients

Every API resource is accessed through a typed sub-client returned by an accessor method on `DappleClient`. Each sub-client borrows the parent client and provides methods for the corresponding REST endpoints.

### Available Resource Clients

| Accessor Method | Resource | Typical Operations |
|----------------|----------|-------------------|
| `.playbooks()` | Playbooks | `get`, `list`, `list_by_ids`, `list_by_slugs`, `create`, `delete`, `delete_one` |
| `.journeys()` | Journeys | `get`, `list`, `list_by_ids`, `list_by_user`, `list_by_playbook`, `create`, `briefs`, `recent`, `favorite`, `unfavorite`, `is_favorite`, `favorites` |
| `.guides()` | Guides | Full CRUD, ingest, render, cards, favorites (see below) |
| `.dispatches()` | Dispatches | CRUD and lifecycle operations |
| `.briefs()` | Briefs | CRUD operations |
| `.brief_groups()` | Brief Groups | CRUD operations |
| `.brief_packs()` | Brief Packs | CRUD operations |
| `.brief_units()` | Brief Units | CRUD operations |
| `.steps()` | Steps | CRUD and slug-based queries |
| `.stages()` | Stages | CRUD and slug-based queries |
| `.tasks()` | Tasks | CRUD and slug-based queries |
| `.categories()` | Categories | CRUD operations |
| `.hats()` | Hats | CRUD operations |
| `.assets()` | Assets | Metadata and binary content |
| `.drafts()` | Drafts | Draft lifecycle |
| `.fluent_messages()` | Fluent Messages | Localized message management |
| `.prompts()` | Prompts | Versioned prompt templates |
| `.directives()` | Directives | Flow-control directive definitions |
| `.value_slots()` | Value Slots | Typed value containers |
| `.inference_profiles()` | Inference Profiles | Model routing configuration |
| `.versioned_objects()` | Version History | Query version chains by path ID |
| `.api_tokens()` | API Tokens | Token management (create, list, revoke) |
| `.user_details()` | User Details | Current user profile |
| `.deprecations()` | Deprecations | Deprecation management |
| `.dispatch_intent_bindings()` | Intent Bindings | Dispatch intent configuration |
| `.dispatch_phases()` | Dispatch Phases | Phase lifecycle lookup |
| `.dispatch_task_attempts()` | Task Attempts | Execution attempt history |

---

## GuidesClient in Detail

The `GuidesClient` is the most feature-rich resource client, covering CRUD, tarball-based ingest, PDF rendering, promotional cards, and user favorites.

### Standard CRUD

```rust
use dapple_sdk::{DappleClient, FilterOptions};
use uuid::Uuid;

let client = DappleClient::with_api_token(base_url, token);
let guides = client.guides();

// Get a single guide
let guide = guides.get(some_uuid).await?;

// List all guides with pagination
let all = guides.list(Some(FilterOptions {
    limit: Some(20),
    offset: Some(0),
})).await?;

// List by object IDs
let batch = guides.list_by_ids(vec![id1, id2], None).await?;

// List by slugs
let by_slug = guides.list_by_slugs(
    vec!["build-resume".into(), "essay-helper".into()],
    None,
).await?;

// List by path IDs (all versions of a guide lineage)
let versions = guides.list_by_path_ids(vec![path_id], None).await?;

// Convenience: get one guide by slug
let resume_guide = guides.get_by_slug("build-resume").await?;

// Create guides via direct schema (not ingest)
let created = guides.create(vec![CreateGuideSchema { /* ... */ }]).await?;

// Delete (deprecate)
let deleted = guides.delete_one(guide_id).await?;

// Deprecate with level and reason
let deprecated = guides.deprecate_one(
    path_id,
    Some("deprecated".into()),
    Some("Replaced by v2".into()),
).await?;
```

### Ingest

Ingest transforms a guide directory (or pre-built tarball) into database entities. The pipeline parses `guide.toml`, `SUMMARY.md`, chapter markdown files, Fluent `.ftl` locale files, and assets, then materializes everything atomically.

```rust
use dapple_sdk::types::IngestParams;

// Ingest from pre-built tarball bytes
let tarball_bytes: Vec<u8> = std::fs::read("my-guide.tar.gz")?;
let response = client.guides().ingest(
    tarball_bytes,
    IngestParams {
        dry_run: false,
        source_path: Some("guides/my-guide".into()),
    },
).await?;
println!("Ingest status: {}", response.status);

// Ingest from a local directory (requires "tarball" feature)
// This creates the .tar.gz in memory automatically.
let response = client.guides().ingest_from_dir(
    &std::path::Path::new("./my-guide"),
    IngestParams { dry_run: false, source_path: None },
).await?;

// Ingest a new version of an existing guide (PATCH)
let response = client.guides().ingest_version(
    existing_path_id,
    tarball_bytes,
    IngestParams { dry_run: false, source_path: None },
).await?;
```

#### IngestParams

| Field | Type | Description |
|-------|------|-------------|
| `dry_run` | `bool` | When `true`, parse and validate without writing to the database. Returns a validation summary with warnings, block counts, and derived object counts. |
| `source_path` | `Option<String>` | Override the guide's `source_path` metadata field. Useful for CI pipelines where the tarball is built from a temporary location. |

#### Dry Run

Dry runs are useful for CI validation and pre-flight checks:

```rust
let response = client.guides().ingest_from_dir(
    &Path::new("./my-guide"),
    IngestParams { dry_run: true, source_path: None },
).await?;

// The response contains validation results in `extra`
println!("Validation: {}", serde_json::to_string_pretty(&response.extra)?);
```

### Render

Generate a PDF from a guide using a named render profile:

```rust
let pdf_bytes = client.guides().render_pdf(
    guide_id,
    "consumer",  // render profile: "consumer", "technical", or "internal"
).await?;

std::fs::write("output.pdf", &pdf_bytes)?;
```

### Cards

Guide cards are promotional summary views used in carousels and discovery UIs:

```rust
// List all guide cards
let cards = client.guides().cards(None).await?;

// Filter by category
let research_cards = client.guides().cards_by_category("researcher", None).await?;
```

### Favorites

```rust
// Add to favorites
client.guides().favorite(guide_id).await?;

// Check if a guide is favorited
let check = client.guides().is_favorite(guide_id).await?;

// List all favorites
let favs = client.guides().favorites().await?;

// Remove from favorites
client.guides().unfavorite(guide_id).await?;
```

---

## Common Patterns

### Pagination

Most list methods accept an `Option<FilterOptions>` parameter for server-side pagination:

```rust
use dapple_sdk::types::FilterOptions;

let page = client.playbooks().list(Some(FilterOptions {
    limit: Some(50),
    offset: Some(100),
})).await?;
```

### Batch Operations

Create and query operations work with vectors, following the API's set-wise design:

```rust
let playbooks = client.playbooks().create(vec![
    CreatePlaybookSchema { name: "alpha".into(), /* ... */ },
    CreatePlaybookSchema { name: "beta".into(), /* ... */ },
]).await?;
// Returns Vec<Playbook> in the same order as input
```

### Request/Response Envelope

The SDK transparently handles the API's envelope format. POST endpoints wrap results in `{ "objs": [...] }` or `{ "obj": {...} }` envelopes; the SDK unwraps these so you receive the inner types directly.

---

## Error Handling

All SDK operations return `Result<T, SdkError>`. The error type covers the full range of failure modes:

```rust
use dapple_sdk::SdkError;

match client.guides().get(some_id).await {
    Ok(guide) => println!("Found: {}", guide.title),
    Err(SdkError::Http(e)) => {
        // Transport error: connection refused, timeout, DNS failure
        eprintln!("Network error: {}", e);
    }
    Err(SdkError::Api { status, message }) => {
        // Server returned an error response
        eprintln!("API error (HTTP {}): {}", status, message);
        if status == 404 {
            eprintln!("Guide not found");
        }
    }
    Err(SdkError::Deserialization(msg)) => {
        // Response body could not be parsed
        eprintln!("Parse error: {}", msg);
    }
    Err(SdkError::Serialization(msg)) => {
        // Request body or query params could not be serialized
        eprintln!("Serialization error: {}", msg);
    }
    Err(SdkError::Config(msg)) => {
        // Client configuration issue
        eprintln!("Config error: {}", msg);
    }
    Err(SdkError::Io(e)) => {
        // File I/O error (tarball creation, etc.)
        eprintln!("I/O error: {}", e);
    }
    Err(SdkError::Json(e)) => {
        // JSON parsing error
        eprintln!("JSON error: {}", e);
    }
}
```

### SdkError Variants

| Variant | Cause |
|---------|-------|
| `Http` | Transport-layer failure (connection, timeout, TLS) |
| `Api { status, message }` | Server returned a non-success HTTP status. The `message` is extracted from the API's error envelope when available. |
| `Deserialization` | Response body did not match the expected type |
| `Serialization` | Request body or query parameters could not be serialized |
| `Config` | Client misconfiguration (invalid MIME type, missing feature) |
| `Io` | File system error during tarball creation or similar |
| `Json` | JSON parsing error |

---

## Optional Features

The SDK has two optional Cargo features that are disabled by default:

| Feature | Dependencies Added | Enables |
|---------|-------------------|---------|
| `tarball` | `tar`, `flate2` | `ingest_from_dir()` -- creates `.tar.gz` archives from a local directory in memory |
| `service-auth` | `ed25519-dalek`, `rand`, `tokio` | `DappleClient::with_service_token()` -- Ed25519-based service authentication with JWT caching |

---

## HTTP Callout Notes for Non-Rust Consumers

The Dapple Prompt API follows standard REST conventions. Non-Rust consumers (Python, TypeScript, curl) can interact with the same endpoints directly.

### Authentication Header

All requests require an `Authorization` header:

```
Authorization: Bearer <token>
```

Where `<token>` is either a PAT (`dpl_...`), a JWT, or a service-exchanged JWT.

### Envelope Format

- **List endpoints** return `{ "status": "...", "objs": [...] }`
- **Single-object endpoints** return `{ "status": "...", "obj": {...} }`
- **Error responses** return `{ "status": "error"|"fail", "message": "..." }`

### Query Parameters

GET list endpoints use bracket-notation query strings for typed queries:

```
GET /api/guides/?query=by_slugs&slugs[0]=build-resume&slugs[1]=essay-helper
GET /api/guides/?query=all&options[limit]=20&options[offset]=0
```

### Guide Ingest (Multipart)

```bash
# POST new guide
curl -X POST https://api.example.com/api/guides/ingest \
  -H "Authorization: Bearer dpl_..." \
  -F "file=@my-guide.tar.gz" \
  -G -d "source_path=guides/my-guide"

# Dry run
curl -X POST https://api.example.com/api/guides/ingest?dry_run=true \
  -H "Authorization: Bearer dpl_..." \
  -F "file=@my-guide.tar.gz"

# PATCH new version
curl -X PATCH https://api.example.com/api/guides/<path_id>/ingest \
  -H "Authorization: Bearer dpl_..." \
  -F "file=@my-guide.tar.gz"
```

### PDF Render

```bash
curl -X POST https://api.example.com/api/guides/<guide_id>/render \
  -H "Authorization: Bearer dpl_..." \
  -H "Content-Type: application/json" \
  -d '{"profile": "consumer"}' \
  -o output.pdf
```
