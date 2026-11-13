# Guide Ingest

> **Audience:** Customer Integration Engineers · Guide Authors

A guide is a structured content directory (markdown directives, TOML metadata, Fluent localization, optional assets) that the platform parses and materializes into the entities a workflow needs — playbook, stages, steps, tasks, brief packs, inference profiles. On the external surface you upload guides into your own namespace, and every object the ingest produces is stamped with your `company_id` and `scope = 'external'`.

There are two operations: a **POST** to create a new guide and a **PATCH** to re-ingest (upsert) an existing one.

## Uploading a New Guide

```
POST https://external-api.dapple.x/api/guides/ingest
Authorization: Bearer <jwt>
Content-Type: multipart/form-data
```

The tarball goes in a multipart field named **`file`**. The guide's `source_path` is read from the top-level `source_path = "..."` in `guide.toml`.

```bash
# Package the guide directory (follow symlinks with -h)
tar czh -f my-guide.tar.gz my-guide/

# Upload — note the field name is "file"
curl -X POST https://external-api.dapple.x/api/guides/ingest \
  -H "Authorization: Bearer <jwt>" \
  -F "file=@my-guide.tar.gz"
```

The ingest parses the guide, derives all entities, and creates them atomically. Every produced `object_path` is written into your namespace (`namespace_company_id = your company_id`) with `scope = 'external'` — this happens automatically via per-transaction namespace configuration, so you do not pass a namespace or scope in the request.

## Re-ingesting / Upserting a Guide

To ship a new version of a guide you already own, PATCH its path id:

```
PATCH https://external-api.dapple.x/api/guides/{path_id}/ingest
Authorization: Bearer <jwt>
Content-Type: multipart/form-data
```

```bash
curl -X PATCH https://external-api.dapple.x/api/guides/{path_id}/ingest \
  -H "Authorization: Bearer <jwt>" \
  -F "file=@my-guide.tar.gz"
```

### Outcomes: refreshed vs new_version

Guides are content-addressable. The PATCH response tells you whether your upload actually changed anything:

| Outcome | Status | Meaning |
|---------|--------|---------|
| `outcome: "refreshed"` | `200 OK` | The content hash matched the latest version. No new version was created; the existing one was refreshed in place. |
| `outcome: "new_version"` | `201 Created` | The content hash differed. A new version was created and the path now pins it. |

This lets your CI answer "did anything change?" without diffing — re-ingest unconditionally and read the outcome.

## Namespace Gating

The PATCH path is namespace-gated. A `path_id` that belongs to another company, or that does not exist, returns the same `404 RESOURCE_NOT_FOUND` body — there is no way to tell the two apart, and no way to probe another company's guides. The same namespace override applies to both the version-create and the refresh code paths, so derived `object_paths` always stay in your namespace.

## Constraints

- **Multipart field name is `file`.** A request without that field is rejected.
- **50 MB maximum tarball size.**
- **Security.** Symlinks and path traversal (`..`) inside the tarball are rejected. Use `tar czh` to follow symlinks at packaging time so they become regular files.
- **`guide.toml` required at the tarball root.** The ingest locates the guide root by finding `guide.toml`.
- **Atomic.** The guide, its chapters, blocks, tags, assets, and card are written in a single transaction.

> **Asset upload note (V1 limitation).** Guide *assets* (binary files) currently land in the dapple namespace rather than your customer namespace, because the asset-upload path runs outside the namespace-scoped transaction. For asset-free guides this is a no-op. If your guide ships assets, coordinate with your dapple contact — full per-namespace asset isolation is tracked as follow-up work.

## After Ingest

Embedding generation (for natural-language search) runs asynchronously after a successful ingest and never blocks or fails it. List your guides with `GET /api/guides` to confirm the upload landed, then create a journey against the playbook the guide exposes — see [Customer Runtime Flow](./runtime-operations.md).
