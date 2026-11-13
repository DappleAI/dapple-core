# Shared Content (deferred to V2+)

> **Audience:** SDK Consumers · API Consumers

> **⚠ Status — deferred to V2+:** the `_shared/` content reference system is **not currently implemented**. The `POST /api/shared-content/ingest` endpoint, the `dapple-sdk` `SharedContentClient`, the parser's `parse_shared_content` pipeline, and the `rev.guide_shared_refs` / `rev.shared_content_objects` tables were all removed when the path layer was refactored to namespace-bound `(scope, namespace_company_id)` ownership.

Shared content was intended to provide reusable building blocks (validation tasks, common glossaries, internal utilities) that multiple guides could reference via the `:::ref` directive. None of those endpoints or types exist today. When the feature returns it will use a proper `rev.shared_content_path_details` table mirroring the other path-detail tables, and the `:::ref` directive (still parsed by `dapple-parser`) will resolve against it. Until then, fold any shared building blocks directly into each guide that needs them.

---

## Next Steps

- [Guide Ingest API](./ingest-api.md) -- Ingesting full guides
- [SDK Overview](./sdk-overview.md) -- Full SDK documentation
