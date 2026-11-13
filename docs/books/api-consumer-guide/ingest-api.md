# Guide Ingest API

> **Audience:** SDK Consumers · API Consumers

This chapter covers the SDK methods and HTTP endpoints for ingesting guides into the platform.

> For guide directory structure, authoring conventions, and what happens during the ingest pipeline, see the [Journey Planner Guide - Ingest Pipeline](../journey-planner-guide/ingest-pipeline.md).

---

## SDK Ingest

The `dapple-sdk` provides ingest methods on the `GuidesClient`:

### Ingest from Directory

The simplest path -- the SDK builds the tarball for you:

```rust
use dapple_sdk::{DappleClient, IngestParams};
use std::path::Path;

let client = DappleClient::with_api_token(
    "https://api.example.com",
    "dpl_your_token",
);

// Ingest from a local directory (requires `tarball` feature)
let response = client.guides().ingest_from_dir(
    Path::new("./my-guide"),
    IngestParams {
        dry_run: false,
        source_path: Some("docs/books/my-guide".into()),
    },
).await?;

println!("Ingested: {:?}", response.status);
```

### Ingest from Pre-Built Tarball

If you build the tarball yourself (e.g., in CI):

```rust
let tarball_bytes = std::fs::read("my-guide.tar.gz")?;

let response = client.guides().ingest(
    tarball_bytes,
    IngestParams {
        dry_run: false,
        source_path: None,
    },
).await?;
```

### IngestParams

| Field | Type | Description |
|-------|------|-------------|
| `dry_run` | `bool` | Parse and validate only; do not materialize |
| `source_path` | `Option<String>` | Override the guide's `source_path` metadata |

---

## HTTP API

### New Guide (POST)

`POST /api/guides/ingest`

Upload a tarball as a multipart form:

```bash
curl -X POST "https://api.example.com/api/guides/ingest?source_path=docs/books/my-guide" \
  -H "Authorization: Bearer dpl_your_token" \
  -F "file=@my-guide.tar.gz"
```

### New Version (PATCH)

`PATCH /api/guides/{path_id}/ingest`

```bash
curl -X PATCH "https://api.example.com/api/guides/019426a0-.../ingest" \
  -H "Authorization: Bearer dpl_your_token" \
  -F "file=@my-guide-v2.tar.gz"
```

Version ingest creates a new version of the guide (incrementing the patch number) while preserving the path identity. The title and slug remain on the path object.

### Query Parameters

| Parameter | Required | Description |
|-----------|----------|-------------|
| `dry_run` | No | Set to `true` to validate without materializing |
| `source_path` | No | Override the guide's `source_path` field |

---

## Dry Run Validation

Use `dry_run=true` to validate a guide without creating any database records:

**SDK:**

```rust
let response = client.guides().ingest_from_dir(
    Path::new("./my-guide"),
    IngestParams { dry_run: true, source_path: None },
).await?;
```

**curl:**

```bash
curl -X POST "https://api.example.com/api/guides/ingest?dry_run=true" \
  -H "Authorization: Bearer dpl_your_token" \
  -F "file=@my-guide.tar.gz"
```

The dry run response includes a validation summary:

```json
{
  "status": "dry_run",
  "valid": true,
  "errors": [],
  "warnings": [":::nushell block at chapter 2, block 3 has no name attribute"],
  "summary": {
    "title": "Resume Builder Guide",
    "chapters": 5,
    "blocks": 12,
    "assets": 3,
    "derived_playbook_stages": 3,
    "derived_brief_packs": 1,
    "derived_tasks": 2,
    "derived_inference_profiles": 1,
    "deprecation_level": null,
    "deprecation_reason": null
  }
}
```

Use dry runs in CI pipelines to catch errors before deployment.

---

## Ingest Response

A successful (non-dry-run) ingest returns the created guide and all derived objects. Embedding generation runs asynchronously after the response is sent — the HTTP call does not wait for embeddings to finish writing to `rev.guide_content_embeddings`. If you intend to immediately query `GET /api/guides/search`, expect a brief window where the new guide is not yet searchable. If `DAPPLE_PROMPT_CONDUIT_URL` is unset on the server or the conduit's embedding service is unavailable, ingest still succeeds; only embedding generation is skipped (a warning is logged).

Re-ingesting a guide whose canonical content hash matches the latest version short-circuits and returns the existing version — no duplicate is created.

```json
{
  "status": "success",
  "guide": {
    "id": "019426a1-...",
    "path_id": "019426a0-...",
    "title": "Resume Builder Guide",
    "slug": "resume-builder-guide",
    "version": { "major": 0, "minor": 0, "patch": 1 }
  },
  "derived_objects": {
    "playbook_ids": ["019426a2-..."],
    "task_ids": ["019426a3-...", "019426a4-..."],
    "brief_pack_ids": [],
    "inference_profile_ids": ["019426a9-..."],
    "prompt_ids": ["019426aa-..."]
  }
}
```

---

## Tarball Requirements

| Constraint | Value |
|------------|-------|
| Maximum tarball size | **50 MB** |
| Symlinks inside tarball | Rejected (use `tar czh` to follow symlinks at archive time) |
| Path traversal (`..`) | Rejected |
| Required file | `guide.toml` must exist in the tarball root or a single nested directory |

Create a tarball with:

```bash
tar czh -f my-guide.tar.gz my-guide/
```

The `h` flag follows symlinks, which is important if your guide references shared assets via symlinks.

---

## Error Responses

### Tarball Errors (400 Bad Request)

| Error | Cause |
|-------|-------|
| No file in request | Multipart form missing the `file` field |
| Tarball too large | Exceeds 50 MB limit |
| Symlink detected | Tarball contains symbolic links |
| Path traversal | Tarball contains `..` path components |
| No guide.toml found | Tarball root has no `guide.toml` |

### Parse Errors (422 Unprocessable Entity)

| Error | Cause |
|-------|-------|
| Invalid TOML | `guide.toml` has syntax errors |
| Missing title | `guide.toml` lacks required `title` field |
| Invalid semver | `version` field is not valid semver |
| Invalid deprecation level | `level` is not `deprecated`, `retired`, or `quarantined` |
| Invalid SUMMARY.md | Link-list parsing failed |
| Directive parse error | Malformed `:::` directive block |

### Materialization Errors (500 Internal Server Error)

| Error | Cause |
|-------|-------|
| Asset upload failure | Draft/chunk pipeline error |
| Database constraint violation | Unexpected duplicate or FK issue |

---

## Next Steps

- [SDK Overview](./sdk-overview.md) -- Full SDK documentation
- [Guide Cards, Favorites, and Rendering](./guide-cards-and-favorites.md) -- Cards, favorites, and PDF rendering
- [Journey Planner Guide - Ingest Pipeline](../journey-planner-guide/ingest-pipeline.md) -- Guide authoring and pipeline internals
